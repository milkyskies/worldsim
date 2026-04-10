//! Small talk topic selection: scores triples in an agent's MindGraph for use as conversation content.
//!
//! Reads: MindGraph (speaker's triples), TheoryOfMind (speaker's belief about partner for novelty)
//! Writes: nothing (pure scoring function — no Bevy types touched)
//! Upstream: agent::mind::knowledge (Triple, Metadata, MemoryType, Source), agent::mind::theory_of_mind
//! Downstream: agent::communication::select_turn_intent (#46 will use these to fill Turn::content)
//!
//! # Design
//!
//! `pick_small_talk_triples` ranks every triple in the speaker's MindGraph by
//! a score combining recency, salience, novelty to the partner, and
//! self-relevance, then returns the top N. Mundane intrinsic/cultural triples
//! are filtered out before scoring so we never propose "food satisfies hunger"
//! as small talk.
//!
//! Novelty is estimated via the speaker's TheoryOfMind rather than by directly
//! querying the partner's MindGraph. If the speaker has no model for the partner,
//! everything is treated as novel (the "stranger model").
//!
//! The function is **pure** — it takes references and returns owned `Triple`s.
//! Tests can build a fake `MindGraph`, call the function, and check the
//! ranking without spinning up a Bevy app.

use bevy::prelude::Entity;

use crate::agent::mind::knowledge::{MemoryType, MindGraph, Node, Source, Triple};
use crate::agent::mind::theory_of_mind::TheoryOfMind;

// ============================================================================
// Tunables
// ============================================================================

/// Half-life (in ticks) for the recency score. Triples timestamped earlier
/// than this lose half their recency weight.
pub const RECENCY_HALF_LIFE_TICKS: f32 = 600.0;

/// Weight applied to the recency component of the score.
pub const RECENCY_WEIGHT: f32 = 1.0;

/// Weight applied to the salience component of the score.
pub const SALIENCE_WEIGHT: f32 = 1.5;

/// Weight applied to the novelty component (1.0 if partner doesn't know it,
/// 0.0 if partner already holds it with high confidence).
pub const NOVELTY_WEIGHT: f32 = 2.0;

/// Bonus added when the triple's subject is `Self_` (people talk about
/// themselves a little more than other things).
pub const SELF_RELEVANCE_BONUS: f32 = 0.3;

/// A triple must score at least this much to be considered worth saying.
pub const MIN_SCORE: f32 = 0.1;

// ============================================================================
// Public API
// ============================================================================

/// Pick up to `n` triples from `speaker_mind` that would make appropriate
/// small-talk content for a conversation with `partner_mind`.
///
/// Selection criteria — each candidate is scored and the top `n` returned:
/// 1. **Recency** — triples written closer to `now` score higher
/// 2. **Salience** — triples with high `meta.salience` score higher
/// 3. **Novelty** — triples the partner doesn't already know score higher
/// 4. **Self-relevance** — small bonus for triples about the speaker's own state
///
/// Mundane sources (`Intrinsic`, `Cultural`) and intrinsic memory types are
/// filtered out before scoring — everyone knows them, no point bringing them up.
pub fn pick_small_talk_triples(
    speaker_mind: &MindGraph,
    speaker_tom: Option<&TheoryOfMind>,
    listener: Entity,
    now: u64,
    n: usize,
) -> Vec<Triple> {
    if n == 0 {
        return Vec::new();
    }

    let mut scored: Vec<(f32, &Triple)> = speaker_mind
        .iter()
        .filter(|t| is_worth_sharing(t))
        .map(|t| (score_triple(t, speaker_tom, listener, now), t))
        .filter(|(s, _)| *s >= MIN_SCORE)
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    scored.into_iter().take(n).map(|(_, t)| t.clone()).collect()
}

// ============================================================================
// Filtering
// ============================================================================

