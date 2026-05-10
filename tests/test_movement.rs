//! Regression tests for agent movement at tile boundaries.
//!
//! Bug: when a Walk target is within ARRIVAL_THRESHOLD (2.0 px) of a tile
//! boundary, the agent's arrival position ends up in the wrong tile.
//! is_step_complete checks Self_ LocatedAt <target_tile> which never matches,
//! so the plan step never advances and the agent is stuck forever.
//!
//! Fix: snap position to the exact target when arriving within threshold.

use bevy::prelude::*;
use worldsim::agent::TargetPosition;
use worldsim::agent::actions::{ActionState, ActionType, ActiveActions};
use worldsim::agent::brains::proposal::BrainState;
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

    // Daze the agent so the brain doesn't repropose Explore during the
    // 5-tick walk — the test verifies snap-to-tile logic in the movement
    // system, not brain selection.
    world
        .app_mut()
        .world_mut()
        .entity_mut(agent)
        .insert(worldsim::agent::Dazed {
            until_tick: u64::MAX,
        });

    // Tick once so any state attached at spawn settles, then clear all
    // actions and chosen_actions before injecting Walk.
    world.tick(1);
    {
        let w = world.app_mut().world_mut();
        w.get_mut::<ActiveActions>(agent).unwrap().clear();
        w.get_mut::<BrainState>(agent)
            .unwrap()
            .chosen_actions
            .clear();
        // Inject Walk with explicit target and update TargetPosition directly so
        // the movement system tracks the right destination this tick.
        w.get_mut::<ActiveActions>(agent)
            .unwrap()
            .insert(ActionState {
                action_type: ActionType::Walk,
                target_position: Some(target),
                ..Default::default()
            });
        w.get_mut::<TargetPosition>(agent).unwrap().0 = Some(target);
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
