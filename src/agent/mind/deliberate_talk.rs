//! Deliberate content selection: picks triples an agent *wants* to share based on their goal.
//!
//! Reads: MindGraph (speaker and partner), Goal (from RationalBrain)
//! Writes: nothing (pure scoring function — returns owned Triples and a Topic)
//! Upstream: agent::mind::knowledge (Triple, Metadata), agent::brains::thinking (Goal)
//! Downstream: agent::communication::select_turn_intent (fills Turn::content)
//!
//! # Design
//!
//! Unlike small talk (which picks whatever is interesting), deliberate sharing is
//! goal-directed. If the speaker has a current goal, triples relevant to its
//! conditions are boosted. Relationship and self-state predicates are excluded —
//! those are not deliberate sharing candidates. Topic is inferred from the content
//! so the listener knows what the speaker is talking about.

use crate::agent::brains::thinking::Goal;
use crate::agent::mind::conversation::Topic;
use crate::agent::mind::knowledge::{
    Concept, MemoryType, MindGraph, Node, Predicate, Source, Triple, Value,
};
use crate::agent::mind::small_talk::{RECENCY_HALF_LIFE_TICKS, RECENCY_WEIGHT, SALIENCE_WEIGHT};

// ============================================================================
// Tunables
// ============================================================================

/// Extra score added when a triple matches a condition in the speaker's current goal.
pub const GOAL_RELEVANCE_BONUS: f32 = 3.0;

/// Higher novelty weight than small talk — we only share things they don't know.
pub const DELIBERATE_NOVELTY_WEIGHT: f32 = 3.0;

/// Minimum score for deliberate content (stricter than small talk).
pub const DELIBERATE_MIN_SCORE: f32 = 0.3;

// ============================================================================
// Public API
// ============================================================================

/// Pick up to `n` triples from `speaker_mind` to share deliberately with a partner,
/// optionally weighted toward the speaker's current `goal`.
///
/// Returns the selected triples and a topic inferred from the dominant content type.
///
/// Selection criteria (scored, top `n` returned):
/// 1. **Goal relevance** — triples whose predicate or subject matches goal conditions
/// 2. **Novelty** — triples the partner doesn't know score much higher (stricter than small talk)
/// 3. **Salience** — high-salience triples score higher
/// 4. **Recency** — recently experienced triples score higher
///
/// Relationship predicates and self-state predicates are excluded — deliberate sharing
/// is about world knowledge, not personal feelings.
pub fn pick_deliberate_content(
    speaker_mind: &MindGraph,
    goal: Option<&Goal>,
    partner_mind: &MindGraph,
    now: u64,
    n: usize,
) -> (Vec<Triple>, Topic) {
    if n == 0 {
        return (Vec::new(), Topic::General);
    }

    let mut scored: Vec<(f32, &Triple)> = speaker_mind
        .iter()
        .filter(|t| is_deliberate_shareable(t))
        .map(|t| {
            let s = score_deliberate(t, goal, partner_mind, now);
            (s, t)
        })
        .filter(|(s, _)| *s >= DELIBERATE_MIN_SCORE)
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let triples: Vec<Triple> = scored.into_iter().take(n).map(|(_, t)| t.clone()).collect();
    let topic = infer_topic(&triples);
    (triples, topic)
}

// ============================================================================
// Filtering
// ============================================================================

/// Returns true if a triple is appropriate deliberate sharing content.
///
/// Excludes:
/// - Universal knowledge (Intrinsic/Cultural sources)
/// - Self-state (Hunger, Thirst, Energy, Pain, SocialDrive)
/// - Relationship dimensions (Trust, Affection, Respect, PowerBalance)
/// - Social perception (Doing, AppearsMood, AppearsInjured, Heading)
fn is_deliberate_shareable(triple: &Triple) -> bool {
    if matches!(triple.meta.source, Source::Intrinsic | Source::Cultural) {
        return false;
    }
    if matches!(
        triple.meta.memory_type,
        MemoryType::Intrinsic | MemoryType::Cultural
    ) {
        return false;
    }
    // Exclude self-state — not useful to share with others
    if matches!(
        triple.predicate,
        Predicate::Hunger
            | Predicate::Thirst
            | Predicate::Energy
            | Predicate::Pain
            | Predicate::SocialDrive
    ) {
        return false;
    }
    // Exclude relationship data — personal, not shareable world-knowledge
    if matches!(
        triple.predicate,
        Predicate::Trust
            | Predicate::Affection
            | Predicate::Respect
            | Predicate::PowerBalance
            | Predicate::Doing
            | Predicate::AppearsMood
            | Predicate::AppearsInjured
            | Predicate::Heading
    ) {
        return false;
    }
    true
}

