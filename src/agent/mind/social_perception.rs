//! Social Perception - Observe other agents' actions, mood, and states
//!
//! When agents see each other, they perceive:
//! - What the other agent is doing (current action)
//! - Their apparent mood (happy, sad, angry, fearful, neutral)
//! - Whether they appear injured
//! - Their movement direction (heading)
//! - Whether they are a stranger or someone we've met

use crate::agent::Agent;
use crate::agent::actions::registry::ActionState;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::mind::knowledge::{Concept, Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::emotions::EmotionalState;
use crate::core::tick::TickCount;
use bevy::prelude::*;

/// Conversation range in pixels (~2 tiles)
pub const CONVERSATION_RANGE: f32 = 32.0;

/// System: Perceive other agents' observable states
pub fn perceive_other_agents(
    mut observers: Query<(Entity, &Transform, &VisibleObjects, &mut MindGraph), With<Agent>>,
    observable_agents: Query<
        (
            Entity,
            &Name,
            &Transform,
            &ActionState,
            &EmotionalState,
            &PhysicalNeeds,
        ),
        With<Agent>,
    >,
    tick: Res<TickCount>,
) {
    let current_time = tick.current;

    for (observer_entity, observer_transform, visible, mut mind) in observers.iter_mut() {
        let observer_pos = observer_transform.translation.truncate();

        for &visible_entity in &visible.entities {
            // Skip self
            if visible_entity == observer_entity {
                continue;
            }

            // Only perceive other agents
            let Ok((_, name, target_transform, action_state, emotional_state, physical)) =
                observable_agents.get(visible_entity)
            else {
                continue;
            };

            let target_pos = target_transform.translation.truncate();
            let distance = observer_pos.distance(target_pos);

            // Confidence decreases with distance
            let confidence = (1.0 - (distance / 256.0).min(1.0)).max(0.3);
            let meta = Metadata::perception_with_conf(current_time, confidence);

            let target_node = Node::Entity(visible_entity);

            // 1. Perceive type: This is a Person
            mind.assert(Triple::with_meta(
                target_node.clone(),
                Predicate::IsA,
                Value::Concept(Concept::Person),
                meta.clone(),
            ));

            // 2. Perceive their current action
            mind.assert(Triple::with_meta(
                target_node.clone(),
                Predicate::Doing,
                Value::Action(action_state.action_type),
                meta.clone(),
            ));

            // 3. Perceive their apparent mood (based on emotional state)
            let apparent_mood = interpret_visible_mood(emotional_state);
            mind.assert(Triple::with_meta(
                target_node.clone(),
                Predicate::AppearsMood,
                Value::Concept(apparent_mood),
                meta.clone(),
            ));

            // 4. Perceive if they appear injured (low health or high pain markers)
            let appears_injured = physical.health < 70.0;
            mind.assert(Triple::with_meta(
                target_node.clone(),
                Predicate::AppearsInjured,
                Value::Boolean(appears_injured),
                meta.clone(),
            ));

            // 5. Store their name (if we don't know it yet, use their entity name)
            // This simulates "seeing their name tag" for now - real introductions come later
            if !mind.has(
                &target_node,
                Predicate::NameOf,
                &Value::Text(name.to_string()),
            ) {
                // Only assert if we've been introduced
                let introduced = mind.query(
                    Some(&target_node),
                    Some(Predicate::Introduced),
                    Some(&Value::Boolean(true)),
                );
                if !introduced.is_empty() {
                    mind.assert(Triple::with_meta(
                        target_node.clone(),
                        Predicate::NameOf,
                        Value::Text(name.to_string()),
                        Metadata::semantic(current_time),
                    ));
                }
            }
        }
    }
}

/// Convert internal emotional state to visible mood concept
fn interpret_visible_mood(emotional_state: &EmotionalState) -> Concept {
    use crate::agent::psyche::emotions::EmotionType;

    // Find the strongest active emotion
    let mut strongest: Option<(EmotionType, f32)> = None;

    for emotion in &emotional_state.active_emotions {
        if strongest.is_none_or(|(_, i)| emotion.intensity > i) {
            strongest = Some((emotion.emotion_type, emotion.intensity));
        }
    }

    // Map to visible mood (only if intensity is noticeable)
    match strongest {
        Some((EmotionType::Joy, i)) if i > 0.3 => Concept::HappyMood,
        Some((EmotionType::Sadness, i)) if i > 0.3 => Concept::SadMood,
        Some((EmotionType::Anger, i)) if i > 0.3 => Concept::AngryMood,
        Some((EmotionType::Fear, i)) if i > 0.3 => Concept::FearfulMood,
        _ => {
            // Default based on overall mood
            if emotional_state.current_mood > 0.3 {
                Concept::HappyMood
            } else if emotional_state.current_mood < -0.3 {
                Concept::SadMood
            } else {
                Concept::NeutralMood
            }
        }
    }
}

/// Check if two positions are within conversation range
pub fn within_conversation_range(pos_a: Vec2, pos_b: Vec2) -> bool {
    pos_a.distance(pos_b) <= CONVERSATION_RANGE
}
