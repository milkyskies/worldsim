//! Continuous invariant assertions that catch silent state corruption.
//!
//! Reads: PhysicalNeeds, Consciousness, PsychologicalDrives, EmotionalState, Body, InConversation, ConversationManager
//! Writes: nothing (pure assertions; panics on violation)
//! Upstream: agent::AgentPlugin (registered in `Last` schedule, debug builds only)
//! Downstream: tests, debug runs (catches range/state-consistency bugs immediately)

use bevy::ecs::world::World;
use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::mind::conversation::{ConversationState, InConversation};
use crate::agent::psyche::emotions::EmotionalState;

/// Plugin that wires per-tick invariant checks into the simulation. The check
/// runs as an exclusive system in `Last` so panics propagate to the caller
/// (instead of being swallowed by Bevy's parallel executor). Only registered
/// in debug builds — release builds pay zero cost.
pub struct InvariantPlugin;

impl Plugin for InvariantPlugin {
    #[cfg_attr(not(debug_assertions), allow(unused_variables))]
    fn build(&self, app: &mut App) {
        #[cfg(debug_assertions)]
        app.add_systems(Last, check_invariants_system);
    }
}

#[cfg(debug_assertions)]
fn check_invariants_system(world: &mut World) {
    assert_invariants(world);
}

/// Runs every invariant check against the given world. Panics with a
/// descriptive message on the first violation. Callable directly from tests
/// so failures aren't masked by mid-tick clamping in other systems.
///
/// Walks every agent archetype exactly twice: once for component-only checks
/// (which can use a fused tuple query) and once for `InConversation` checks
/// (which need an additional `ConversationManager` resource borrow).
pub fn assert_invariants(world: &mut World) {
    check_components(world);
    check_conversations(world);
}

fn check_components(world: &mut World) {
    let mut query = world.query_filtered::<(
        Entity,
        Option<&PhysicalNeeds>,
        Option<&Consciousness>,
        Option<&PsychologicalDrives>,
        Option<&EmotionalState>,
        Option<&Body>,
    ), With<Agent>>();

    for (entity, needs, consciousness, drives, emotions, body) in query.iter(world) {
        if let Some(n) = needs {
            assert_in_range(
                entity,
                "metabolism.stomach_carbs",
                n.metabolism.stomach_carbs,
                0.0,
                crate::agent::body::metabolism::STOMACH_CAPACITY,
            );
            assert_in_range(
                entity,
                "metabolism.stomach_fat",
                n.metabolism.stomach_fat,
                0.0,
                crate::agent::body::metabolism::STOMACH_CAPACITY,
            );
            assert_in_range(
                entity,
                "metabolism.glucose",
                n.metabolism.glucose,
                0.0,
                crate::agent::body::metabolism::GLUCOSE_MAX,
            );
            assert_in_range(
                entity,
                "metabolism.reserves",
                n.metabolism.reserves,
                0.0,
                crate::agent::body::metabolism::RESERVES_MAX,
            );
            assert_in_range(entity, "hydration", n.hydration.value, 0.0, 1.0);
            assert_in_range(
                entity,
                "stamina.aerobic",
                n.stamina.aerobic,
                0.0,
                n.stamina.aerobic_max,
            );
            assert_in_range(
                entity,
                "stamina.anaerobic",
                n.stamina.anaerobic,
                0.0,
                n.stamina.anaerobic_max,
            );
            assert_in_range(entity, "wakefulness", n.wakefulness.value, 0.0, 1.0);
        }
        if let Some(c) = consciousness {
            assert_in_range(entity, "alertness", c.alertness, 0.0, 1.0);
        }
        if let Some(d) = drives {
            assert_in_range(entity, "drive.companionship", d.companionship.value, 0.0, 1.0);
            assert_in_range(entity, "drive.enjoyment", d.enjoyment.value, 0.0, 1.0);
            assert_in_range(entity, "drive.stimulation", d.stimulation.value, 0.0, 1.0);
            assert_in_range(entity, "drive.esteem", d.esteem.value, 0.0, 1.0);
            assert_in_range(entity, "drive.safety", d.safety.value, 0.0, 1.0);
            assert_in_range(entity, "drive.autonomy", d.autonomy.value, 0.0, 1.0);
            assert_in_range(entity, "drive.dominion", d.dominion.value, 0.0, 1.0);
        }
        if let Some(state) = emotions {
            assert_in_range(entity, "mood", state.current_mood, -1.0, 1.0);
            assert_in_range(entity, "stress_level", state.stress_level, 0.0, 100.0);
            for emotion in &state.active_emotions {
                assert_in_range(entity, "emotion.intensity", emotion.intensity, 0.0, 1.0);
                assert!(
                    emotion.fuel.is_finite() && emotion.fuel >= 0.0,
                    "agent {entity:?} emotion {:?} has invalid fuel: {}",
                    emotion.emotion_type,
                    emotion.fuel,
                );
            }
        }
        if let Some(body) = body {
            for part in body.parts() {
                assert!(
                    part.max_hp.is_finite() && part.max_hp > 0.0,
                    "agent {entity:?} body part has non-positive max_hp: {}",
                    part.max_hp,
                );
                assert!(
                    part.current_hp.is_finite() && part.current_hp >= 0.0,
                    "agent {entity:?} body part has negative current_hp: {}",
                    part.current_hp,
                );
                assert!(
                    part.current_hp <= part.max_hp,
                    "agent {entity:?} body part current_hp {} exceeds max_hp {}",
                    part.current_hp,
                    part.max_hp,
                );
                assert_in_range(entity, "body.function_rate", part.function_rate, 0.0, 1.0);
            }
        }
    }
}

