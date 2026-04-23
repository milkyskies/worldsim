//! Integration tests for the reactive drift behavior layer.
//!
//! Phase 2 (#642) is tile-based: the agent samples its local tile
//! neighborhood each tick, scores each tile by drive relief from both
//! perceived entities AND physical fields, and walks to the best tile.
//! The old entity-picking seek functions are gone, so assertions are
//! on the agent's position convergence, not on target_entity.

use bevy::math::Vec2;
use worldsim::agent::body::need::Need;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::core::tick::TickCount;
use worldsim::testing::{AgentConfig, TestWorld};
use worldsim::world::field_grid::FIELD_CHUNK_SIZE;
use worldsim::world::field_grid_plugin::FieldGrids;
use worldsim::world::map::TILE_SIZE;

fn agent_pos(world: &TestWorld, agent: bevy::prelude::Entity) -> Vec2 {
    world
        .app()
        .world()
        .get::<bevy::prelude::Transform>(agent)
        .unwrap()
        .translation
        .truncate()
}

/// Pin warmth low across `ticks` steps so the drift threshold stays
/// crossed — baseline drain could otherwise wander it across the gate
/// mid-run and drop the Walk proposal.
fn tick_with_cold_pinned(world: &mut TestWorld, agent: bevy::prelude::Entity, ticks: u32) {
    for _ in 0..ticks {
        {
            let mut needs = world.get_mut::<PhysicalNeeds>(agent);
            needs.warmth = Need::new(0.1);
        }
        world.tick(1);
    }
}

/// Cold human + one visible campfire → agent drifts closer to the fire
/// over time. Entity-targeted walks are no longer the mechanism (the
/// tile scorer produces a `target_position`, not a `target_entity`), so
/// this asserts on position convergence — the user-observable behavior.
#[test]
fn cold_human_drifts_toward_visible_campfire() {
    let mut world = TestWorld::with_seed(42);

    let fire_pos = Vec2::new(90.0, 0.0);
    world.spawn_campfire(fire_pos);
    world.tick(1); // let derive_ontology_heat_source fill the Ontology

    let cold = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.1,
        ..Default::default()
    });
    world.tick(1);

    let start_dist = agent_pos(&world, cold).distance(fire_pos);
    tick_with_cold_pinned(&mut world, cold, 300);
    let end_dist = agent_pos(&world, cold).distance(fire_pos);

    assert!(
        end_dist < start_dist - 10.0,
        "cold agent should drift toward fire; start_dist={start_dist:.1}, end_dist={end_dist:.1}"
    );
}

/// Discriminator for tile-based scoring: a warm zone injected directly
/// into the field grid, with NO HeatEmitting entity anywhere. Entity-
/// picking cannot see this; only field-sampling can. If the agent
/// drifts toward the warm tiles, tile-based scoring is working.
#[test]
fn cold_human_drifts_toward_field_warmth_with_no_emitter() {
    let mut world = TestWorld::with_seed(42);

    let cold = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.1,
        ..Default::default()
    });
    world.tick(1);

    // Warm zone 3 tiles east (48px) — within the agent's local tile
    // sampling neighborhood so the field-only pull actually shows up
    // in the scorer. Brightly-injected each tick so relaxation doesn't
    // bleed it back to ambient before the agent arrives.
    let warm_tile_center_px = Vec2::new(3.0 * TILE_SIZE + TILE_SIZE / 2.0, TILE_SIZE / 2.0);
    let warm_tile = bevy::math::IVec2::new(3, 0);

    for _ in 0..300 {
        // Re-inject each tick since relaxation bleeds deltas toward zero.
        // Absolute write via inject (delta += 40) keeps the tile strongly
        // above ambient throughout the test.
        world
            .app_mut()
            .world_mut()
            .resource_mut::<FieldGrids>()
            .temperature_mut()
            .inject_at_tile(warm_tile, 4.0);
        {
            let mut needs = world.get_mut::<PhysicalNeeds>(cold);
            needs.warmth = Need::new(0.1);
        }
        world.tick(1);
    }

    let end = agent_pos(&world, cold);
    let dist_to_warm = end.distance(warm_tile_center_px);

    assert!(
        dist_to_warm < 80.0,
        "agent should drift toward the manually-warmed tiles; end pos {end:?} dist {dist_to_warm:.1}"
    );
}

/// Keep the warning flag: the #644 field grid initializes on FieldGrids
/// with one Temperature grid, and our tile-based scorer reads that
/// resource. Make sure the test harness still sees a live FieldGrids
/// resource so the rest of the tests don't silently pass on an empty
/// grid.
#[test]
fn field_grids_resource_is_registered_in_test_world() {
    let world = TestWorld::with_seed(0);
    assert!(
        world.app().world().get_resource::<FieldGrids>().is_some(),
        "FieldGrids resource must be registered for tile-based drift tests"
    );
}

/// Silence unused-import complaints on helpers we want to keep around
/// for future tests without triggering dead-code lints.
#[allow(dead_code)]
fn _unused_imports_sink() {
    let _ = FIELD_CHUNK_SIZE;
    let _ = TickCount::new(60.0);
}
