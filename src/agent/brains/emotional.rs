use super::proposal::{BrainProposal, BrainType};
use crate::agent::actions::ActionType;
use crate::agent::mind::perception::VisibleObjects;

use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use bevy::prelude::*;

/// The Emotional Brain: Association-driven behavior based on feelings
///
/// Characteristics:
/// - Uses learned associations ("I like/fear this thing")
/// - Responds to current emotional state
/// - Doesn't plan, but has memory
/// - Can be "irrational" (phobias, attachments)
/// - Medium response time
pub fn emotional_brain_propose(
    emotions: &EmotionalState,
    mind: &crate::agent::mind::knowledge::MindGraph,
    visible: &VisibleObjects,
    drives: Option<&crate::agent::body::needs::PsychologicalDrives>,
    action_registry: &crate::agent::actions::ActionRegistry,
    in_conversation: Option<&crate::agent::mind::conversation::InConversation>,
) -> Option<BrainProposal> {
    // PRIORITY: Handle active conversation first
    if let Some(in_conv) = in_conversation {
        if in_conv.my_turn {
            use crate::agent::mind::conversation::Topic;

            let social_drive = drives.map(|d| d.social).unwrap_or(0.0);

            // Priority 1: Must respond if we owe a response
            if in_conv.owes_response {
                if let Some(action) = action_registry.get(ActionType::Talk) {
                    let mut template = action.to_template(Some(in_conv.partner), None);
                    template.topic = Some(Topic::General); // Respond with general topic
                    // In future: use actual topic from conversation history for specific answer

                    return Some(BrainProposal {
                        brain: BrainType::Emotional,
                        action: template,
                        urgency: 90.0, // Very high - must respond!
                        reasoning: "Responding to question".to_string(),
                    });
                }
            }

            // Priority 2: Check if we want to leave
            // If social drive satisfied (< 0.2) or urgent needs (> 80)
            let social_satisfied = social_drive < 0.2;
            let others_urgent = false; // TODO check external urgency

            if social_satisfied && !in_conv.owes_response {
                // For now, we just continue the conversation or let it naturally end
                // TODO: Add explicit farewell handling
            }

            // Priority 3: Continue conversation if we still have social drive
            if social_drive > 0.2 {
                if let Some(action) = action_registry.get(ActionType::Talk) {
                    let mut template = action.to_template(Some(in_conv.partner), None);
                    template.topic = Some(Topic::General);

                    return Some(BrainProposal {
                        brain: BrainType::Emotional,
                        action: template,
                        urgency: 70.0, // High - maintain conversation
                        reasoning: format!("Continuing conversation (social: {:.2})", social_drive),
                    });
                }
            }
        }
        // If it's not our turn, don't propose Talk - wait for partner
    }

    let mut best: Option<BrainProposal> = None;
    let mut best_urgency = 0.0;

    // Check feelings about each visible entity
    for &entity in &visible.entities {
        // Query MindGraph for emotions triggered by this entity
        // We look for direct associations: (Entity, TriggersEmotion, Value::Emotion)
        // AND inherited associations (via IsA)

        use crate::agent::mind::knowledge::{Node, Predicate, Value};

        let subject = Node::Entity(entity);
        let mut feelings = Vec::new();

        // Helper to collect emotions
        let mut collect_emotions = |subj: &Node| {
            let triples = mind.query(Some(subj), Some(Predicate::TriggersEmotion), None);
            for triple in triples {
                if let Value::Emotion(etype, intensity) = triple.object {
                    feelings.push((etype, intensity));
                }
            }
        };

        // 1. Direct entity feelings
        collect_emotions(&subject);

        // 2. Inherited feelings (Concepts)
        for concept in mind.all_types(&subject) {
            collect_emotions(&Node::Concept(concept));
        }

        // Calculate total intensity for each emotion type
        let fear_intensity: f32 = feelings
            .iter()
            .filter(|(e, _)| *e == EmotionType::Fear)
            .map(|(_, i)| i)
            .sum();

        let joy_intensity: f32 = feelings
            .iter()
            .filter(|(e, _)| *e == EmotionType::Joy)
            .map(|(_, i)| i)
            .sum();

        let anger_intensity: f32 = feelings
            .iter()
            .filter(|(e, _)| *e == EmotionType::Anger)
            .map(|(_, i)| i)
            .sum();

        // Strong fear about this entity? AVOID (Flee)
        if fear_intensity > best_urgency
            && fear_intensity > 0.3
            && let Some(action) = action_registry.get(ActionType::Flee)
        {
            best = Some(BrainProposal {
                brain: BrainType::Emotional,
                action: action.to_template(Some(entity), None),
                urgency: fear_intensity * 80.0,
                reasoning: format!("I'm scared of {:?} (fear: {:.2})", entity, fear_intensity),
            });
            best_urgency = fear_intensity;
        }

        // Strong positive feeling? APPROACH (Walk)
        if joy_intensity > best_urgency
            && joy_intensity > 0.3
            && let Some(action) = action_registry.get(ActionType::Walk)
        {
            best = Some(BrainProposal {
                brain: BrainType::Emotional,
                action: action.to_template(Some(entity), None),
                urgency: joy_intensity * 50.0,
                reasoning: format!("I like {:?} (joy: {:.2})", entity, joy_intensity),
            });
            best_urgency = joy_intensity;
        }

        // Strong anger? ATTACK (if above threshold)
        if anger_intensity > best_urgency
            && anger_intensity > 0.5
            && let Some(action) = action_registry.get(ActionType::Attack)
        {
            best = Some(BrainProposal {
                brain: BrainType::Emotional,
                action: action.to_template(Some(entity), None),
                urgency: anger_intensity * 60.0,
                reasoning: format!("I hate {:?}! (anger: {:.2})", entity, anger_intensity),
            });
            best_urgency = anger_intensity;
        }
    }

    // Also respond to current emotional state (not entity-specific)
    // General fear/anxiety - seek safety
    let fear_level: f32 = emotions
        .active_emotions
        .iter()
        .filter(|e| e.emotion_type == EmotionType::Fear)
        .map(|e| e.intensity)
        .sum();

    if fear_level > 0.7 {
        let fear_urgency = fear_level * 90.0;
        if fear_urgency > best_urgency
            && let Some(action) = action_registry.get(ActionType::Flee)
        {
            best = Some(BrainProposal {
                brain: BrainType::Emotional,
                action: action.to_template(None, None),
                urgency: fear_urgency,
                reasoning: format!("I'm terrified! (fear: {:.2})", fear_level),
            });
        }
    }

    // SOCIAL: Look for someone to talk to when we feel lonely/social
    // Read social drive directly from PsychologicalDrives component (not MindGraph)
    use crate::agent::mind::knowledge::{Node, Predicate, Value};
    let social_drive = drives.map(|d| d.social).unwrap_or(0.0);

    // If social drive > 0.3, look for friendly entities to talk to
    if social_drive > 0.3 {
        for &entity in &visible.entities {
            // Only consider entities that are Persons (not trees, rocks, etc.)
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
                continue; // Skip non-person entities
            }

            // Check if we've already introduced ourselves to this entity
            let introduced = !mind
                .query(
                    Some(&Node::Entity(entity)),
                    Some(Predicate::Introduced),
                    Some(&Value::Boolean(true)),
                )
                .is_empty();

            // Check trust level (positive = friendly)
            let trust = mind
                .query(Some(&Node::Entity(entity)), Some(Predicate::Trust), None)
                .first()
                .and_then(|t| match &t.object {
                    Value::Float(v) => Some(*v),
                    _ => None,
                })
                .unwrap_or(0.0);

            if introduced && trust >= 0.0 {
                let talk_urgency = social_drive * 40.0 + trust * 10.0;
                if talk_urgency > best_urgency
                    && let Some(action) = action_registry.get(ActionType::Talk)
                {
                    // Create template with Topic::General for social chat
                    let mut template = action.to_template(Some(entity), None);
                    template.topic = Some(crate::agent::mind::conversation::Topic::General);

                    best = Some(BrainProposal {
                        brain: BrainType::Emotional,
                        action: template,
                        urgency: talk_urgency,
                        reasoning: format!(
                            "I want to chat with {:?} (social: {:.2}, trust: {:.2})",
                            entity, social_drive, trust
                        ),
                    });
                    best_urgency = talk_urgency;
                }
            } else if !introduced {
                // Stranger! Let's introduce ourselves if we're feeling social
                let intro_urgency = social_drive * 35.0; // Slightly less than talking to friends
                if intro_urgency > best_urgency
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
                    best_urgency = intro_urgency;
                }
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
        assert_eq!(prop.action.name, "Flee"); // SeekSafety now maps to Flee if found
        assert!(prop.urgency > 60.0);
    }

    #[test]
    fn test_emotional_entity_fear() {
        let state = EmotionalState::default();
        let entity = Entity::from_bits(42); // Arbitrary ID
        let mut mind = setup_mind();

        // Add fear association for entity
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

        // Add joy association (liking someone)
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