/// Returns true if a triple is interesting enough to be worth bringing up in
/// small talk. Filters out universal/cultural knowledge that the partner is
/// guaranteed to already have.
fn is_worth_sharing(triple: &Triple) -> bool {
    // Mundane sources: everyone knows these.
    if matches!(triple.meta.source, Source::Intrinsic | Source::Cultural) {
        return false;
    }
    // Mundane memory types: same reason.
    if matches!(
        triple.meta.memory_type,
        MemoryType::Intrinsic | MemoryType::Cultural
    ) {
        return false;
    }
    true
}

// ============================================================================
// Scoring
// ============================================================================

/// Composite score: recency + salience + novelty + self-relevance bonus.
fn score_triple(
    triple: &Triple,
    speaker_tom: Option<&TheoryOfMind>,
    listener: Entity,
    now: u64,
) -> f32 {
    let recency = recency_score(triple.meta.timestamp, now);
    let salience = triple.meta.salience.clamp(0.0, 1.0);
    let novelty = novelty_score(triple, speaker_tom, listener);
    let self_bonus = if matches!(triple.subject, Node::Self_) {
        SELF_RELEVANCE_BONUS
    } else {
        0.0
    };

    RECENCY_WEIGHT * recency + SALIENCE_WEIGHT * salience + NOVELTY_WEIGHT * novelty + self_bonus
}

/// Exponential decay around `RECENCY_HALF_LIFE_TICKS`. Returns 1.0 for a
/// triple stamped at `now` and approaches 0 as the gap grows.
fn recency_score(timestamp: u64, now: u64) -> f32 {
    let age = now.saturating_sub(timestamp) as f32;
    (-age / RECENCY_HALF_LIFE_TICKS).exp()
}

