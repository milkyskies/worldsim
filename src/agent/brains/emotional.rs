//! Emotional brain: association-driven behavior based on feelings.
//!
//! Reads: EmotionalState, MindGraph, VisibleObjects, PsychologicalDrives, InConversation
//! Writes: BrainProposal
//! Upstream: perception (VisibleObjects), psyche (EmotionalState)
//! Downstream: brains::proposal (winner selection)
//!
//! Conversation continuation and turn-taking are handled by the
//! [`CommunicationPlugin`](crate::agent::communication::CommunicationPlugin).
//! The emotional brain only proposes the *initiation* of conversations
//! (`ActionType::InitiateConversation`); once registered, the plugin owns the
//! lifecycle.

use super::proposal::{BrainProposal, BrainType, Intent};
use crate::agent::actions::ActionType;
use crate::agent::body::needs::PsychologicalDrives;
use crate::agent::mind::conversation::InConversation;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::constants::brains::emotional::{
    ANGER_ENTITY_THRESHOLD, ANGER_ENTITY_URGENCY_MULTIPLIER, FEAR_ENTITY_THRESHOLD,
    FEAR_ENTITY_URGENCY_MULTIPLIER, FEAR_GENERAL_THRESHOLD, FEAR_GENERAL_URGENCY_MULTIPLIER,
    JOY_ENTITY_THRESHOLD, JOY_ENTITY_URGENCY_MULTIPLIER, SOCIAL_SEEK_THRESHOLD,
    SOCIAL_SEEK_URGENCY_MULTIPLIER,
};
use bevy::prelude::*;

