//! Integration tests for campfire ownership (#231).
//!
//! Verifies that when an agent builds a campfire, the becomes_system writes
//! a `(Self, Owns, campfire_entity)` triple into the builder's MindGraph
//! at the moment the construction site transforms into the finished entity.
//! The triple must reference the new campfire entity, not the despawned site.

use bevy::prelude::*;
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate};
use worldsim::testing::{AgentConfig, TestWorld};
use worldsim::world::campfire::CampfireMarker;
use worldsim::world::construction_site::spawn_construction_site_headless;
use worldsim::world::property::BuiltBy;

/// After Alice places a site that transforms into a campfire, her MindGraph
/// should contain `(Self, Owns, campfire_entity)`.
#[test]
fn building_a_campfire_records_ownership_triple() {
    let mut world = TestWorld::with_seed(0);

    let alice = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));

    // Place a site that's already fully stocked, so it transforms next tick.
    let site = spawn_site_for_builder(
        &mut world,
        Concept::Campfire,
        Vec2::new(120.0, 100.0),
        alice,
    );

    // One tick: becomes_system fires, despawns the site, spawns the campfire,
    // copies BuiltBy forward, writes the ownership triple.
    world.tick(1);

    // The site is gone.
    assert!(
        !world.entity_exists(site),
        "construction site should be despawned after transformation"
    );

    // Alice's MindGraph should contain exactly one Owns triple, pointing at
    // the new campfire entity.
    let mind = world.get::<MindGraph>(alice);
    let owns_triples = mind.query(Some(&Node::Self_), Some(Predicate::Owns), None);
    assert_eq!(
        owns_triples.len(),
        1,
        "Alice should own exactly one entity after building one campfire (got {})",
        owns_triples.len()
    );

    // The owned entity must be a real campfire (CampfireMarker present).
    let owned_entity = owns_triples[0]
        .object
        .as_entity()
        .expect("Owns triple's object must be an Entity");
    assert!(
        world
            .app()
            .world()
            .get::<CampfireMarker>(owned_entity)
            .is_some(),
        "Owned entity should be a campfire (have CampfireMarker)"
    );
}

/// The entity ID stored in the Owns triple must be the finished campfire,
/// NOT the construction site (which has been despawned).
#[test]
fn ownership_triple_references_finished_entity_not_site() {
    let mut world = TestWorld::with_seed(0);

    let alice = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));
    let site = spawn_site_for_builder(
        &mut world,
        Concept::Campfire,
        Vec2::new(120.0, 100.0),
        alice,
    );

    world.tick(1);

    let mind = world.get::<MindGraph>(alice);
    let owns_triples = mind.query(Some(&Node::Self_), Some(Predicate::Owns), None);
    let owned_entity = owns_triples[0].object.as_entity().unwrap();

    assert_ne!(
        owned_entity, site,
        "Owns triple must reference the new campfire entity, not the despawned site"
    );
    assert!(
        world.entity_exists(owned_entity),
        "the owned entity must still exist (it's the new campfire)"
    );
}

/// Each agent should own only the campfire they built — not their neighbour's.
#[test]
fn multiple_agents_each_own_their_own_campfire() {
    let (mut world, entities) = TestWorld::scenario(0)
        .agent("alice")
        .pos(Vec2::new(100.0, 100.0))
        .done()
        .agent("bob")
        .pos(Vec2::new(300.0, 300.0))
        .done()
        .build();
    let alice = entities.get("alice");
    let bob = entities.get("bob");

    let _alice_site = spawn_site_for_builder(
        &mut world,
        Concept::Campfire,
        Vec2::new(110.0, 100.0),
        alice,
    );
    let _bob_site =
        spawn_site_for_builder(&mut world, Concept::Campfire, Vec2::new(310.0, 300.0), bob);

    world.tick(1);

    // Alice owns one campfire — but it isn't Bob's.
    let alice_owns = {
        let mind = world.get::<MindGraph>(alice);
        mind.query(Some(&Node::Self_), Some(Predicate::Owns), None)
            .into_iter()
            .map(|t| t.object.as_entity().unwrap())
            .collect::<Vec<_>>()
    };
    let bob_owns = {
        let mind = world.get::<MindGraph>(bob);
        mind.query(Some(&Node::Self_), Some(Predicate::Owns), None)
            .into_iter()
            .map(|t| t.object.as_entity().unwrap())
            .collect::<Vec<_>>()
    };

    assert_eq!(alice_owns.len(), 1, "Alice should own exactly one campfire");
    assert_eq!(bob_owns.len(), 1, "Bob should own exactly one campfire");
    assert_ne!(
        alice_owns[0], bob_owns[0],
        "Alice and Bob should own different campfires"
    );

    // Each campfire's BuiltBy should point at its actual builder.
    let alice_built_by = world
        .app()
        .world()
        .get::<BuiltBy>(alice_owns[0])
        .expect("Alice's campfire should have BuiltBy");
    assert_eq!(alice_built_by.builder, alice);

    let bob_built_by = world
        .app()
        .world()
        .get::<BuiltBy>(bob_owns[0])
        .expect("Bob's campfire should have BuiltBy");
    assert_eq!(bob_built_by.builder, bob);
}

/// Only the agent who PLACED the site owns the result. Other agents who
/// merely deposit materials into someone else's site do not become owners.
#[test]
fn building_for_a_friend_does_not_create_my_ownership() {
    let (mut world, entities) = TestWorld::scenario(0)
        .agent("alice")
        .pos(Vec2::new(100.0, 100.0))
        .done()
        .agent("bob")
        .pos(Vec2::new(110.0, 100.0))
        .done()
        .build();
    let alice = entities.get("alice");
    let bob = entities.get("bob");

    // Alice places a site (so she'll be the BuiltBy.builder). Bob is nearby
    // but doesn't place anything — depositors do not get a BuiltBy.
    let _site = spawn_site_for_builder(
        &mut world,
        Concept::Campfire,
        Vec2::new(120.0, 100.0),
        alice,
    );

    world.tick(1);

    // Alice owns the campfire. Bob owns nothing.
    let alice_owns_count = {
        let mind = world.get::<MindGraph>(alice);
        mind.query(Some(&Node::Self_), Some(Predicate::Owns), None)
            .len()
    };
    let bob_owns_count = {
        let mind = world.get::<MindGraph>(bob);
        mind.query(Some(&Node::Self_), Some(Predicate::Owns), None)
            .len()
    };

    assert_eq!(
        alice_owns_count, 1,
        "Alice (the placer) should own the campfire"
    );
    assert_eq!(
        bob_owns_count, 0,
        "Bob (who only stood nearby) should not own anything"
    );
}

// ─── Test helpers ────────────────────────────────────────────────────────────

/// Spawn a fully-stocked construction site for the given builder. Used to
/// short-circuit Build's GOAP path and test the ownership write directly.
fn spawn_site_for_builder(
    world: &mut TestWorld,
    target: Concept,
    position: Vec2,
    builder: Entity,
) -> Entity {
    let mut commands_queue = bevy::ecs::world::CommandQueue::default();
    let mut commands = Commands::new(&mut commands_queue, world.app().world());
    let id = spawn_construction_site_headless(
        &mut commands,
        target,
        position,
        &[(Concept::Wood, 3)],
        &[(Concept::Wood, 3)], // fully stocked → transforms next tick
        None,                  // no labor required
        0,
        Some(builder),
    );
    commands_queue.apply(world.app_mut().world_mut());
    id
}