fn check_conversations(world: &mut World) {
    // Snapshot agent → conversation_id pairs first so the resource borrow
    // doesn't conflict with the query borrow on `world`.
    let mut query = world.query_filtered::<(Entity, &InConversation), With<Agent>>();
    let snapshot: Vec<(Entity, u64)> = query
        .iter(world)
        .map(|(entity, in_conv)| (entity, in_conv.conversation_id))
        .collect();
    if snapshot.is_empty() {
        return;
    }

    let manager = world.resource::<crate::agent::mind::conversation::ConversationManager>();
    for (entity, conversation_id) in snapshot {
        let conversation = manager
            .conversations
            .get(&conversation_id)
            .unwrap_or_else(|| {
                panic!("agent {entity:?} references non-existent conversation {conversation_id}")
            });
        assert!(
            conversation.state != ConversationState::Ended,
            "agent {entity:?} still attached to ended conversation {conversation_id}",
        );
        assert!(
            conversation.participants.contains(&entity),
            "agent {entity:?} marked InConversation {conversation_id} but is not a participant",
        );
    }
}

#[track_caller]
fn assert_in_range(entity: Entity, label: &str, value: f32, min: f32, max: f32) {
    assert!(
        value.is_finite(),
        "agent {entity:?} {label} is not finite: {value}",
    );
    assert!(
        value >= min && value <= max,
        "agent {entity:?} {label} out of range [{min}, {max}]: {value}",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::biology::body::BodyNodeKind;
    use crate::agent::psyche::emotions::{Emotion, EmotionType};
    use crate::testing::{AgentConfig, TestWorld};

    #[test]
    fn fresh_agent_passes_invariants() {
        let mut world = TestWorld::new();
        world.spawn_agent(AgentConfig::default());
        world.tick(1);
        assert_invariants(world.app_mut().world_mut());
    }

    #[test]
    fn ticking_many_times_does_not_violate_invariants() {
        let mut world = TestWorld::new();
        world.spawn_agent(AgentConfig {
            metabolism: crate::agent::body::metabolism::Metabolism::at_urgency(0.5),
            ..Default::default()
        });
        world.spawn_apple_tree(Vec2::new(20.0, 20.0), 10);
        world.tick(120);
        assert_invariants(world.app_mut().world_mut());
    }

    #[test]
    #[should_panic(expected = "metabolism.glucose out of range")]
    fn out_of_range_glucose_is_caught() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig::default());
        world.get_mut::<PhysicalNeeds>(agent).metabolism.glucose = 1_000.0;
        assert_invariants(world.app_mut().world_mut());
    }

    #[test]
    #[should_panic(expected = "stress_level out of range")]
    fn out_of_range_stress_is_caught() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig::default());
        world.get_mut::<EmotionalState>(agent).stress_level = -5.0;
        assert_invariants(world.app_mut().world_mut());
    }

    #[test]
    #[should_panic(expected = "invalid fuel")]
    fn negative_emotion_fuel_is_caught() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig::default());
        {
            let mut state = world.get_mut::<EmotionalState>(agent);
            state.active_emotions.push(Emotion {
                emotion_type: EmotionType::Joy,
                intensity: 0.5,
                fuel: -1.0,
            });
        }
        assert_invariants(world.app_mut().world_mut());
    }

    #[test]
    #[should_panic(expected = "function_rate out of range")]
    fn out_of_range_body_function_rate_is_caught() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig::default());
        let mut body = world.get_mut::<Body>(agent);
        body.part_mut(BodyNodeKind::Head)
            .expect("human body has head")
            .function_rate = 1.5;
        assert_invariants(world.app_mut().world_mut());
    }

    #[test]
    #[should_panic(expected = "non-existent conversation")]
    fn dangling_conversation_reference_is_caught() {
        let mut world = TestWorld::new();
        let a = world.spawn_agent(AgentConfig::default());
        let _b = world.spawn_agent(AgentConfig::at(Vec2::new(5.0, 0.0)));
        world
            .app_mut()
            .world_mut()
            .entity_mut(a)
            .insert(InConversation {
                conversation_id: 9_999,
            });
        assert_invariants(world.app_mut().world_mut());
    }
}
