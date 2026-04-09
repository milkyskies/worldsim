//! Emotional brain: association-driven behavior based on feelings.
//!
//! Reads: EmotionalState, MindGraph, VisibleObjects, PsychologicalDrives, InConversation
//! Writes: BrainProposal
//! Upstream: perception (VisibleObjects), psyche (EmotionalState, PsychologicalDrives)
//! Downstream: brains::proposal (winner selection)

use super::proposal::{BrainProposal, BrainType};
use crate::agent::actions::ActionType;
use crate::agent::body::needs::PsychologicalDrives;
use crate::agent::mind::conversation::{InConversation, Topic};
use crate::agent::mind::knowledge::{MindGraph, Node, Predicate, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::constants::brains::emotional::{
    ANGER_ENTITY_THRESHOLD, ANGER_ENTITY_URGENCY_MULTIPLIER, CONVERSATION_CONTINUE_URGENCY,
    CONVERSATION_RESPONSE_URGENCY, CONVERSATION_SOCIAL_THRESHOLD, FEAR_ENTITY_THRESHOLD,
    FEAR_ENTITY_URGENCY_MULTIPLIER, FEAR_GENERAL_THRESHOLD, FEAR_GENERAL_URGENCY_MULTIPLIER,
    INTRODUCE_SOCIAL_URGENCY_MULTIPLIER, JOY_ENTITY_THRESHOLD, JOY_ENTITY_URGENCY_MULTIPLIER,
    SOCIAL_SEEK_THRESHOLD, TALK_SOCIAL_URGENCY_MULTIPLIER, TALK_TRUST_URGENCY_BONUS,
};
use bevy::prelude::*;

pub fn emotional_brain_propose(
    emotions: &EmotionalState,
    mind: &MindGraph,
    visible: &VisibleObjects,
    drives: Option<&PsychologicalDrives>,
    action_registry: &crate::agent::actions::ActionRegistry,
    in_conversation: Option<&InConversation>,
) -> Option<BrainProposal> {
    if let Some(in_conv) = in_conversation
        && let Some(proposal) = handle_active_conversation(in_conv, drives, action_registry)
    {
        return Some(proposal);
    }

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

    // General fear does NOT update best_urgency — intentional: social seeking
    // compares against the entity-loop threshold, not the general fear urgency.
    if let Some(proposal) = check_general_fear(emotions, best_urgency, action_registry) {
        best = Some(proposal);
    }

    let social_drive = drives.map(|d| d.social).unwrap_or(0.0);
    if let Some(proposal) =
        seek_social_interaction(social_drive, visible, mind, action_registry, best_urgency)
    {
        best = Some(proposal);
    }

    best
}

fn handle_active_conversation(
    in_conv: &InConversation,
    drives: Option<&PsychologicalDrives>,
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<BrainProposal> {
    if !in_conv.my_turn {
        return None;
    }

    let social_drive = drives.map(|d| d.social).unwrap_or(0.0);

    if in_conv.owes_response
        && let Some(action) = action_registry.get(ActionType::Talk)
    {
        let mut template = action.to_template(Some(in_conv.partner), None);
        template.topic = Some(Topic::General);
        return Some(BrainProposal {
            brain: BrainType::Emotional,
            action: template,
            urgency: CONVERSATION_RESPONSE_URGENCY,
            reasoning: "Responding to question".to_string(),
        });
    }

    if social_drive > CONVERSATION_SOCIAL_THRESHOLD
        && let Some(action) = action_registry.get(ActionType::Talk)
    {
        let mut template = action.to_template(Some(in_conv.partner), None);
        template.topic = Some(Topic::General);
        return Some(BrainProposal {
            brain: BrainType::Emotional,
            action: template,
            urgency: CONVERSATION_CONTINUE_URGENCY,
            reasoning: format!("Continuing conversation (social: {:.2})", social_drive),
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
                action: action.to_template(Some(entity), None),
                urgency: fear * FEAR_ENTITY_URGENCY_MULTIPLIER,
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
                action: action.to_template(Some(entity), None),
                urgency: joy * JOY_ENTITY_URGENCY_MULTIPLIER,
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
                action: action.to_template(Some(entity), None),
                urgency: anger * ANGER_ENTITY_URGENCY_MULTIPLIER,
                reasoning: format!("I hate {:?}! (anger: {:.2})", entity, anger),
            },
            anger,
        ));
    }

    best
}

/// Responds to general (non-entity) fear. Does NOT expose updated urgency so
/// that social seeking uses the entity-loop threshold.
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
            action: action.to_template(None, None),
            urgency: fear_urgency,
            reasoning: format!("I'm terrified! (fear: {:.2})", fear_level),
        });
    }

    None
}

