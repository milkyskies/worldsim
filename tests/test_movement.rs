//! Regression tests for agent movement at tile boundaries.
//!
//! Bug: when a Walk target is within ARRIVAL_THRESHOLD (2.0 px) of a tile
//! boundary, the agent's arrival position ends up in the wrong tile.
//! is_step_complete checks Self_ LocatedAt <target_tile> which never matches,
//! so the plan step never advances and the agent is stuck forever.
//!
//! Fix: snap position to the exact target when arriving within threshold.

use bevy::prelude::*;
use worldsim::agent::actions::{ActionState, ActionType, ActiveActions};
use worldsim::testing::{AgentConfig, TestWorld};
use worldsim::world::map::TILE_SIZE;

/// Regression: Walk action arriving within ARRIVAL_THRESHOLD of a tile boundary
/// must snap position to the exact target so the perceived tile matches the Walk
/// effect's tile, allowing is_step_complete to return true.
///
/// Setup: TILE_SIZE=16, ARRIVAL_THRESHOLD=2, BASE_SPEED=0.8 px/tick.
/// Agent at (14.0, 50.0), target at (16.5, 50.0).
/// After 1 tick: movement puts agent at ~(14.8, 50.0) — within threshold, but
/// tile floor(14.8/16)=0 while Walk effect needs tile floor(16.5/16)=1.
/// Without fix: position not snapped → tile mismatch → plan never advances.
/// With fix: position snapped to (16.5, 50.0) → tile matches → plan advances.
#[test]
fn walk_snaps_to_target_when_arriving_near_tile_boundary() {
    let start = Vec2::new(14.0, 50.0);
    let target = Vec2::new(16.5, 50.0); // 0.5 px past the x=16.0 tile boundary

    // Sanity check: start and target are in DIFFERENT tiles
    let start_tile_x = (start.x / TILE_SIZE).floor() as i32;
    let target_tile_x = (target.x / TILE_SIZE).floor() as i32;
    assert_ne!(
        start_tile_x, target_tile_x,
        "test setup: start and target must be in different tiles"
    );

    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::at(start));

    // Advance past tick 0 before injecting the Walk action. At tick 0,
    // entity_id 0 satisfies (0 + 0) % 60 == 0, so the brain fires and
    // proposes Explore, which would preempt the injected Walk via start_actions.
    // After tick 1, the next brain fire is at tick 60 — well outside our window.
    world.tick(1);

    // Inject the Walk action directly into ActiveActions so we don't need to wait
    // for the 60-tick thinking interval before the brain proposes it.
    {
        let mut active = world.get_mut::<ActiveActions>(agent);
        active.insert(ActionState {
            action_type: ActionType::Walk,
            target_position: Some(target),
            ..Default::default()
        });
    }

    world.tick(5);

    let pos = world.get::<Transform>(agent).translation.truncate();
    assert!(
        pos.distance(target) < 0.01,
        "Walk must snap position to exact target on arrival. \
         Got {pos:?}, expected {target:?}. \
         If not snapped, perceived tile ({},{}) != target tile ({},{}) \
         and is_step_complete stays false forever.",
        (pos.x / TILE_SIZE).floor() as i32,
        (pos.y / TILE_SIZE).floor() as i32,
        target_tile_x,
        (target.y / TILE_SIZE).floor() as i32,
    );
}
