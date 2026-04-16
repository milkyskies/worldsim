//! Integration tests for the decay_relationships ECS wiring.
//!
//! The pure-math behavior (half-life model, strength scaling, negativity bias)
//! is unit-tested in `src/agent/psyche/relationships.rs`. These tests only
//! verify the two things that require the ECS:
//!   - Decay fires at `decay_interval_ticks` boundaries.
//!   - A relationship updated within the grace window is skipped.
//!
//! Tests override `RelationshipConfig` with a small `decay_interval_ticks`
//! so they don't need to tick through 86,400 ticks per game day.

use bevy::prelude::*;
use worldsim::agent::mind::knowledge::{
    Metadata, MindGraph, Node, Predicate, Quantity, Triple, Value,
};
use worldsim::agent::psyche::relationships::RelationshipConfig;
use worldsim::testing::{AgentConfig, TestWorld};

fn set_trust(world: &mut TestWorld, agent: Entity, other: Entity, trust: f32, timestamp: u64) {
    world
        .app_mut()
        .world_mut()
        .get_mut::<MindGraph>(agent)
        .expect("agent should have MindGraph")
        .assert(Triple::with_meta(
            Node::Entity(other),
            Predicate::Trust,
            Value::Quantity(Quantity::Exact(trust)),
            Metadata::semantic(timestamp),
        ));
}

fn get_trust(world: &TestWorld, agent: Entity, other: Entity) -> Option<f32> {
    world
        .app()
        .world()
        .get::<MindGraph>(agent)
        .and_then(|mind| mind.get(&Node::Entity(other), Predicate::Trust))
        .and_then(|v| v.as_quantity().map(|q| q.point_estimate()))
}

/// Override decay config so the test can tick a handful of ticks instead
/// of marching through a full game day (86,400 ticks).
fn set_fast_decay(world: &mut TestWorld, interval_ticks: u64, grace_ticks: u64) {
    let mut config = world
        .app_mut()
        .world_mut()
        .resource_mut::<RelationshipConfig>();
    config.decay_interval_ticks = interval_ticks;
    config.decay_grace_ticks = grace_ticks;
}

/// When the grace window has passed and the decay interval is hit, trust
/// should move from its starting value toward neutral.
#[test]
fn decay_fires_and_pulls_trust_toward_neutral() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());
    // Bare entity so no social system refreshes the timestamp.
    let other = world.app_mut().world_mut().spawn_empty().id();

    // Fire every 10 ticks with no grace period.
    set_fast_decay(&mut world, 10, 0);

    // Set trust at tick 0; advance past the first decay boundary at tick 10.
    set_trust(&mut world, agent, other, 0.8, 0);
    world.tick(11);

    let trust = get_trust(&world, agent, other).expect("trust should still exist");
    assert!(
        trust < 0.8,
        "trust should have decayed from 0.8, got {trust}"
    );
    assert!(
        trust > 0.5,
        "trust should not overshoot neutral, got {trust}"
    );
}

/// A relationship refreshed within the grace window must not decay.
#[test]
fn recent_interaction_skips_decay() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());
    let other = world.app_mut().world_mut().spawn_empty().id();

    // Fire every 10 ticks; grace window of 20 ticks.
    set_fast_decay(&mut world, 10, 20);

    // Refresh trust at tick 5. At the next decay fire (tick 10), age = 5, which
    // is well within the 20-tick grace window → should be skipped.
    world.tick(5);
    set_trust(&mut world, agent, other, 0.8, 5);
    world.tick(6);

    let trust = get_trust(&world, agent, other).expect("trust should still exist");
    assert_eq!(
        trust, 0.8,
        "trust refreshed within the grace window should not decay"
    );
}
