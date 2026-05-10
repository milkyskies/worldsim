//! Integration tests for the Graze action (#259).
//!
//! Covers the end-to-end grazing loop: a hungry deer standing on grass must
//! find a grazable tile, choose `Graze`, drift while nibbling, and reduce
//! its hunger without ever needing to carry food in inventory.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Predicate, Value};
use worldsim::testing::TestWorld;

/// Per #739, terrain traits live in the world (terrain type + ontology),
/// not in each agent's MindGraph. A grazing deer must never have
/// `(Tile(_), HasTrait, Grazable)` triples written into its belief store
/// — that was the per-agent duplication the refactor removed.
#[test]
fn grazing_deer_has_no_per_tile_grazable_beliefs() {
    let mut world = TestWorld::with_seed(42);
    let deer = world.spawn_deer(Vec2::new(200.0, 200.0));

    // Run long enough for the deer to perceive its surroundings, plan,
    // and execute multiple graze drifts.
    world.tick(400);

    let mind = world.get::<MindGraph>(deer);
    let grazable = mind.query(
        None,
        Some(Predicate::HasTrait),
        Some(&Value::Concept(Concept::Grazable)),
    );
    assert!(
        grazable.is_empty(),
        "Grazable is objective world state — agents must not store per-tile beliefs about it. Got {} triples",
        grazable.len()
    );
}

/// Two deer in overlapping perception of the same grass area must both be
/// able to plan against grazable tiles without either of them writing
/// per-tile MindGraph beliefs. This is the core duplication-eliminating
/// claim of #739: N agents perceiving the same tile produces 0 redundant
/// triples, not N.
#[test]
fn two_deer_both_graze_without_per_tile_beliefs() {
    let mut world = TestWorld::with_seed(42);
    let deer_a = world.spawn_deer(Vec2::new(200.0, 200.0));
    let deer_b = world.spawn_deer(Vec2::new(220.0, 200.0));

    {
        let mut needs = world.get_mut::<PhysicalNeeds>(deer_a);
        needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.8);
    }
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(deer_b);
        needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.8);
    }

    world.tick(400);

    for deer in [deer_a, deer_b] {
        let mind = world.get::<MindGraph>(deer);
        let grazable = mind.query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Grazable)),
        );
        assert!(
            grazable.is_empty(),
            "deer {deer:?} should have no Grazable tile beliefs, got {}",
            grazable.len()
        );
    }

    let started_graze = world.sim_events().all().iter().any(|ev| {
        matches!(
            ev,
            worldsim::agent::events::SimEvent {
                kind: worldsim::agent::events::SimEventKind::ActionStarted {
                    action: ActionType::Graze,
                    ..
                },
                ..
            }
        )
    });
    assert!(
        started_graze,
        "at least one of the deer should have started grazing"
    );
}

/// A hungry deer standing on a flat grass field should choose Graze over
/// anything else and reduce its hunger through the continuous `runtime_effects`
/// hunger drain. No berries in inventory, no bushes nearby — grass must be a
/// sufficient food source on its own.
#[test]
fn hungry_deer_on_grass_grazes_and_reduces_hunger() {
    let mut world = TestWorld::with_seed(42);
    let deer = world.spawn_deer(Vec2::new(200.0, 200.0));

    // Make the deer hungry enough that hunger urgency dominates.
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(deer);
        needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.8);
    }
    let start_hunger = world.agent_hunger(deer);

    // Enough ticks for: grass perception (30), brain thinking (~60),
    // several full graze drifts to elapse.
    world.tick(400);

    let end_hunger = world.agent_hunger(deer);
    assert!(
        end_hunger < start_hunger,
        "deer hunger should drop via grazing (start={start_hunger:.1}, end={end_hunger:.1})"
    );
}

/// Assert the agent actually entered `Graze` at some point, not just that
/// hunger drifted down from some other effect. Without this the previous test
/// could pass on any hunger-reducing action in the registry.
#[test]
fn hungry_deer_on_grass_enters_graze_action() {
    use worldsim::agent::events::{SimEvent, SimEventKind};

    let mut world = TestWorld::with_seed(42);
    let deer = world.spawn_deer(Vec2::new(200.0, 200.0));

    {
        let mut needs = world.get_mut::<PhysicalNeeds>(deer);
        needs.metabolism = worldsim::agent::body::metabolism::Metabolism::at_urgency(0.8);
    }

    world.tick(400);

    let started_graze = world.sim_events().all().iter().any(|ev| {
        matches!(
            ev,
            SimEvent { kind: SimEventKind::ActionStarted { agent, action: ActionType::Graze, .. }, .. } if *agent == deer
        )
    });

    assert!(
        started_graze,
        "deer should have started a Graze action at least once"
    );
}
