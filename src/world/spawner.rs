//! Unified Spawner: The single source of truth for creating entities in the world.
//! Ensures consistent ECS components + Knowledge Graph assertions.
//!
//! Reads: WorldMap, Ontology, spawn placement helpers
//! Writes: Person, Deer, BerryBush, AppleTree entities (initial population)
//! Upstream: world::map (terrain), spawn_placement (location scoring)
//! Downstream: agent systems consume the resulting entities
//!
//! Individual entity spawning logic is delegated to:
//! - `human.rs` - Person/Agent spawning
//! - `apple_tree.rs` - Apple Tree spawning
//! - `berry_bush.rs` - Berry Bush spawning
//! - `deer.rs` - Deer spawning

use crate::agent::mind::knowledge::Ontology;
use crate::constants::world::{
    APPLE_TREE_SPAWN_COUNT, BERRY_BUSH_SPAWN_COUNT, DEER_HERD_RADIUS_TILES, DEER_HERD_SIZE,
    DEER_MIN_DISTANCE_FROM_SETTLEMENT, DEER_SPAWN_COUNT, HUMAN_CLUSTER_RADIUS_TILES,
    HUMAN_SPAWN_COUNT, MAX_SPAWN_ATTEMPTS, SETTLEMENT_BERRY_BUSH_COUNT,
    SETTLEMENT_FOOD_RADIUS_TILES,
};
use crate::world::map::TileType;
use crate::world::spawn_placement::{
    SettlementSearch, cluster_positions, find_biome_tile, find_settlement_center,
    find_tile_away_from,
};
use bevy::math::UVec2;
use bevy::prelude::*;

// Re-export spawning functions for convenience
pub use super::apple_tree::{
    ResourceRegeneration, VisualApple, VisualLeaves, regenerate_resources, spawn_apple_tree,
    sync_apple_visuals,
};
pub use super::berry_bush::{VisualBerry, VisualBushLeaves, spawn_berry_bush, sync_berry_visuals};
pub use super::deer::{Deer, spawn_deer};
pub use super::human::spawn_person;

pub struct SpawnerPlugin;

impl Plugin for SpawnerPlugin {
    fn build(&self, app: &mut App) {
        // Initialize the shared Ontology resource
        app.insert_resource(crate::agent::mind::knowledge::setup_ontology());

        app.register_type::<ResourceRegeneration>()
            .register_type::<Deer>()
            .add_systems(
                Startup,
                spawn_initial_population.after(crate::world::map::setup_map),
            )
            .add_systems(
                Update,
                (regenerate_resources, sync_apple_visuals, sync_berry_visuals),
            );
    }
}

fn spawn_initial_population(
    mut commands: Commands,
    map: Res<crate::world::map::WorldMap>,
    ontology: Res<Ontology>,
) {
    use rand::Rng;
    use std::collections::HashMap;
    use std::sync::Arc;
    let mut rng = rand::rng();

    // Precompute cultural knowledge
    let mut cultural_knowledge_map = HashMap::new();
    let all_cultures = [
        crate::agent::culture::Culture::Nomad,
        crate::agent::culture::Culture::Farmer,
        crate::agent::culture::Culture::Hunter,
        crate::agent::culture::Culture::Gatherer,
    ];
    for culture in all_cultures {
        let triples = crate::agent::culture::create_cultural_knowledge(culture);
        cultural_knowledge_map.insert(culture, Arc::new(triples));
    }

    // ── Settlement: pick the best grass tile near water and seed the tribe there.
    let settlement = find_settlement_center(&map, &SettlementSearch::default());

    // Plant berry bushes around the settlement *before* humans spawn so the
    // food is already in place when they look around.
    if let Some(center) = settlement {
        let near_settlement = cluster_positions(
            &map,
            center,
            SETTLEMENT_BERRY_BUSH_COUNT,
            SETTLEMENT_FOOD_RADIUS_TILES,
            &mut rng,
        );
        for pos in near_settlement {
            spawn_berry_bush(&mut commands, pos, 4);
        }
    }

    // Cluster humans tightly around the settlement so they can see each other
    // on spawn (vision range = 100 px ≈ 6 tiles).
    let human_positions = match settlement {
        Some(center) => cluster_positions(
            &map,
            center,
            HUMAN_SPAWN_COUNT,
            HUMAN_CLUSTER_RADIUS_TILES,
            &mut rng,
        ),
        None => fallback_random_walkable(&map, &mut rng, HUMAN_SPAWN_COUNT),
    };

    for (i, pos) in human_positions.into_iter().enumerate() {
        let culture = all_cultures[rng.random_range(0..all_cultures.len())];
        let knowledge = cultural_knowledge_map.get(&culture).unwrap().clone();
        spawn_person(&mut commands, ontology.clone(), pos, i, culture, knowledge);
    }

    // Apple trees prefer forest biomes.
    for _ in 0..APPLE_TREE_SPAWN_COUNT {
        if let Some(pos) = find_biome_tile(&map, &mut rng, &[TileType::Forest], MAX_SPAWN_ATTEMPTS)
        {
            spawn_apple_tree(&mut commands, pos, 5);
        }
    }

    // Remaining berry bushes scatter across grass and forest.
    let scattered_bushes = BERRY_BUSH_SPAWN_COUNT.saturating_sub(if settlement.is_some() {
        SETTLEMENT_BERRY_BUSH_COUNT
    } else {
        0
    });
    for _ in 0..scattered_bushes {
        if let Some(pos) = find_biome_tile(
            &map,
            &mut rng,
            &[TileType::Grass, TileType::Forest],
            MAX_SPAWN_ATTEMPTS,
        ) {
            spawn_berry_bush(&mut commands, pos, 4);
        }
    }

    // Deer spawn in small herds in grass/forest, kept clear of the settlement.
    spawn_deer_herds(
        &mut commands,
        &map,
        &ontology,
        settlement,
        DEER_SPAWN_COUNT,
        &mut rng,
    );
}

