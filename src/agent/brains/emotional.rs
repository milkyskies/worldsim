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

    // Social seeking. Two parallel branches share the same `social` drive
    // gate (#260): Persons get the conversation path; non-Person
    // conspecifics (deer in herds, wolves in packs) get a flock-walk
    // toward the highest-affection visible kin.
    if in_conversation.is_none()
        && let Some(d) = drives
    {
        if let Some(proposal) =
            seek_social_initiation(d.social, visible, mind, action_registry, best_urgency)
        {
            best_urgency = proposal.urgency;
            best = Some(proposal);
        }

        if let Some(self_concept) = self_concept
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
    }

    best
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

    let mut best: Option<(Entity, f32)> = None;
    for &entity in &visible.entities {
        // Same-species check via the perception-written `IsA` triple.
        let is_conspecific = !mind
            .query(
                Some(&Node::Entity(entity)),
                Some(Predicate::IsA),
                Some(&Value::Concept(self_concept)),
            )
            .is_empty();
        if !is_conspecific {
            continue;
        }

        let affection = mind
            .get(&Node::Entity(entity), Predicate::Affection)
            .and_then(|v| match v {
                Value::Float(f) => Some(*f),
                _ => None,
            })
            .unwrap_or(0.5);

        match best {
            Some((_, current)) if affection <= current => {}
            _ => best = Some((entity, affection)),
        }
    }

    let (target, affection) = best?;
    Some(BrainProposal {
        brain: BrainType::Emotional,
        // Walk's target_position resolves from target_entity in execution.rs.
        action: action.to_template(Some(target)),
        urgency,
        intent: Intent::SatisfySocial,
        reasoning: format!(
            "I want to be near {target:?} (social: {social_drive:.2}, affection: {affection:.2})"
        ),
    })
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

        let proposal =
            emotional_brain_propose(&state, &mind, &visible, None, None, None, &registry);

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

        let proposal =
            emotional_brain_propose(&state, &mind, &visible, None, None, None, &registry);

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

        let proposal =
            emotional_brain_propose(&state, &mind, &visible, None, None, None, &registry);

        assert!(proposal.is_some());
        let prop = proposal.unwrap();
        assert!(prop.action.name.contains("Walk"));
    }

    #[test]
    fn test_emotional_no_response() {
        let state = EmotionalState::default();
        let mind = setup_mind();
        let visible = VisibleObjects::default();

        let registry = crate::agent::actions::ActionRegistry::default();
        let proposal =
            emotional_brain_propose(&state, &mind, &visible, None, None, None, &registry);

        assert!(proposal.is_none());
    }
}
