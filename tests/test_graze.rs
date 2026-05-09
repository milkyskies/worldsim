//! Integration tests for the Graze action (#259).
//!
//! Covers the end-to-end grazing loop: a hungry deer standing on grass must
//! perceive the tile as `Grazable`, choose `Graze`, drift while nibbling, and
//! reduce its hunger without ever needing to carry food in inventory.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use worldsim::testing::TestWorld;

/// Herbivore perception asserts `Tile(?) HasTrait Grazable` for visible grass
/// tiles, mirroring the water → drinkable pathway. Without this, the rational
/// brain has nothing for `TargetSource::TileWithTrait(Grazable)` to enumerate.
#[test]
fn deer_perceives_grass_tiles_as_grazable() {
    let mut world = TestWorld::with_seed(42);
    let deer = world.spawn_deer(Vec2::new(200.0, 200.0));

    // Grass perception runs every 30 ticks per agent; tick well past that.
    world.tick(60);

    let mind = world.get::<MindGraph>(deer);
    let grazable = mind.query(
        None,
        Some(Predicate::HasTrait),
        Some(&Value::Concept(Concept::Grazable)),
    );

    assert!(
        !grazable.is_empty(),
        "deer on a flat grass map should perceive at least one Grazable tile"
    );
    // And they should be Tile nodes, not entity or concept nodes.
    assert!(
        grazable.iter().all(|t| matches!(t.subject, Node::Tile(_))),
        "Grazable must be asserted against tile nodes, got {:?}",
        grazable.iter().map(|t| &t.subject).collect::<Vec<_>>()
    );
}

/// Humans are omnivores — they must not perceive grass as grazable. Otherwise
/// the rational planner would consider Graze as a food candidate for humans
/// and pollute their MindGraph with useless tile-trait triples.
#[test]
fn humans_do_not_perceive_grass_as_grazable() {
    let mut world = TestWorld::with_seed(42);
    let human = world.spawn_agent(worldsim::testing::AgentConfig {
        pos: Vec2::new(200.0, 200.0),
        ..Default::default()
    });

    world.tick(60);

    let mind = world.get::<MindGraph>(human);
    let grazable = mind.query(
        None,
        Some(Predicate::HasTrait),
        Some(&Value::Concept(Concept::Grazable)),
    );

    assert!(
        grazable.is_empty(),
        "non-herbivores should never see Grazable triples; got {} entries",
        grazable.len()
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