/// Spawn deer in herds of `DEER_HERD_SIZE`. Each herd anchor is placed in
/// grass/forest biome, kept at least `DEER_MIN_DISTANCE_FROM_SETTLEMENT` tiles
/// from the human settlement (when one exists). Within a herd, members cluster
/// inside `DEER_HERD_RADIUS_TILES` of the anchor.
fn spawn_deer_herds(
    commands: &mut Commands,
    map: &crate::world::map::WorldMap,
    ontology: &Ontology,
    settlement: Option<UVec2>,
    total: usize,
    rng: &mut impl rand::Rng,
) {
    let allowed = [TileType::Grass, TileType::Forest];
    let mut spawned = 0usize;
    let mut attempts = 0usize;

    while spawned < total {
        let remaining = total - spawned;
        let herd_size = remaining.min(DEER_HERD_SIZE);

        let anchor = match settlement {
            Some(center) => find_tile_away_from(
                map,
                rng,
                &allowed,
                center,
                DEER_MIN_DISTANCE_FROM_SETTLEMENT,
                MAX_SPAWN_ATTEMPTS,
            ),
            None => find_biome_tile(map, rng, &allowed, MAX_SPAWN_ATTEMPTS),
        };

        let Some(anchor_pos) = anchor else {
            for _ in 0..remaining {
                if let Some(pos) = find_biome_tile(map, rng, &allowed, MAX_SPAWN_ATTEMPTS) {
                    spawn_deer(commands, ontology.clone(), pos, spawned);
                    spawned += 1;
                }
            }
            return;
        };

        let (anchor_tx, anchor_ty) = map.world_to_tile(anchor_pos);
        let positions = cluster_positions(
            map,
            UVec2::new(anchor_tx, anchor_ty),
            herd_size,
            DEER_HERD_RADIUS_TILES,
            rng,
        );

        if positions.is_empty() {
            spawn_deer(commands, ontology.clone(), anchor_pos, spawned);
            spawned += 1;
        } else {
            for pos in positions {
                spawn_deer(commands, ontology.clone(), pos, spawned);
                spawned += 1;
                if spawned == total {
                    break;
                }
            }
        }

        attempts += 1;
        if attempts > total * 2 {
            // Defensive: never loop forever if the map is pathological.
            break;
        }
    }
}

/// Last-resort placement when no settlement could be found: scatter `count`
/// agents across any walkable tiles. Used only when terrain generation produces
/// a map with no grass-near-water spot at all.
fn fallback_random_walkable(
    map: &crate::world::map::WorldMap,
    rng: &mut impl rand::Rng,
    count: usize,
) -> Vec<Vec2> {
    let mut positions = Vec::with_capacity(count);
    for _ in 0..count {
        for _ in 0..MAX_SPAWN_ATTEMPTS {
            let x = rng.random_range(0..map.width);
            let y = rng.random_range(0..map.height);
            let Some(tile) = map.get_tile(x, y) else {
                continue;
            };
            if tile.is_walkable() && !matches!(tile, TileType::ShallowWater) {
                positions.push(map.tile_to_world(x as i32, y as i32));
                break;
            }
        }
    }
    positions
}