/// Delegates to [`theory_of_mind::tom_novelty_score`].
fn novelty_score(triple: &Triple, speaker_tom: Option<&TheoryOfMind>, listener: Entity) -> f32 {
    crate::agent::mind::theory_of_mind::tom_novelty_score(triple, speaker_tom, listener)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{
        Concept, Metadata, MindGraph, Node, Predicate, Triple, Value,
    };
    use crate::agent::mind::theory_of_mind::TheoryOfMind;

    fn empty_mind() -> MindGraph {
        MindGraph::default()
    }

    fn test_entity(id: u32) -> Entity {
        Entity::from_raw(id)
    }

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
            },
        )
    }

    #[test]
    fn empty_mind_returns_no_topics() {
        let speaker = empty_mind();
        let listener = test_entity(1);
        let picks = pick_small_talk_triples(&speaker, None, listener, 100, 5);
        assert!(picks.is_empty());
    }

    #[test]
    fn requesting_zero_triples_returns_empty() {
        let mut speaker = empty_mind();
        speaker.assert(episodic(
            Node::Self_,
            Predicate::Energy,
            Value::Int(20),
            100,
            0.5,
        ));
        let listener = test_entity(1);
        let picks = pick_small_talk_triples(&speaker, None, listener, 100, 0);
        assert!(picks.is_empty());
    }

    #[test]
    fn intrinsic_triples_are_filtered_out() {
        let mut speaker = empty_mind();
        speaker.assert(Triple::with_meta(
            Node::Concept(Concept::Apple),
            Predicate::IsA,
            Value::Concept(Concept::Food),
            Metadata::default(), // Source::Intrinsic, MemoryType::Intrinsic
        ));
        let listener = test_entity(1);
        let picks = pick_small_talk_triples(&speaker, None, listener, 100, 5);
        assert!(picks.is_empty(), "intrinsic triples must be filtered");
    }

    #[test]
    fn picks_top_n_by_score() {
        let mut speaker = empty_mind();
        // Three episodic memories of varying salience.
        speaker.assert(episodic(
            Node::Concept(Concept::Deer),
            Predicate::LocatedAt,
            Value::Tile((1, 1)),
            100,
            0.9, // very salient
        ));
        speaker.assert(episodic(
            Node::Concept(Concept::AppleTree),
            Predicate::LocatedAt,
            Value::Tile((2, 2)),
            100,
            0.5,
        ));
        speaker.assert(episodic(
            Node::Concept(Concept::BerryBush),
            Predicate::LocatedAt,
            Value::Tile((3, 3)),
            100,
            0.1,
        ));
        let listener = test_entity(1);

        let picks = pick_small_talk_triples(&speaker, None, listener, 100, 2);
        assert_eq!(picks.len(), 2);
        // Highest salience should come first.
        assert_eq!(picks[0].subject, Node::Concept(Concept::Deer));
        assert_eq!(picks[1].subject, Node::Concept(Concept::AppleTree));
    }

    #[test]
    fn recent_triples_outrank_old_ones_at_equal_salience() {
        let mut speaker = empty_mind();
        speaker.assert(episodic(
            Node::Concept(Concept::Deer),
            Predicate::LocatedAt,
            Value::Tile((1, 1)),
            10, // very old (now - 10 = 990 ticks ago)
            0.5,
        ));
        speaker.assert(episodic(
            Node::Concept(Concept::AppleTree),
            Predicate::LocatedAt,
            Value::Tile((2, 2)),
            999, // recent
            0.5,
        ));
        let listener = test_entity(1);

        let picks = pick_small_talk_triples(&speaker, None, listener, 1000, 1);
        assert_eq!(picks.len(), 1);
        assert_eq!(picks[0].subject, Node::Concept(Concept::AppleTree));
    }

    #[test]
    fn novel_triples_outrank_known_ones() {
        let mut speaker = empty_mind();
        let listener = test_entity(1);
        speaker.assert(episodic(
            Node::Concept(Concept::Deer),
            Predicate::LocatedAt,
            Value::Tile((1, 1)),
            500,
            0.5,
        ));
        speaker.assert(episodic(
            Node::Concept(Concept::AppleTree),
            Predicate::LocatedAt,
            Value::Tile((2, 2)),
            500,
            0.5,
        ));

        // Speaker believes listener already knows about the deer.
        let mut tom = TheoryOfMind::default();
        tom.record_belief(
            listener,
            Node::Concept(Concept::Deer),
            Predicate::LocatedAt,
            Value::Tile((1, 1)),
            1.0,
            500,
        );

        let picks = pick_small_talk_triples(&speaker, Some(&tom), listener, 600, 1);
        assert_eq!(picks.len(), 1);
        assert_eq!(
            picks[0].subject,
            Node::Concept(Concept::AppleTree),
            "should prefer the novel apple-tree fact over the already-known deer fact"
        );
    }

    #[test]
    fn self_relevance_bonus_breaks_ties() {
        let mut speaker = empty_mind();
        // Both at salience 0.3, same timestamp, same novelty.
        speaker.assert(episodic(
            Node::Self_,
            Predicate::Energy,
            Value::Int(20),
            500,
            0.3,
        ));
        speaker.assert(episodic(
            Node::Concept(Concept::Deer),
            Predicate::LocatedAt,
            Value::Tile((1, 1)),
            500,
            0.3,
        ));
        let listener = test_entity(1);

        let picks = pick_small_talk_triples(&speaker, None, listener, 600, 1);
        assert_eq!(picks.len(), 1);
        assert_eq!(
            picks[0].subject,
            Node::Self_,
            "self-state triples should win at equal score"
        );
    }

    #[test]
    fn min_score_filters_out_truly_uninteresting_triples() {
        let mut speaker = empty_mind();
        let listener = test_entity(1);
        // Salience 0, very old, speaker believes listener knows it → score should fall below MIN_SCORE.
        speaker.assert(episodic(
            Node::Concept(Concept::Deer),
            Predicate::LocatedAt,
            Value::Tile((1, 1)),
            0, // ancient
            0.0,
        ));
        let mut tom = TheoryOfMind::default();
        tom.record_belief(
            listener,
            Node::Concept(Concept::Deer),
            Predicate::LocatedAt,
            Value::Tile((1, 1)),
            1.0,
            0,
        );

        let picks = pick_small_talk_triples(&speaker, Some(&tom), listener, 100_000, 5);
        assert!(picks.is_empty(), "uninteresting triples should be filtered");
    }
}
