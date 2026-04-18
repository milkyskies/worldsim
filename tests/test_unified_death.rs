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
//! substrate that combat kills already use.
//!
//! Since #456, death triggers when a vital organ (heart, brain, lungs) hits
//! 0 HP rather than when a flat `health` scalar reaches zero. Starvation
//! cascades through body nodes until the heart fails.

use bevy::math::Vec2;
use bevy::prelude::With;
use worldsim::agent::biology::body::{Body, BodyNodeKind};
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::events::SimEvent;
use worldsim::agent::inventory::EntityType;
use worldsim::agent::mind::knowledge::Concept;
use worldsim::agent::{Alive, Dead};
use worldsim::testing::{AgentConfig, TestWorld};

/// Helper: destroy the heart so the next check_death pass triggers death.
fn destroy_heart(world: &mut TestWorld, agent: bevy::prelude::Entity) {
    world
        .app_mut()
        .world_mut()
        .get_mut::<Body>(agent)
        .expect("agent has a body")
        .node_mut(BodyNodeKind::Heart)
        .expect("body has a heart")
        .current_hp = 0.0;
}

/// A starving agent whose heart is destroyed must morph into a Corpse in
/// place -- preserving its entity ID -- rather than despawning outright.
/// This is the #356 regression test the original fix never actually wrote.
#[test]
fn vital_organ_death_transforms_agent_into_corpse_in_place() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        metabolism: Metabolism::empty(),
        ..Default::default()
    });

    // Damage the heart to near-zero. The starvation cascade (from
    // Metabolism::empty()) will finish it off within a few ticks,
    // then check_death detects the destroyed heart and calls die().
    destroy_heart(&mut world, agent);

    world.tick(60);

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

/// An agent that starves without direct health manipulation -- pools drain,
/// cascade escalates through body nodes, heart fails, corpse appears.
/// Exercises the full metabolism -> starvation cascade -> death chain.
#[test]
fn prolonged_starvation_eventually_spawns_a_corpse() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        metabolism: Metabolism::empty(),
        ..Default::default()
    });

    // Metabolism::empty() means is_starving() is true from tick 1.
    // The starvation cascade runs: limbs -> gut -> liver -> heart -> death.
    world.enable_fast_forward();
    world.tick(20_000);

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

    destroy_heart(&mut world, agent);

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

    destroy_heart(&mut world, agent);
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
/// predicate -- it replaces the old `(With<Agent>, Without<Becomes>)` pattern.
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

    destroy_heart(&mut world, agent);
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
/// post-mortem inspection, but their values must be frozen -- no metabolism
/// ticking, no healing, no starvation damage after death.
#[test]
fn corpse_biology_is_frozen() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        metabolism: Metabolism::empty(),
        ..Default::default()
    });

    destroy_heart(&mut world, agent);
    world.tick(60);

    let entity_type = world.get::<EntityType>(agent);
    assert_eq!(entity_type.0, Concept::Corpse, "agent must be dead");

    // Snapshot the corpse's state.
    let glucose_after_death = world.get::<PhysicalNeeds>(agent).metabolism.glucose;
    let heart_hp_after_death = world
        .get::<Body>(agent)
        .node(BodyNodeKind::Heart)
        .unwrap()
        .current_hp;

    // Tick 200 more times -- biology systems should not touch the corpse.
    world.tick(200);

    let glucose_later = world.get::<PhysicalNeeds>(agent).metabolism.glucose;
    let heart_hp_later = world
        .get::<Body>(agent)
        .node(BodyNodeKind::Heart)
        .unwrap()
        .current_hp;

    assert_eq!(
        glucose_after_death, glucose_later,
        "corpse glucose must not change (was {glucose_after_death}, now {glucose_later})"
    );
    assert_eq!(
        heart_hp_after_death, heart_hp_later,
        "corpse heart HP must not change (was {heart_hp_after_death}, now {heart_hp_later})"
    );
}