// ============================================================================
// Scoring
// ============================================================================

fn score_deliberate(
    triple: &Triple,
    goal: Option<&Goal>,
    partner_mind: &MindGraph,
    now: u64,
) -> f32 {
    let recency = recency_score(triple.meta.timestamp, now);
    let salience = triple.meta.salience.clamp(0.0, 1.0);
    let novelty = novelty_score(triple, partner_mind);
    let goal_bonus = goal.map(|g| goal_relevance(triple, g)).unwrap_or(0.0);

    RECENCY_WEIGHT * recency
        + SALIENCE_WEIGHT * salience
        + DELIBERATE_NOVELTY_WEIGHT * novelty
        + GOAL_RELEVANCE_BONUS * goal_bonus
}

/// Exponential decay around `RECENCY_HALF_LIFE_TICKS`.
fn recency_score(timestamp: u64, now: u64) -> f32 {
    let age = now.saturating_sub(timestamp) as f32;
    (-age / RECENCY_HALF_LIFE_TICKS).exp()
}

/// 1.0 if partner has no *personal* record of this triple, scaling toward 0.0
/// as their personal confidence grows.
///
/// We deliberately ignore the partner's ontology and shared cultural knowledge
/// here. A personal observation (e.g. "I just saw a dangerous wolf") is socially
/// novel even when the partner abstractly knows the same fact from the ontology
/// — the value of sharing is the *specific lived observation*, not the abstract
/// category. Checking the full mind would silently suppress warnings about
/// known-dangerous-but-not-yet-personally-observed threats.
fn novelty_score(triple: &Triple, partner_mind: &MindGraph) -> f32 {
    let known = partner_mind
        .iter()
        .filter(|t| {
            t.subject == triple.subject
                && t.predicate == triple.predicate
                && t.object == triple.object
        })
        .map(|t| t.meta.confidence.clamp(0.0, 1.0))
        .fold(0.0_f32, f32::max);

    1.0 - known
}

/// Returns 1.0 if the triple's predicate or subject matches any goal condition, else 0.0.
fn goal_relevance(triple: &Triple, goal: &Goal) -> f32 {
    for cond in &goal.conditions {
        if let Some(pred) = cond.predicate
            && triple.predicate == pred
        {
            return 1.0;
        }
        if let Some(subj) = &cond.subject
            && &triple.subject == subj
        {
            return 0.5;
        }
    }
    0.0
}

// ============================================================================
// Topic inference
// ============================================================================

