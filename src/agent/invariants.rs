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
use crate::agent::mind::conversation::{ConversationManager, ConversationState, InConversation};
use crate::agent::psyche::emotions::EmotionalState;

/// Plugin that wires per-tick invariant checks into the simulation. The check
/// runs as an exclusive system in `Last` so panics propagate to the caller
/// (instead of being swallowed by Bevy's parallel executor). Only registered
/// in debug builds — release builds pay zero cost.
pub struct InvariantPlugin;

impl Plugin for InvariantPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(debug_assertions)]
        app.add_systems(Last, check_invariants_system);
        let _ = app;
    }
}

#[cfg(debug_assertions)]
fn check_invariants_system(world: &mut World) {
    assert_invariants(world);
}

/// Runs every invariant check against the given world. Panics with a
/// descriptive message on the first violation. Callable directly from tests
/// so failures aren't masked by mid-tick clamping in other systems.
pub fn assert_invariants(world: &World) {
    check_physical_needs(world);
    check_consciousness(world);
    check_psychological_drives(world);
    check_emotions(world);
    check_bodies(world);
    check_conversations(world);
}

fn check_physical_needs(world: &World) {
    for entity_ref in world.iter_entities() {
        if !entity_ref.contains::<Agent>() {
            continue;
        }
        let Some(needs) = entity_ref.get::<PhysicalNeeds>() else {
            continue;
        };
        let entity = entity_ref.id();
        assert_in_range(entity, "hunger", needs.hunger, 0.0, 100.0);
        assert_in_range(entity, "thirst", needs.thirst, 0.0, 100.0);
        assert_in_range(entity, "energy", needs.energy, 0.0, 100.0);
        assert_in_range(entity, "health", needs.health, 0.0, 100.0);
    }
}

fn check_consciousness(world: &World) {
    for entity_ref in world.iter_entities() {
        if !entity_ref.contains::<Agent>() {
            continue;
        }
        let Some(c) = entity_ref.get::<Consciousness>() else {
            continue;
        };
        assert_in_range(entity_ref.id(), "alertness", c.alertness, 0.0, 1.0);
    }
}

fn check_psychological_drives(world: &World) {
    for entity_ref in world.iter_entities() {
        if !entity_ref.contains::<Agent>() {
            continue;
        }
        let Some(d) = entity_ref.get::<PsychologicalDrives>() else {
            continue;
        };
        let entity = entity_ref.id();
        assert_in_range(entity, "drive.social", d.social, 0.0, 1.0);
        assert_in_range(entity, "drive.fun", d.fun, 0.0, 1.0);
        assert_in_range(entity, "drive.curiosity", d.curiosity, 0.0, 1.0);
        assert_in_range(entity, "drive.status", d.status, 0.0, 1.0);
        assert_in_range(entity, "drive.security", d.security, 0.0, 1.0);
        assert_in_range(entity, "drive.autonomy", d.autonomy, 0.0, 1.0);
    }
}

fn check_emotions(world: &World) {
    for entity_ref in world.iter_entities() {
        if !entity_ref.contains::<Agent>() {
            continue;
        }
        let Some(state) = entity_ref.get::<EmotionalState>() else {
            continue;
        };
        let entity = entity_ref.id();
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
}

fn check_bodies(world: &World) {
    for entity_ref in world.iter_entities() {
        if !entity_ref.contains::<Agent>() {
            continue;
        }
        let Some(body) = entity_ref.get::<Body>() else {
            continue;
        };
        let entity = entity_ref.id();
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

fn check_conversations(world: &World) {
    let conv_manager = world.resource::<ConversationManager>();
    for entity_ref in world.iter_entities() {
        if !entity_ref.contains::<Agent>() {
            continue;
        }
        let Some(in_conv) = entity_ref.get::<InConversation>() else {
            continue;
        };
        let entity = entity_ref.id();
        let conversation = conv_manager
            .conversations
            .get(&in_conv.conversation_id)
            .unwrap_or_else(|| {
                panic!(
                    "agent {entity:?} references non-existent conversation {}",
                    in_conv.conversation_id
                )
            });
        assert!(
            conversation.state != ConversationState::Ended,
            "agent {entity:?} still attached to ended conversation {}",
            in_conv.conversation_id,
        );
        assert!(
            conversation.participants.contains(&entity),
            "agent {entity:?} marked InConversation {} but is not a participant",
            in_conv.conversation_id,
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
    use crate::agent::psyche::emotions::{Emotion, EmotionType};
    use crate::testing::{AgentConfig, TestWorld};

    #[test]
    fn fresh_agent_passes_invariants() {
        let mut world = TestWorld::new();
        let _ = world.spawn_agent(AgentConfig::default());
        world.tick(1);
        assert_invariants(world.app().world());
    }

    #[test]
    fn ticking_many_times_does_not_violate_invariants() {
        let mut world = TestWorld::new();
        let _ = world.spawn_agent(AgentConfig {
            hunger: 50.0,
            ..Default::default()
        });
        world.spawn_apple_tree(Vec2::new(20.0, 20.0), 10);
        world.tick(120);
        assert_invariants(world.app().world());
    }

    #[test]
    #[should_panic(expected = "hunger out of range")]
    fn out_of_range_hunger_is_caught() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig::default());
        world.get_mut::<PhysicalNeeds>(agent).hunger = 150.0;
        assert_invariants(world.app().world());
    }

    #[test]
    #[should_panic(expected = "stress_level out of range")]
    fn out_of_range_stress_is_caught() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig::default());
        world.get_mut::<EmotionalState>(agent).stress_level = -5.0;
        assert_invariants(world.app().world());
    }

    #[test]
    #[should_panic(expected = "invalid fuel")]
    fn negative_emotion_fuel_is_caught() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig::default());
        let mut state = world.get_mut::<EmotionalState>(agent);
        state.active_emotions.push(Emotion {
            emotion_type: EmotionType::Joy,
            intensity: 0.5,
            fuel: -1.0,
        });
        drop(state);
        assert_invariants(world.app().world());
    }

    #[test]
    #[should_panic(expected = "function_rate out of range")]
    fn out_of_range_body_function_rate_is_caught() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig::default());
        world.get_mut::<Body>(agent).head.function_rate = 1.5;
        assert_invariants(world.app().world());
    }

    #[test]
    #[should_panic(expected = "non-existent conversation")]
    fn dangling_conversation_reference_is_caught() {
        let mut world = TestWorld::new();
        let a = world.spawn_agent(AgentConfig::default());
        let b = world.spawn_agent(AgentConfig::at(Vec2::new(5.0, 0.0)));
        world
            .app_mut()
            .world_mut()
            .entity_mut(a)
            .insert(InConversation {
                conversation_id: 9_999,
                partner: b,
                my_turn: true,
                owes_response: false,
            });
        assert_invariants(world.app().world());
    }
}
