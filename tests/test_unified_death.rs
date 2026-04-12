//! Regression tests for the unified death path introduced in #350.
//!
//! Before #350, `check_death` in `biology/body.rs` called
//! `commands.entity(entity).despawn()` directly when HP hit zero, so
//! starvation deaths vanished without a trace (no Corpse, no harvestable
//! meat, no entity-id preservation for memory graphs). #356 was opened to
//! fix this but the fix never landed.
//!
//! #350 replaces that path with a unified `die()` helper that inserts a
//! `Becomes InPlace Corpse` component, so every cause of death (starvation,
//! combat, future disease / drowning / old age) routes through the same
//! substrate that combat kills already use. These tests exercise the
//! starvation → corpse path end-to-end.

use bevy::math::Vec2;
use bevy::prelude::With;
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::events::SimEvent;
use worldsim::agent::inventory::EntityType;
use worldsim::agent::mind::knowledge::Concept;
use worldsim::agent::{Alive, Dead};
use worldsim::testing::{AgentConfig, TestWorld};

/// A starving agent whose health reaches zero must morph into a Corpse in
/// place — preserving its entity ID — rather than despawning outright.
/// This is the #356 regression test the original fix never actually wrote.
#[test]
fn starvation_death_transforms_agent_into_corpse_in_place() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        metabolism: Metabolism::empty(),
        ..Default::default()
    });

    // Stage the agent just above the death threshold. With TestWorld's
    // `ticks_per_second = 60` (`dt ≈ 0.0167s`) and `STARVATION_DAMAGE = 0.3/s`,
    // a 0.1-HP cushion drains in about 20 ticks of `process_starvation`,
    // at which point `check_death` -> `die()` -> `Becomes InPlace Corpse`
    // -> `becomes_system` -> `kill_into_corpse` fires and the agent morphs.
    world
        .app_mut()
        .world_mut()
        .get_mut::<PhysicalNeeds>(agent)
        .expect("agent has physical needs")
        .health = 0.1;

    world.tick(60);

    // Entity ID survives the in-place transformation — this is the point
    // of the fix. The legacy despawn path would have deleted the entity
    // outright, leaving any lingering memory references dangling.
    assert!(
        world.entity_exists(agent),
        "entity ID must survive the in-place corpse transformation"
    );
    let entity_type = world.get::<EntityType>(agent);
    assert_eq!(
        entity_type.0,
        Concept::Corpse,
        "dead agent should now be classified as a Corpse (got {:?})",
        entity_type.0
    );
}

/// An agent that starves without direct health manipulation — pools drain,
/// gradient escalates, HP damage accumulates, death fires, corpse appears.
/// Exercises the full metabolism -> starvation -> death chain.
#[test]
fn prolonged_starvation_eventually_spawns_a_corpse() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        metabolism: Metabolism::empty(),
        ..Default::default()
    });

    // Start with low but non-critical health. Starvation damage ticks
    // while the metabolism stays empty (is_starving() is true from the
    // start because Metabolism::empty() has zero glucose + zero reserves).
    world
        .app_mut()
        .world_mut()
        .get_mut::<PhysicalNeeds>(agent)
        .expect("agent has physical needs")
        .health = 5.0;

    // 2000 ticks is enough for STARVATION_DAMAGE_PER_SEC (0.3/s) to
    // accumulate beyond 5.0 HP even at the generous tick dt the simulation
    // uses, then trigger check_death, Becomes, and the in-place morph.
    world.tick(2000);

    assert!(
        world.entity_exists(agent),
        "entity ID must survive the in-place corpse transformation"
    );
    let entity_type = world.get::<EntityType>(agent);
    assert_eq!(
        entity_type.0,
        Concept::Corpse,
        "starved agent should end the test as a Corpse (got {:?})",
        entity_type.0
    );
}