/// Seeks social interaction when social drive exceeds threshold.
/// Returns the best social proposal above min_urgency, if any.
fn seek_social_interaction(
    social_drive: f32,
    visible: &VisibleObjects,
    mind: &MindGraph,
    action_registry: &crate::agent::actions::ActionRegistry,
    min_urgency: f32,
) -> Option<BrainProposal> {
    if social_drive <= SOCIAL_SEEK_THRESHOLD {
        return None;
    }

    let mut best: Option<BrainProposal> = None;
    let mut threshold = min_urgency;

    for &entity in &visible.entities {
        let is_person = !mind
            .query(
                Some(&Node::Entity(entity)),
                Some(Predicate::IsA),
                Some(&Value::Concept(
                    crate::agent::mind::knowledge::Concept::Person,
                )),
            )
            .is_empty();

        if !is_person {
            continue;
        }

        let introduced = !mind
            .query(
                Some(&Node::Entity(entity)),
                Some(Predicate::Introduced),
                Some(&Value::Boolean(true)),
            )
            .is_empty();

        let trust = mind
            .query(Some(&Node::Entity(entity)), Some(Predicate::Trust), None)
            .first()
            .and_then(|t| match &t.object {
                Value::Float(v) => Some(*v),
                _ => None,
            })
            .unwrap_or(0.0);

        if introduced && trust >= 0.0 {
            let talk_urgency =
                social_drive * TALK_SOCIAL_URGENCY_MULTIPLIER + trust * TALK_TRUST_URGENCY_BONUS;
            if talk_urgency > threshold
                && let Some(action) = action_registry.get(ActionType::Talk)
            {
                let mut template = action.to_template(Some(entity), None);
                template.topic = Some(Topic::General);
                best = Some(BrainProposal {
                    brain: BrainType::Emotional,
                    action: template,
                    urgency: talk_urgency,
                    reasoning: format!(
                        "I want to chat with {:?} (social: {:.2}, trust: {:.2})",
                        entity, social_drive, trust
                    ),
                });
                threshold = talk_urgency;
            }
        } else if !introduced {
            let intro_urgency = social_drive * INTRODUCE_SOCIAL_URGENCY_MULTIPLIER;
            if intro_urgency > threshold
                && let Some(action) = action_registry.get(ActionType::Introduce)
            {
                best = Some(BrainProposal {
                    brain: BrainType::Emotional,
                    action: action.to_template(Some(entity), None),
                    urgency: intro_urgency,
                    reasoning: format!(
                        "I should introduce myself to {:?} (social: {:.2})",
                        entity, social_drive
                    ),
                });
                threshold = intro_urgency;
            }
        }
    }

    best
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

        let proposal = emotional_brain_propose(&state, &mind, &visible, None, &registry, None);

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

        let proposal = emotional_brain_propose(&state, &mind, &visible, None, &registry, None);

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

        let proposal = emotional_brain_propose(&state, &mind, &visible, None, &registry, None);

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
        let proposal = emotional_brain_propose(&state, &mind, &visible, None, &registry, None);

        assert!(proposal.is_none());
    }
}
