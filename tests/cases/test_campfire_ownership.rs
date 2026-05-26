//! Integration tests for campfire ownership (#231).
//!
//! Verifies that when an agent builds a campfire, the becomes_system writes
//! a `(Self, Owns, campfire_entity)` triple into the builder's MindGraph
//! at the moment the construction site transforms into the finished entity.
//! The triple must reference the new campfire entity, not the despawned site.

use bevy::prelude::*;
use worldsim::agent::mind::knowledge::Concept;
use worldsim::testing::{AgentConfig, TestWorld};
use worldsim::world::campfire::CampfireMarker;
use worldsim::world::construction_site::spawn_construction_site_headless;
use worldsim::world::property::BuiltBy;

/// Returns every entity whose `BuiltBy` records the given builder. Replaces
/// the old `mind.query(Self, Owns, ?)` lookup after the Owns triple was
/// deleted in #587 — `BuiltBy` was always the canonical record; the triple
/// was redundant duplication.
fn entities_built_by(world: &mut TestWorld, builder: Entity) -> Vec<Entity> {
    let mut q = world.app_mut().world_mut().query::<(Entity, &BuiltBy)>();
    q.iter(world.app().world())
        .filter_map(|(entity, built_by)| (built_by.builder == builder).then_some(entity))
        .collect()
}

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
    // and tags it with BuiltBy.
    world.tick(1);

    // The site is gone.
    assert!(
        !world.entity_exists(site),
        "construction site should be despawned after transformation"
    );

    // Alice should be the builder of exactly one entity — the new campfire.
    let owned = entities_built_by(&mut world, alice);
    assert_eq!(
        owned.len(),
        1,
        "Alice should be the builder of exactly one entity after building one campfire (got {})",
        owned.len()
    );

    // That entity must be a real campfire (CampfireMarker present).
    assert!(
        world
            .app()
            .world()
            .get::<CampfireMarker>(owned[0])
            .is_some(),
        "BuiltBy entity should be a campfire (have CampfireMarker)"
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

    let owned = entities_built_by(&mut world, alice);
    let owned_entity = owned[0];

    assert_ne!(
        owned_entity, site,
        "BuiltBy must tag the new campfire entity, not the despawned site"
    );
    assert!(
        world.entity_exists(owned_entity),
        "the built entity must still exist (it's the new campfire)"
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

    // Alice built one campfire — but it isn't Bob's.
    let alice_built = entities_built_by(&mut world, alice);
    let bob_built = entities_built_by(&mut world, bob);

    assert_eq!(
        alice_built.len(),
        1,
        "Alice should build exactly one campfire"
    );
    assert_eq!(bob_built.len(), 1, "Bob should build exactly one campfire");
    assert_ne!(
        alice_built[0], bob_built[0],
        "Alice and Bob should build different campfires"
    );
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

    // Alice built the campfire. Bob built nothing.
    let alice_count = entities_built_by(&mut world, alice).len();
    let bob_count = entities_built_by(&mut world, bob).len();

    assert_eq!(
        alice_count, 1,
        "Alice (the placer) should be the builder of the campfire"
    );
    assert_eq!(
        bob_count, 0,
        "Bob (who only stood nearby) should not be the builder of anything"
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
