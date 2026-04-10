//! Verifies relationship decay behavior:
//! 1. Trust/affection decay toward neutral (0.5) over time without contact.
//! 2. A recent interaction resets the decay window so values are NOT decayed
//!    until the agent has been out of contact for a full decay period.

use bevy::prelude::*;
use worldsim::agent::mind::knowledge::{Metadata, MindGraph, Node, Predicate, Triple, Value};
use worldsim::testing::{AgentConfig, TestWorld};

/// Decay period in ticks (must match decay_relationships: every 300 ticks).
const DECAY_PERIOD: u64 = 300;

fn set_trust(world: &mut TestWorld, agent: Entity, other: Entity, trust: f32, timestamp: u64) {
    world
        .app_mut()
        .world_mut()
        .get_mut::<MindGraph>(agent)
        .expect("agent should have MindGraph")
        .assert(Triple::with_meta(
            Node::Entity(other),
            Predicate::Trust,
            Value::Float(trust),
            Metadata::semantic(timestamp),
        ));
}

fn get_trust(world: &TestWorld, agent: Entity, other: Entity) -> Option<f32> {
    world
        .app()
        .world()
        .get::<MindGraph>(agent)
        .and_then(|mind| mind.get(&Node::Entity(other), Predicate::Trust))
        .and_then(|v| {
            if let Value::Float(f) = v {
                Some(*f)
            } else {
                None
            }
        })
}

#[test]
fn trust_decays_toward_neutral_without_contact() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());
    // Use a bare entity (not an agent) so no social-interaction system can
    // refresh the trust timestamp during the ticking window.
    let other = world.app_mut().world_mut().spawn_empty().id();

    // Set trust well above neutral at tick 0.
    set_trust(&mut world, agent, other, 0.8, 0);

    // Tick one past the decay period boundary. decay_relationships and
    // deterministic_tick have no explicit ordering, so decay may read
    // tick.current before it is incremented in a given update. Ticking
    // DECAY_PERIOD + 1 guarantees tick 300 is visible to the decay system
    // regardless of which system runs first within the Update schedule.
    world.tick(DECAY_PERIOD + 1);

    let trust = get_trust(&world, agent, other).expect("trust should still exist");
    assert!(
        trust < 0.8,
        "trust ({trust}) should have decayed from 0.8 toward 0.5 after {DECAY_PERIOD} ticks"
    );
    assert!(
        trust > 0.5,
        "trust ({trust}) should still be above neutral 0.5 after one decay step"
    );
}

#[test]
fn trust_does_not_decay_within_one_period_of_last_interaction() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());
    let other = world.spawn_agent(AgentConfig::default());

    // Advance close to the first decay tick.
    world.tick(DECAY_PERIOD - 5);

    // Set trust at this point — simulating a recent interaction.
    // The trust triple timestamp = DECAY_PERIOD - 5.
    let current_tick = DECAY_PERIOD - 5;
    set_trust(&mut world, agent, other, 0.8, current_tick);

    // Tick 5 more ticks: crosses tick DECAY_PERIOD, which fires decay.
    // ticks_since_last_interaction = DECAY_PERIOD - (DECAY_PERIOD - 5) = 5
    // 5 < DECAY_PERIOD → should NOT decay.
    world.tick(5);

    let trust = get_trust(&world, agent, other).expect("trust should still exist");
    assert_eq!(
        trust, 0.8,
        "trust should not decay when the last interaction was only 5 ticks ago \
         (within the {DECAY_PERIOD}-tick decay window)"
    );
}