pub fn emotional_brain_propose(
    emotions: &EmotionalState,
    mind: &MindGraph,
    visible: &VisibleObjects,
    drives: Option<&PsychologicalDrives>,
    in_conversation: Option<&InConversation>,
    self_concept: Option<Concept>,
    cns: &crate::agent::nervous_system::cns::CentralNervousSystem,
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<BrainProposal> {
    let mut best: Option<BrainProposal> = None;
    let mut best_urgency = 0.0f32;

    for &entity in &visible.entities {
        if let Some((proposal, urgency)) =
            evaluate_entity_emotions(entity, mind, action_registry, best_urgency)
        {
            best = Some(proposal);
            best_urgency = urgency;
        }
    }

    if let Some(proposal) = check_general_fear(emotions, best_urgency, action_registry) {
        best = Some(proposal);
    }

    // Social seeking — conversation path (humans). Identical to the
    // pre-#260 code so the if-let chain is unchanged.
    if in_conversation.is_none()
        && let Some(d) = drives
        && let Some(proposal) =
            seek_social_initiation(d.social, visible, mind, action_registry, best_urgency)
    {
        best_urgency = proposal.urgency;
        best = Some(proposal);
    }

    // Flock seeking — walk-toward-kin path (deer, wolves). Only fires for
    // non-Person species; the drive gate and urgency gate are inside the
    // function so this branch is effectively dormant for humans.
    if in_conversation.is_none()
        && let Some(d) = drives
        && let Some(self_concept) = self_concept
        && self_concept != Concept::Person
        && let Some(proposal) = seek_flock_proximity(
            d.social,
            self_concept,
            visible,
            mind,
            action_registry,
            best_urgency,
        )
    {
        best = Some(proposal);
    }

    // Ambient drives (#386). When Emotional would otherwise leave the
    // proposal empty, check the agent's CNS urgencies for drives that
    // don't have their own brain: Curiosity (wired to drives.curiosity,
    // drained by Observe/Explore/Wander/Converse) and Territoriality
    // (patrol). These are suppressed mid-conversation because their
    // actions would ride Locomotion or Cognition and break the social
    // turn (#330).
    if in_conversation.is_none() {
        if let Some(proposal) = propose_curiosity(cns, visible, mind, action_registry, best_urgency)
        {
            best_urgency = proposal.urgency;
            best = Some(proposal);
        }
        if let Some(proposal) = propose_patrol(cns, action_registry, best_urgency) {
            best = Some(proposal);
        }
    }

    // Baseline idle behaviour (#386). When nothing else is pressing,
    // Emotional proposes Groom at very low urgency — self-care as the
    // natural default instead of "stand frozen doing nothing". Any
    // real drive outbids this; it's purely the "alive but at rest"
    // hum. Suppressed in conversation so agents don't idly preen
    // mid-dialogue (which would fight for the Manipulation channel).
    if best.is_none() && in_conversation.is_none() {
        best = propose_groom_baseline(action_registry);
    }

    best
}

/// Propose `Observe` (if an agent is visible to watch) or `Explore`
/// (otherwise) when the agent's `Curiosity` urgency is active.
///
/// No thresholds. No arbitrary multipliers. `drives.curiosity` is a
/// real drainable state: it rises during unstimulating activity
/// (Idle/Sleep/Rest/Groom at ~+0.01/s) and drains from
/// Observe/Explore/Wander/Converse (via `RuntimeEffects::curiosity_per_sec`).
/// The proposal urgency follows the same `value * 40` pattern as
/// Social — comparable in weight, so the arbitrator picks whichever
/// drive is more pressing this moment.
///
/// When something interesting (another agent) is visible, Observe is
/// the specific satisfier. Otherwise the agent falls through to
/// Explore — go find something to look at.
fn propose_curiosity(
    cns: &crate::agent::nervous_system::cns::CentralNervousSystem,
    visible: &VisibleObjects,
    mind: &MindGraph,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<BrainProposal> {
    use crate::agent::nervous_system::urgency::UrgencySource;
    let u = cns
        .urgencies
        .iter()
        .find(|u| matches!(u.source, UrgencySource::Curiosity | UrgencySource::Fun))?;
    // Match Social's 40× multiplier so curiosity competes on the same
    // scale as other psychological drives. The drive itself gates
    // firing — once drives.curiosity drains below the drive config's
    // `min_threshold`, no urgency is emitted and this whole path
    // returns None on its own.
    let urgency = u.value * 40.0;
    if urgency <= min_urgency {
        return None;
    }

    // Pick the most interesting visible entity: another agent beats a
    // static object. A curious creature watches moving things, not
    // the berry bush it's seen a thousand times. Filtering to agents
    // also keeps the target fresh because agents themselves move.
    let interesting_target = visible.entities.iter().find(|&&e| is_agent(mind, e));
    if let Some(&target) = interesting_target {
        let observe = action_registry.get(ActionType::Observe)?;
        let mut template = observe.to_template(None);
        template.target_entity = Some(target);
        return Some(BrainProposal {
            brain: BrainType::Emotional,
            action: template,
            urgency,
            intent: Intent::from_urgency_source(u.source),
            reasoning: format!("Curious — watching ({:.2})", u.value),
        });
    }
    let explore = action_registry.get(ActionType::Explore)?;
    Some(BrainProposal {
        brain: BrainType::Emotional,
        action: explore.to_template(None),
        urgency,
        intent: Intent::from_urgency_source(u.source),
        reasoning: format!("Curious — exploring ({:.2})", u.value),
    })
}

/// True if the MindGraph says this entity is a Person, Deer, or Wolf —
/// i.e. another agent. Static objects (berry bushes, trees, rocks) are
/// filtered out so a curious agent doesn't freeze staring at a stump.
fn is_agent(mind: &MindGraph, entity: Entity) -> bool {
    const AGENT_CONCEPTS: &[Concept] = &[Concept::Person, Concept::Deer, Concept::Wolf];
    for concept in AGENT_CONCEPTS {
        if !mind
            .query(
                Some(&Node::Entity(entity)),
                Some(Predicate::IsA),
                Some(&Value::Concept(*concept)),
            )
            .is_empty()
        {
            return true;
        }
    }
    false
}

/// Propose `Wander` for Territoriality urgency. The agent paces a
/// short local loop to watch over familiar ground. Wander's random-
/// walkable-target picker already keeps movement local, so the effect
/// reads as patrolling without needing a dedicated patrol action.
fn propose_patrol(
    cns: &crate::agent::nervous_system::cns::CentralNervousSystem,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<BrainProposal> {
    use crate::agent::nervous_system::urgency::UrgencySource;
    let u = cns
        .urgencies
        .iter()
        .find(|u| matches!(u.source, UrgencySource::Territoriality))?;
    let urgency = u.value * 100.0;
    if urgency <= min_urgency {
        return None;
    }
    let wander = action_registry.get(ActionType::Wander)?;
    Some(BrainProposal {
        brain: BrainType::Emotional,
        action: wander.to_template(None),
        urgency,
        intent: Intent::SatisfyTerritoriality,
        reasoning: format!("Patrolling territory ({:.2})", u.value),
    })
}

/// Propose `Groom` as the zero-drive baseline. Tiny urgency so any
/// real proposal from any brain outbids it. This is what an agent
/// does when genuinely at peace: self-care, not empty waiting.
fn propose_groom_baseline(
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<BrainProposal> {
    let groom = action_registry.get(ActionType::Groom)?;
    Some(BrainProposal {
        brain: BrainType::Emotional,
        action: groom.to_template(None),
        urgency: 1.0, // below IDLE_WANDER_URGENCY and any real drive
        intent: Intent::None,
        reasoning: "At rest — grooming".to_string(),
    })
}

/// Propose `Walk` toward the highest-affection visible conspecific when
/// social drive is high. The non-Person counterpart of
/// `seek_social_initiation`: deer drift back toward herd-mates, wolves
/// rejoin pack-mates, all using the same drive that humans use to seek
/// conversation. Affection-weighted target selection means kin always
/// outrank random strangers of the same species when both are visible.
///
/// Returns `None` for solitary species (`self_concept == Concept::Person`
/// is filtered by the caller; future solitary animals like bears would
/// pass this check but find no conspecifics in their group anyway, since
/// they wouldn't be introduced as kin at spawn).
fn seek_flock_proximity(
    social_drive: f32,
    self_concept: Concept,
    visible: &VisibleObjects,
    mind: &MindGraph,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<BrainProposal> {
    if social_drive <= SOCIAL_SEEK_THRESHOLD {
        return None;
    }

    let urgency = social_drive * SOCIAL_SEEK_URGENCY_MULTIPLIER;
    if urgency <= min_urgency {
        return None;
    }

    let action = action_registry.get(ActionType::Walk)?;

    // First pass: visible kin (preferred — current position is fresh).
    let mut best_visible: Option<(Entity, f32)> = None;
    for &entity in &visible.entities {
        if !is_conspecific(mind, entity, self_concept) {
            continue;
        }
        let affection = read_affection(mind, entity);
        match best_visible {
            Some((_, current)) if affection <= current => {}
            _ => best_visible = Some((entity, affection)),
        }
    }

    if let Some((target, affection)) = best_visible {
        return Some(BrainProposal {
            brain: BrainType::Emotional,
            // Walk's target_position resolves from target_entity in execution.rs.
            action: action.to_template(Some(target)),
            urgency,
            intent: Intent::SatisfySocial,
            reasoning: format!(
                "I want to be near {target:?} (social: {social_drive:.2}, affection: {affection:.2})"
            ),
        });
    }

    // Fallback: known kin who isn't currently visible. Walk to their last
    // remembered tile. This is what makes a separated deer find its way
    // back to the herd — without it, the agent only knows to flock when
    // already in vision range, which defeats the whole "rejoin the
    // herd" semantic. Recency falls off naturally because the
    // `LocatedAt` belief is a percept and decays over time.
    let mut best_remembered: Option<(Entity, f32, (i32, i32))> = None;
    let known_conspecifics = mind.query(
        None,
        Some(Predicate::IsA),
        Some(&Value::Concept(self_concept)),
    );
    for triple in known_conspecifics {
        let Node::Entity(entity) = triple.subject else {
            continue;
        };
        // Skip currently-visible — they were handled above.
        if visible.entities.contains(&entity) {
            continue;
        }
        let affection = read_affection(mind, entity);
        // Only chase remembered kin, not random animals you once saw.
        if affection <= 0.5 {
            continue;
        }
        // Look up last remembered tile.
        let Some(Value::Tile((tx, ty))) = mind.get(&Node::Entity(entity), Predicate::LocatedAt)
        else {
            continue;
        };
        let tile = (*tx, *ty);
        match best_remembered {
            Some((_, current, _)) if affection <= current => {}
            _ => best_remembered = Some((entity, affection, tile)),
        }
    }

    let (target, affection, (tx, ty)) = best_remembered?;
    let world_pos = Vec2::new(
        tx as f32 * crate::world::map::TILE_SIZE + crate::world::map::TILE_SIZE / 2.0,
        ty as f32 * crate::world::map::TILE_SIZE + crate::world::map::TILE_SIZE / 2.0,
    );
    let mut template = action.to_template(Some(target));
    template.target_position = Some(world_pos);
    Some(BrainProposal {
        brain: BrainType::Emotional,
        action: template,
        urgency,
        intent: Intent::SatisfySocial,
        reasoning: format!(
            "I remember {target:?} was at tile ({tx}, {ty}) — heading there \
             (social: {social_drive:.2}, affection: {affection:.2})"
        ),
    })
}

fn is_conspecific(mind: &MindGraph, entity: Entity, self_concept: Concept) -> bool {
    !mind
        .query(
            Some(&Node::Entity(entity)),
            Some(Predicate::IsA),
            Some(&Value::Concept(self_concept)),
        )
        .is_empty()
}

fn read_affection(mind: &MindGraph, entity: Entity) -> f32 {
    mind.get(&Node::Entity(entity), Predicate::Affection)
        .and_then(|v| match v {
            Value::Float(f) => Some(*f),
            _ => None,
        })
        .unwrap_or(0.5)
}

/// Propose `InitiateConversation` toward a visible person if social drive is
/// high enough. Skips strangers (the agent's recognition system handles those
/// separately) — for now we accept any visible Person concept.
fn seek_social_initiation(
    social_drive: f32,
    visible: &VisibleObjects,
    mind: &MindGraph,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<BrainProposal> {
    if social_drive <= SOCIAL_SEEK_THRESHOLD {
        return None;
    }

    let urgency = social_drive * SOCIAL_SEEK_URGENCY_MULTIPLIER;
    if urgency <= min_urgency {
        return None;
    }

    let action = action_registry.get(ActionType::InitiateConversation)?;

    for &entity in &visible.entities {
        // Must be a person.
        let is_person = !mind
            .query(
                Some(&Node::Entity(entity)),
                Some(Predicate::IsA),
                Some(&Value::Concept(Concept::Person)),
            )
            .is_empty();
        if !is_person {
            continue;
        }
        return Some(BrainProposal {
            brain: BrainType::Emotional,
            action: action.to_template(Some(entity)),
            urgency,
            intent: Intent::SatisfySocial,
            reasoning: format!("I want to chat with {entity:?} (social: {social_drive:.2})"),
        });
    }

    None
}

/// Returns the first visible entity that the mind knows is `Dangerous`.
///
/// Perception writes `(entity, IsA, Concept::Wolf)` triples; `has_trait` walks
/// the IsA chain to find `(Wolf, HasTrait, Dangerous)` in the agent's knowledge.
pub(crate) fn find_most_feared_visible_entity(
    visible: &VisibleObjects,
    mind: &MindGraph,
) -> Option<Entity> {
    visible
        .entities
        .iter()
        .find(|&&e| mind.has_trait(&Node::Entity(e), Concept::Dangerous))
        .copied()
}

/// Returns (fear, joy, anger) intensities from direct and inherited associations.
fn collect_entity_feelings(entity: Entity, mind: &MindGraph) -> (f32, f32, f32) {
    let subject = Node::Entity(entity);
    let mut feelings: Vec<(EmotionType, f32)> = Vec::new();

    let mut collect = |subj: &Node| {
        for triple in mind.query(Some(subj), Some(Predicate::TriggersEmotion), None) {
            if let Value::Emotion(etype, intensity) = triple.object {
                feelings.push((etype, intensity));
            }
        }
    };

    collect(&subject);
    for concept in mind.all_types(&subject) {
        collect(&Node::Concept(concept));
    }

    let sum = |target: EmotionType| -> f32 {
        feelings
            .iter()
            .filter(|(e, _)| *e == target)
            .map(|(_, i)| i)
            .sum()
    };

    (
        sum(EmotionType::Fear),
        sum(EmotionType::Joy),
        sum(EmotionType::Anger),
    )
}

/// Returns the best (proposal, intensity) for a single entity, if above min_urgency.
fn evaluate_entity_emotions(
    entity: Entity,
    mind: &MindGraph,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<(BrainProposal, f32)> {
    let (fear, joy, anger) = collect_entity_feelings(entity, mind);
    let mut best: Option<(BrainProposal, f32)> = None;
    let mut threshold = min_urgency;

    if fear > threshold
        && fear > FEAR_ENTITY_THRESHOLD
        && let Some(action) = action_registry.get(ActionType::Flee)
    {
        best = Some((
            BrainProposal {
                brain: BrainType::Emotional,
                action: action.to_template(Some(entity)),
                urgency: fear * FEAR_ENTITY_URGENCY_MULTIPLIER,
                intent: Intent::SatisfySafety,
                reasoning: format!("I'm scared of {:?} (fear: {:.2})", entity, fear),
            },
            fear,
        ));
        threshold = fear;
    }

    if joy > threshold
        && joy > JOY_ENTITY_THRESHOLD
        && let Some(action) = action_registry.get(ActionType::Walk)
    {
        best = Some((
            BrainProposal {
                brain: BrainType::Emotional,
                action: action.to_template(Some(entity)),
                urgency: joy * JOY_ENTITY_URGENCY_MULTIPLIER,
                intent: Intent::SatisfySocial,
                reasoning: format!("I like {:?} (joy: {:.2})", entity, joy),
            },
            joy,
        ));
        threshold = joy;
    }

    if anger > threshold
        && anger > ANGER_ENTITY_THRESHOLD
        && let Some(action) = action_registry.get(ActionType::Attack)
    {
        best = Some((
            BrainProposal {
                brain: BrainType::Emotional,
                action: action.to_template(Some(entity)),
                urgency: anger * ANGER_ENTITY_URGENCY_MULTIPLIER,
                intent: Intent::SatisfySafety,
                reasoning: format!("I hate {:?}! (anger: {:.2})", entity, anger),
            },
            anger,
        ));
    }

    best
}

/// Responds to general (non-entity) fear.
fn check_general_fear(
    emotions: &EmotionalState,
    best_urgency: f32,
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<BrainProposal> {
    let fear_level: f32 = emotions
        .active_emotions
        .iter()
        .filter(|e| e.emotion_type == EmotionType::Fear)
        .map(|e| e.intensity)
        .sum();

    if fear_level <= FEAR_GENERAL_THRESHOLD {
        return None;
    }

    let fear_urgency = fear_level * FEAR_GENERAL_URGENCY_MULTIPLIER;
    if fear_urgency > best_urgency
        && let Some(action) = action_registry.get(ActionType::Flee)
    {
        return Some(BrainProposal {
            brain: BrainType::Emotional,
            action: action.to_template(None),
            urgency: fear_urgency,
            intent: Intent::SatisfySafety,
            reasoning: format!("I'm terrified! (fear: {:.2})", fear_level),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{Metadata, MindGraph, Node, Predicate, Triple, Value};
    use crate::agent::psyche::emotions::Emotion;

    fn setup_mind() -> MindGraph {
        MindGraph::default()
    }

    #[test]
    fn test_emotional_fear_response() {
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Fear, 0.9));

        let mind = setup_mind();
        let visible = VisibleObjects::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::FleeAction);

        let proposal = emotional_brain_propose(
            &state,
            &mind,
            &visible,
            None,
            None,
            None,
            &Default::default(),
            &registry,
        );

        assert!(proposal.is_some());
        let prop = proposal.unwrap();
        assert_eq!(prop.brain, BrainType::Emotional);
        assert_eq!(prop.action.name, "Flee");
        assert!(prop.urgency > 60.0);
    }

    #[test]
    fn test_emotional_entity_fear() {
        let state = EmotionalState::default();
        let entity = Entity::from_bits(42);
        let mut mind = setup_mind();

        mind.assert(Triple::with_meta(
            Node::Entity(entity),
            Predicate::TriggersEmotion,
            Value::Emotion(EmotionType::Fear, 0.8),
            Metadata::default(),
        ));

        let mut visible = VisibleObjects::default();
        visible.entities.push(entity);

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::FleeAction);

        let proposal = emotional_brain_propose(
            &state,
            &mind,
            &visible,
            None,
            None,
            None,
            &Default::default(),
            &registry,
        );

        assert!(proposal.is_some());
        let prop = proposal.unwrap();
        assert!(prop.action.name.contains("Flee"));
    }

    #[test]
    fn test_emotional_entity_joy() {
        let state = EmotionalState::default();
        let entity = Entity::from_bits(42);
        let mut mind = setup_mind();

        mind.assert(Triple::with_meta(
            Node::Entity(entity),
            Predicate::TriggersEmotion,
            Value::Emotion(EmotionType::Joy, 0.6),
            Metadata::default(),
        ));

        let mut visible = VisibleObjects::default();
        visible.entities.push(entity);

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::WalkAction);

        let proposal = emotional_brain_propose(
            &state,
            &mind,
            &visible,
            None,
            None,
            None,
            &Default::default(),
            &registry,
        );

        assert!(proposal.is_some());
        let prop = proposal.unwrap();
        assert!(prop.action.name.contains("Walk"));
    }

    #[test]
    fn test_emotional_no_response_returns_none_without_groom_registered() {
        // Pre-#386, an idle agent produced no Emotional proposal. Post-#386,
        // Emotional owns the "at rest" baseline and proposes `Groom` at tiny
        // urgency. This test uses an empty registry (no Groom action) so
        // the baseline path can't build its template, and Emotional still
        // returns None — documenting that the baseline degrades gracefully
        // when its action is unavailable.
        let state = EmotionalState::default();
        let mind = setup_mind();
        let visible = VisibleObjects::default();

        let registry = crate::agent::actions::ActionRegistry::default();
        let proposal = emotional_brain_propose(
            &state,
            &mind,
            &visible,
            None,
            None,
            None,
            &Default::default(),
            &registry,
        );

        assert!(proposal.is_none());
    }

    #[test]
    fn test_emotional_proposes_groom_baseline_when_truly_idle() {
        // With Groom registered, an idle agent (no visible entities, no
        // drives, no goals, not in conversation) gets the baseline Groom
        // proposal — the "alive but at rest" hum from #386.
        let state = EmotionalState::default();
        let mind = setup_mind();
        let visible = VisibleObjects::default();

        let mut registry = crate::agent::actions::ActionRegistry::default();
        registry.register(crate::agent::actions::action::GroomAction);

        let proposal = emotional_brain_propose(
            &state,
            &mind,
            &visible,
            None,
            None,
            None,
            &Default::default(),
            &registry,
        );

        let proposal = proposal.expect("Groom baseline should fire for a truly idle agent");
        assert_eq!(proposal.action.action_type, ActionType::Groom);
        assert!(
            proposal.urgency < 10.0,
            "Groom baseline must be very low urgency"
        );
    }
}
