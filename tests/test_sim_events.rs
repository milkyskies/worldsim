//! Integration tests for the SimEvent bus.
//!
//! Verifies that SimEvent variants are emitted during standard TestWorld scenarios
//! and that systems don't panic when no SimEvent consumer is present.

use bevy::prelude::*;
use worldsim::agent::events::SimEvent;
use worldsim::agent::inventory::Inventory;
use worldsim::agent::mind::knowledge::Concept;
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
    let mut world = TestWorld::with_seed(42);
    world.spawn_agent(AgentConfig {
        hunger: 50.0,
        ..Default::default()
    });
    world.spawn_berry_bush(Vec2::new(20.0, 20.0), 5);
    world.tick(100);
}

#[test]
fn entity_perceived_events_emitted_when_agents_see_new_entities() {
    let mut world = test_world_with_collector();
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
fn brain_and_action_lifecycle_events_emitted() {
    let mut world = test_world_with_collector();
    // Hungry agent with food already in inventory — will eat without needing to
    // walk or harvest, guaranteeing a short Eat action (20 ticks) completes.
    let agent = world.spawn_agent(AgentConfig {
        hunger: 90.0,
        pos: Vec2::new(20.0, 20.0),
        ..Default::default()
    });
    world
        .app_mut()
        .world_mut()
        .get_mut::<Inventory>(agent)
        .unwrap()
        .add(Concept::Berry, 5);

    // Brain thinking_interval is 60 ticks. 300 ticks gives ~5 brain cycles,
    // plenty for Decision → ActionStarted → ActionCompleted (Eat = 20 ticks).
    world.tick(300);

    let collector = world.app().world().resource::<SimEventCollector>();

    let has_decision = collector
        .events
        .iter()
        .any(|e| matches!(e, SimEvent::Decision { .. }));
    let has_action_started = collector
        .events
        .iter()
        .any(|e| matches!(e, SimEvent::ActionStarted { .. }));
    let has_action_completed = collector
        .events
        .iter()
        .any(|e| matches!(e, SimEvent::ActionCompleted { .. }));

    assert!(has_decision, "expected Decision events after 300 ticks");
    assert!(
        has_action_started,
        "expected ActionStarted events after 300 ticks"
    );
    assert!(
        has_action_completed,
        "expected ActionCompleted events after 300 ticks"
    );
}
