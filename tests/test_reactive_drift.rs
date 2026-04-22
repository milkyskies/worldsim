//! Integration tests for the reactive drift behavior layer (#640).
//!
//! Reactive drift is a behavior layer that steers agents toward ambient
//! comfort between acute survival actions. Phase 1 covers the warmth
//! axis: cold humans should walk toward visible heat sources, mirroring
//! the way lonely deer walk toward visible herd-mates.

use bevy::math::Vec2;
use worldsim::agent::actions::ActionType;
use worldsim::agent::body::need::Need;
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::testing::{AgentConfig, TestWorld};

/// A cold human who can see a campfire should start walking toward it.
/// Mirrors `lonely_deer_with_visible_kin_walks_toward_them` from the
/// flocking tests — the same "drift toward drive-relieving emitter"
/// mechanism, applied to the Warmth axis.
///
/// Geometry: campfire at 90px. Human vision range is 100px and TestWorld
/// pins `LightLevel(1.0)`, so the campfire is visible. 90px is outside
/// the 80px HeatSource radius, so proximity warming isn't already
/// solving the deficit for us.
#[test]
fn cold_human_walks_toward_visible_campfire() {
    let mut world = TestWorld::with_seed(42);

    // Spawn the campfire first and tick once so `derive_ontology_heat_source`
    // writes `Campfire HasTrait HeatEmitting` into the shared Ontology before
    // the agent's MindGraph clones it. Without this, the agent's ontology
    // snapshot predates the derivation and has_trait(HeatEmitting) returns
    // false even though the campfire is visible.
    let campfire = world.spawn_campfire(Vec2::new(90.0, 0.0));
    world.tick(1);

    let cold = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.1,
        ..Default::default()
    });

    // One more tick so phenotype/genome pipelines settle before we pin warmth.
    world.tick(1);

    // Pin warmth low for the duration of the test so the drift threshold
    // stays crossed — otherwise baseline recovery / ambient drain could
    // wander the deficit across the gate mid-run.
    for _ in 0..200 {
        {
            let mut needs = world.get_mut::<worldsim::agent::body::needs::PhysicalNeeds>(cold);
            needs.warmth = Need::new(0.1);
        }
        world.tick(1);
    }

    let started_walk_toward_fire = world.sim_events().all().iter().any(|ev| {
        matches!(
            ev,
            SimEvent {
                kind: SimEventKind::ActionStarted {
                    agent,
                    action: ActionType::Walk,
                    target: Some(target),
                    ..
                },
                ..
            } if *agent == cold && *target == campfire
        )
    });

    assert!(
        started_walk_toward_fire,
        "cold human with a visible campfire should start a Walk toward it"
    );
}

/// Control: without any heat emitter to drift toward, the warmth path
/// must not fire a targeted Walk. Proves the positive test above is
/// driven by the perceived campfire, not by some incidental Walk the
/// rest of the brain stack would fire regardless.
#[test]
fn cold_human_without_heat_source_does_not_target_walk_anywhere() {
    let mut world = TestWorld::with_seed(42);

    let cold = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.1,
        ..Default::default()
    });

    world.tick(1);

    for _ in 0..200 {
        {
            let mut needs = world.get_mut::<worldsim::agent::body::needs::PhysicalNeeds>(cold);
            needs.warmth = Need::new(0.1);
        }
        world.tick(1);
    }

    let started_targeted_walk = world.sim_events().all().iter().any(|ev| {
        matches!(
            ev,
            SimEvent {
                kind: SimEventKind::ActionStarted {
                    agent,
                    action: ActionType::Walk,
                    target: Some(_),
                    ..
                },
                ..
            } if *agent == cold
        )
    });

    assert!(
        !started_targeted_walk,
        "cold human with no heat source in the world must not fire an entity-targeted Walk"
    );
}