/// Regression test for #402: a corpse must not re-emit death SimEvents after
/// the initial transformation. `check_death` queries `With<Alive>`, so once
/// `die()` removes `Alive` and inserts `Dead`, the corpse is invisible to
/// `check_death` regardless of what happens to `Becomes` or `Agent`.
#[test]
fn corpse_emits_death_event_exactly_once() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        metabolism: Metabolism::empty(),
        ..Default::default()
    });

    world
        .app_mut()
        .world_mut()
        .get_mut::<PhysicalNeeds>(agent)
        .expect("agent has physical needs")
        .health = 0.1;

    // Tick well past death so the corpse has many opportunities to re-fire.
    world.tick(120);

    let death_count = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| matches!(e, SimEvent::Death { agent: a, .. } if *a == agent))
        .count();

    assert_eq!(
        death_count, 1,
        "death SimEvent must fire exactly once per agent, got {death_count}"
    );
}

/// Living agents have `Alive` but not `Dead`. After death, `Alive` is removed
/// and `Dead` is inserted. The corpse retains `Dead` permanently.
#[test]
fn alive_marker_removed_and_dead_inserted_on_death() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        metabolism: Metabolism::empty(),
        ..Default::default()
    });

    assert!(
        world.app().world().get::<Alive>(agent).is_some(),
        "living agent must have Alive marker"
    );
    assert!(
        world.app().world().get::<Dead>(agent).is_none(),
        "living agent must not have Dead marker"
    );

    world
        .app_mut()
        .world_mut()
        .get_mut::<PhysicalNeeds>(agent)
        .expect("agent has physical needs")
        .health = 0.1;

    world.tick(60);

    assert!(
        world.app().world().get::<Alive>(agent).is_none(),
        "corpse must not have Alive marker"
    );
    assert!(
        world.app().world().get::<Dead>(agent).is_some(),
        "corpse must have Dead marker"
    );
}

/// `With<Alive>` queries must skip corpses. This is the primary liveness
/// predicate — it replaces the old `(With<Agent>, Without<Becomes>)` pattern.
#[test]
fn alive_query_skips_corpses() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        metabolism: Metabolism::empty(),
        ..Default::default()
    });

    let alive_before = {
        let mut q = world
            .app_mut()
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, With<Alive>>();
        q.iter(world.app().world()).count()
    };
    assert!(alive_before >= 1, "at least one living agent");

    world
        .app_mut()
        .world_mut()
        .get_mut::<PhysicalNeeds>(agent)
        .expect("agent has physical needs")
        .health = 0.1;

    world.tick(60);

    let alive_after = {
        let mut q = world
            .app_mut()
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, With<Alive>>();
        q.iter(world.app().world()).count()
    };
    assert_eq!(
        alive_after,
        alive_before - 1,
        "one agent died, alive count should drop by 1"
    );
}

/// Biology components (PhysicalNeeds, Body) must remain on the corpse for
/// post-mortem inspection, but their values must be frozen — no metabolism
/// ticking, no healing, no starvation damage after death.
#[test]
fn corpse_biology_is_frozen() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        metabolism: Metabolism::empty(),
        ..Default::default()
    });

    world
        .app_mut()
        .world_mut()
        .get_mut::<PhysicalNeeds>(agent)
        .expect("agent has physical needs")
        .health = 0.1;

    // Let the agent die.
    world.tick(60);

    let entity_type = world.get::<EntityType>(agent);
    assert_eq!(entity_type.0, Concept::Corpse, "agent must be dead");

    // Snapshot the corpse's physical needs.
    let health_after_death = world.get::<PhysicalNeeds>(agent).health;
    let glucose_after_death = world.get::<PhysicalNeeds>(agent).metabolism.glucose;

    // Tick 200 more times — biology systems should not touch the corpse.
    world.tick(200);

    let health_later = world.get::<PhysicalNeeds>(agent).health;
    let glucose_later = world.get::<PhysicalNeeds>(agent).metabolism.glucose;

    assert_eq!(
        health_after_death, health_later,
        "corpse health must not change (was {health_after_death}, now {health_later})"
    );
    assert_eq!(
        glucose_after_death, glucose_later,
        "corpse glucose must not change (was {glucose_after_death}, now {glucose_later})"
    );
}
