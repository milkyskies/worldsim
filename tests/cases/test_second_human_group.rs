//! Integration tests for the second human group (#58).
//!
//! Verifies that `game_defaults` spawns two human clusters on opposite sides
//! of the river so agents start as strangers separated by a natural barrier.

use bevy::prelude::*;
use worldsim::agent::Person;
use worldsim::testing::TestWorld;
use worldsim::world::map::{DEFAULT_TERRAIN_SEED, TILE_SIZE, WORLD_WIDTH, river_center_x};

/// A second human cluster should exist on the opposite side of the river
/// from the first cluster when using game defaults.
#[test]
fn game_defaults_spawns_two_clusters_on_opposite_sides_of_river() {
    let mut world = TestWorld::game_defaults(42);
    let humans: Vec<Entity> = world
        .all_agents()
        .into_iter()
        .filter(|&e| world.app().world().get::<Person>(e).is_some())
        .collect();
    assert!(
        humans.len() >= 2,
        "game_defaults should spawn multiple humans, got {}",
        humans.len()
    );

    // Partition humans by which side of the river they stand on.
    let mut left_of_river = 0;
    let mut right_of_river = 0;
    for &entity in &humans {
        let transform = world.get::<Transform>(entity);
        let tile_x = (transform.translation.x / TILE_SIZE) as u32;
        let tile_y = (transform.translation.y / TILE_SIZE).max(0.0) as u32;
        let river_cx = river_center_x(tile_y, WORLD_WIDTH, DEFAULT_TERRAIN_SEED);
        if tile_x < river_cx {
            left_of_river += 1;
        } else {
            right_of_river += 1;
        }
    }

    assert!(
        left_of_river > 0 && right_of_river > 0,
        "expected humans on both sides of the river, got L={left_of_river} R={right_of_river}"
    );
}
