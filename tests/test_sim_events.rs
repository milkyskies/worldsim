//! Integration tests for the SimEvent bus.
//!
//! Verifies that SimEvent variants are emitted during standard TestWorld scenarios
//! and that systems don't panic when no SimEvent consumer is present.

use bevy::prelude::*;
use worldsim::agent::events::SimEvent;
use worldsim::testing::{AgentConfig, TestWorld};

/// Resource that collects SimEvents across ticks for test inspection.
#[derive(Resource, Default)]
struct SimEventCollector {
    events: Vec<SimEvent>,
}

/// System that reads SimEvents and stores them in the collector resource.
fn collect_sim_events(
    mut reader: MessageReader<SimEvent>,
    mut collector: ResMut<SimEventCollector>,
) {
    for event in reader.read() {
        collector.events.push(event.clone());
    }
}

/// Helper: creates a TestWorld with the SimEvent collector system installed.
fn test_world_with_collector() -> TestWorld {
    let mut world = TestWorld::with_seed(42);
    world
        .app_mut()
        .init_resource::<SimEventCollector>()
        .add_systems(Last, collect_sim_events);
    world
}

#[test]
fn systems_run_without_panic_when_no_sim_event_reader_exists() {
    // Default TestWorld has no SimEvent consumer — events are fire-and-forget.
    let mut world = TestWorld::with_seed(42);
    world.spawn_agent(AgentConfig {
        hunger: 50.0,
        ..Default::default()
    });
    world.spawn_berry_bush(Vec2::new(20.0, 20.0), 5);
    // Tick enough for brains + actions to fire. No panic = pass.
    world.tick(100);
}

#[test]
fn decision_events_emitted_during_brain_ticks() {
    let mut world = test_world_with_collector();
    world.spawn_agent(AgentConfig {
        hunger: 50.0,
        ..Default::default()
    });
    world.spawn_berry_bush(Vec2::new(20.0, 20.0), 5);

    world.tick(60);

    let collector = world.app().world().resource::<SimEventCollector>();
    let decisions: Vec<_> = collector
        .events
        .iter()
        .filter(|e| matches!(e, SimEvent::Decision { .. }))
        .collect();
    assert!(
        !decisions.is_empty(),
        "expected at least one Decision event after 60 ticks"
    );
}

#[test]
fn action_started_events_emitted_when_actions_begin() {
    let mut world = test_world_with_collector();
    world.spawn_agent(AgentConfig {
        hunger: 80.0,
        pos: Vec2::new(18.0, 20.0),
        ..Default::default()
    });
    world.spawn_berry_bush(Vec2::new(20.0, 20.0), 5);

    world.tick(60);

    let collector = world.app().world().resource::<SimEventCollector>();
    let started: Vec<_> = collector
        .events
        .iter()
        .filter(|e| matches!(e, SimEvent::ActionStarted { .. }))
        .collect();
    assert!(
        !started.is_empty(),
        "expected at least one ActionStarted event"
    );
}

#[test]
fn entity_perceived_events_emitted_when_agents_see_new_entities() {
    let mut world = test_world_with_collector();
    // Place agent near a bush so it perceives it.
    world.spawn_agent(AgentConfig {
        pos: Vec2::new(10.0, 10.0),
        ..Default::default()
    });
    world.spawn_berry_bush(Vec2::new(15.0, 10.0), 3);

    world.tick(5);

    let collector = world.app().world().resource::<SimEventCollector>();
    let perceived: Vec<_> = collector
        .events
        .iter()
        .filter(|e| matches!(e, SimEvent::EntityPerceived { .. }))
        .collect();
    assert!(
        !perceived.is_empty(),
        "expected EntityPerceived when agent sees nearby bush"
    );
}

#[test]
fn stranger_detected_when_two_agents_meet() {
    let mut world = test_world_with_collector();
    // Place two agents close together so they perceive each other.
    world.spawn_agent(AgentConfig {
        pos: Vec2::new(10.0, 10.0),
        ..Default::default()
    });
    world.spawn_agent(AgentConfig {
        pos: Vec2::new(12.0, 10.0),
        ..Default::default()
    });

    world.tick(5);

    let collector = world.app().world().resource::<SimEventCollector>();
    let strangers: Vec<_> = collector
        .events
        .iter()
        .filter(|e| matches!(e, SimEvent::StrangerDetected { .. }))
        .collect();
    assert!(
        !strangers.is_empty(),
        "expected StrangerDetected when two agents first see each other"
    );
}

#[test]
fn action_completed_events_emitted() {
    let mut world = test_world_with_collector();
    // Place agent directly on top of the bush so it doesn't need to walk far.
    world.spawn_agent(AgentConfig {
        hunger: 90.0,
        pos: Vec2::new(20.0, 20.0),
        ..Default::default()
    });
    world.spawn_berry_bush(Vec2::new(20.0, 20.0), 5);

    // Generous tick budget — the agent needs to decide, pick up, and eat.
    world.tick(1000);

    let collector = world.app().world().resource::<SimEventCollector>();
    let completed: Vec<_> = collector
        .events
        .iter()
        .filter(|e| matches!(e, SimEvent::ActionCompleted { .. }))
        .collect();
    assert!(
        !completed.is_empty(),
        "expected at least one ActionCompleted event after 1000 ticks"
    );
}