/// Infer the conversation topic from the selected content.
///
/// - `LocatedAt` / `Contains` triples about a named concept → `Topic::Location(concept)`
/// - `HasTrait(Dangerous)` triples → `Topic::Help` (warning about danger)
/// - Anything else → `Topic::General`
pub fn infer_topic(triples: &[Triple]) -> Topic {
    if triples.is_empty() {
        return Topic::General;
    }

    // Check for danger warnings first — highest priority
    let has_danger = triples.iter().any(|t| {
        t.predicate == Predicate::HasTrait && t.object == Value::Concept(Concept::Dangerous)
    });
    if has_danger {
        return Topic::Help;
    }

    // Location knowledge — find the most salient LocatedAt triple
    let best_location = triples
        .iter()
        .filter(|t| matches!(t.predicate, Predicate::LocatedAt | Predicate::Contains))
        .max_by(|a, b| {
            a.meta
                .salience
                .partial_cmp(&b.meta.salience)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some(triple) = best_location
        && let Node::Concept(concept) = triple.subject
    {
        return Topic::Location(concept);
    }

    Topic::General
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{Metadata, Node, Predicate, Triple, Value};

    fn episodic(
        subject: Node,
        predicate: Predicate,
        object: Value,
        ts: u64,
        salience: f32,
    ) -> Triple {
        Triple::with_meta(
            subject,
            predicate,
            object,
            Metadata {
                source: Source::Experienced,
                memory_type: MemoryType::Episodic,
                timestamp: ts,
                confidence: 1.0,
                informant: None,
                evidence: Vec::new(),
                salience,
                source_sense: None,
            },
        )
    }

    fn empty_mind() -> MindGraph {
        MindGraph::default()
    }

    #[test]
    fn empty_mind_returns_no_content() {
        let speaker = empty_mind();
        let partner = empty_mind();
        let (triples, topic) = pick_deliberate_content(&speaker, None, &partner, 100, 5);
        assert!(triples.is_empty());
        assert_eq!(topic, Topic::General);
    }

    #[test]
    fn self_state_predicates_are_excluded() {
        let mut speaker = empty_mind();
        speaker.assert(episodic(
            Node::Self_,
            Predicate::Hunger,
            Value::Int(80),
            100,
            0.9,
        ));
        speaker.assert(episodic(
            Node::Self_,
            Predicate::Energy,
            Value::Int(10),
            100,
            0.9,
        ));
        let partner = empty_mind();
        let (triples, _) = pick_deliberate_content(&speaker, None, &partner, 100, 5);
        assert!(triples.is_empty(), "self-state predicates must be excluded");
    }

    #[test]
    fn relationship_predicates_are_excluded() {
        let mut speaker = empty_mind();
        let e = bevy::ecs::entity::Entity::from_bits(1);
        speaker.assert(episodic(
            Node::Entity(e),
            Predicate::Trust,
            Value::Float(0.9),
            100,
            0.9,
        ));
        speaker.assert(episodic(
            Node::Entity(e),
            Predicate::Affection,
            Value::Float(0.8),
            100,
            0.9,
        ));
        let partner = empty_mind();
        let (triples, _) = pick_deliberate_content(&speaker, None, &partner, 100, 5);
        assert!(
            triples.is_empty(),
            "relationship predicates must be excluded"
        );
    }

    #[test]
    fn location_knowledge_is_selected() {
        let mut speaker = empty_mind();
        speaker.assert(episodic(
            Node::Concept(Concept::AppleTree),
            Predicate::LocatedAt,
            Value::Tile((3, 4)),
            100,
            0.8,
        ));
        let partner = empty_mind();
        let (triples, topic) = pick_deliberate_content(&speaker, None, &partner, 100, 5);
        assert_eq!(triples.len(), 1);
        assert!(matches!(topic, Topic::Location(Concept::AppleTree)));
    }

    #[test]
    fn danger_warning_sets_help_topic() {
        let mut speaker = empty_mind();
        speaker.assert(episodic(
            Node::Concept(Concept::Wolf),
            Predicate::HasTrait,
            Value::Concept(Concept::Dangerous),
            100,
            0.9,
        ));
        let partner = empty_mind();
        let (_, topic) = pick_deliberate_content(&speaker, None, &partner, 100, 5);
        assert_eq!(topic, Topic::Help);
    }

    #[test]
    fn goal_relevant_triple_outranks_unrelated() {
        use crate::agent::brains::thinking::{Goal, TriplePattern};

        let mut speaker = empty_mind();
        // High-salience food location (goal-relevant: LocatedAt predicate matches goal)
        speaker.assert(episodic(
            Node::Concept(Concept::BerryBush),
            Predicate::LocatedAt,
            Value::Tile((1, 1)),
            50, // older
            0.5,
        ));
        // Very salient but not goal-relevant
        speaker.assert(episodic(
            Node::Concept(Concept::Deer),
            Predicate::HasTrait,
            Value::Concept(Concept::Prey),
            50,
            0.9,
        ));

        let goal_with_location = Goal {
            conditions: vec![TriplePattern::new(None, Some(Predicate::LocatedAt), None)],
            priority: 1.0,
        };

        let partner = empty_mind();
        let (triples, _) =
            pick_deliberate_content(&speaker, Some(&goal_with_location), &partner, 100, 1);
        assert_eq!(triples.len(), 1);
        // BerryBush LocatedAt should win due to goal relevance bonus
        assert_eq!(triples[0].subject, Node::Concept(Concept::BerryBush));
    }

    #[test]
    fn already_known_triple_scores_lower() {
        let mut speaker = empty_mind();
        speaker.assert(episodic(
            Node::Concept(Concept::AppleTree),
            Predicate::LocatedAt,
            Value::Tile((1, 1)),
            100,
            0.8,
        ));
        speaker.assert(episodic(
            Node::Concept(Concept::BerryBush),
            Predicate::LocatedAt,
            Value::Tile((2, 2)),
            100,
            0.8,
        ));

        // Partner already knows about the apple tree
        let mut partner = empty_mind();
        partner.assert(Triple::with_meta(
            Node::Concept(Concept::AppleTree),
            Predicate::LocatedAt,
            Value::Tile((1, 1)),
            Metadata {
                source: Source::Experienced,
                memory_type: MemoryType::Episodic,
                timestamp: 100,
                confidence: 1.0,
                informant: None,
                evidence: Vec::new(),
                salience: 0.8,
                source_sense: None,
            },
        ));

        let (triples, _) = pick_deliberate_content(&speaker, None, &partner, 100, 1);
        assert_eq!(triples.len(), 1);
        assert_eq!(
            triples[0].subject,
            Node::Concept(Concept::BerryBush),
            "should prefer the novel berry bush over the already-known apple tree"
        );
    }
}
