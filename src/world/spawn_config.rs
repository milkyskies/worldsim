//! WorldSpawnConfig: unified configuration for world entity placement.
//!
//! Reads: WorldMap (terrain), constants::world (default counts)
//! Writes: SpawnLayout (positions + initial resource amounts for all entity types)
//! Upstream: world::map (terrain data), world::spawn_placement (placement algorithms)
//! Downstream: world::spawner (windowed game), headless (CLI), testing::world (TestWorld)

use bevy::math::{UVec2, Vec2};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::constants::world::{
    APPLE_TREE_SPAWN_COUNT, BERRY_BUSH_SPAWN_COUNT, DEER_HERD_RADIUS_TILES, DEER_HERD_SIZE,
    DEER_MIN_DISTANCE_FROM_SETTLEMENT, DEER_SPAWN_COUNT, HUMAN_CLUSTER_RADIUS_TILES,
    HUMAN_SPAWN_COUNT, MAX_SPAWN_ATTEMPTS, SETTLEMENT_BERRY_BUSH_COUNT,
    SETTLEMENT_FOOD_RADIUS_TILES, STONE_NODE_SPAWN_COUNT, WOOD_LOG_SPAWN_COUNT,
};
use crate::world::map::{TileType, WORLD_HEIGHT, WORLD_WIDTH, WorldMap};
use crate::world::spawn_placement::{
    SettlementSearch, cluster_positions, find_biome_tile, find_settlement_center,
    find_tile_away_from,
};

/// Area in pixels for the Uniform scatter algorithm.
const UNIFORM_AREA_PX: f32 = 1024.0;

/// Which algorithm to use when placing entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnAlgorithm {
    /// Settlement detection, biome clustering, herd grouping — same as the normal game.
    Realistic,
    /// Uniform random scatter within a fixed pixel area (fast, simple).
    Uniform,
}

/// Configuration for world entity placement. Controls what is spawned and how.
#[derive(Debug, Clone)]
pub struct WorldSpawnConfig {
    pub map_size: (u32, u32),
    pub humans: usize,
    pub deer: usize,
    pub berry_bushes: usize,
    pub apple_trees: usize,
    pub stone_nodes: usize,
    pub wood_logs: usize,
    /// Seed for the spawn-position RNG. Same seed + same config → same layout.
    pub seed: u64,
    pub spawn_algorithm: SpawnAlgorithm,
}

impl WorldSpawnConfig {
    /// Matches the normal game launch: 128×128 map, realistic biome-aware placement,
    /// and default population counts from `constants::world`.
    pub fn game_defaults() -> Self {
        Self {
            map_size: (WORLD_WIDTH, WORLD_HEIGHT),
            humans: HUMAN_SPAWN_COUNT,
            deer: DEER_SPAWN_COUNT,
            berry_bushes: BERRY_BUSH_SPAWN_COUNT,
            apple_trees: APPLE_TREE_SPAWN_COUNT,
            stone_nodes: STONE_NODE_SPAWN_COUNT,
            wood_logs: WOOD_LOG_SPAWN_COUNT,
            seed: 0,
            spawn_algorithm: SpawnAlgorithm::Realistic,
        }
    }

    /// Compute where each entity should be placed, without actually spawning anything.
    /// Callers use the returned [`SpawnLayout`] to spawn via their preferred spawners.
    pub fn compute_layout(&self, map: &WorldMap) -> SpawnLayout {
        match self.spawn_algorithm {
            SpawnAlgorithm::Realistic => compute_realistic_layout(self, map),
            SpawnAlgorithm::Uniform => compute_uniform_layout(self),
        }
    }
}

/// Resolved positions and initial resource amounts for all entity types.
/// Produced by [`WorldSpawnConfig::compute_layout`]; consumed by spawn functions.
#[derive(Debug, Clone, Default)]
pub struct SpawnLayout {
    pub human_positions: Vec<Vec2>,
    pub deer_positions: Vec<Vec2>,
    /// Each entry is (world position, initial berry count).
    pub berry_bush_positions: Vec<(Vec2, u32)>,
    /// Each entry is (world position, initial apple count).
    pub apple_tree_positions: Vec<(Vec2, u32)>,
    /// Each entry is (world position, initial stone count).
    pub stone_node_positions: Vec<(Vec2, u32)>,
    /// Each entry is (world position, initial wood count).
    pub wood_log_positions: Vec<(Vec2, u32)>,
}

// ─── Realistic layout ─────────────────────────────────────────────────────

fn compute_realistic_layout(config: &WorldSpawnConfig, map: &WorldMap) -> SpawnLayout {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut layout = SpawnLayout::default();

    let settlement = find_settlement_center(map, &SettlementSearch::default());

    // Berry bushes near the settlement are planted first so agents perceive
    // food as soon as they spawn.
    if let Some(center) = settlement {
        let near_settlement = cluster_positions(
            map,
            center,
            SETTLEMENT_BERRY_BUSH_COUNT,
            SETTLEMENT_FOOD_RADIUS_TILES,
            &mut rng,
        );
        for pos in near_settlement {
            layout.berry_bush_positions.push((pos, 4));
        }
    }

    // Humans cluster tightly around the settlement so they can see each other.
    layout.human_positions = match settlement {
        Some(center) => cluster_positions(
            map,
            center,
            config.humans,
            HUMAN_CLUSTER_RADIUS_TILES,
            &mut rng,
        ),
        None => fallback_random_walkable(map, &mut rng, config.humans),
    };

    // Apple trees prefer forest biomes.
    for _ in 0..config.apple_trees {
        if let Some(pos) = find_biome_tile(map, &mut rng, &[TileType::Forest], MAX_SPAWN_ATTEMPTS) {
            layout.apple_tree_positions.push((pos, 5));
        }
    }

    // Remaining berry bushes scatter across grass and forest.
    let scattered = config.berry_bushes.saturating_sub(if settlement.is_some() {
        SETTLEMENT_BERRY_BUSH_COUNT
    } else {
        0
    });
    for _ in 0..scattered {
        if let Some(pos) = find_biome_tile(
            map,
            &mut rng,
            &[TileType::Grass, TileType::Forest],
            MAX_SPAWN_ATTEMPTS,
        ) {
            layout.berry_bush_positions.push((pos, 4));
        }
    }

    // Stone nodes spawn in rocky terrain.
    for _ in 0..config.stone_nodes {
        if let Some(pos) = find_biome_tile(map, &mut rng, &[TileType::Rock], MAX_SPAWN_ATTEMPTS) {
            layout.stone_node_positions.push((pos, 5));
        }
    }

    // Wood logs scatter across forest biomes.
    for _ in 0..config.wood_logs {
        if let Some(pos) = find_biome_tile(map, &mut rng, &[TileType::Forest], MAX_SPAWN_ATTEMPTS) {
            layout.wood_log_positions.push((pos, 4));
        }
    }

    // Deer in small herds kept away from the settlement.
    layout.deer_positions = compute_deer_herd_positions(map, settlement, config.deer, &mut rng);

    layout
}

fn compute_deer_herd_positions(
    map: &WorldMap,
    settlement: Option<UVec2>,
    total: usize,
    rng: &mut impl rand::Rng,
) -> Vec<Vec2> {
    let allowed = [TileType::Grass, TileType::Forest];
    let mut positions = Vec::new();
    let mut attempts = 0usize;

    while positions.len() < total {
        let remaining = total - positions.len();
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
                    positions.push(pos);
                }
            }
            return positions;
        };

        let (anchor_tx, anchor_ty) = map.world_to_tile(anchor_pos);
        let herd_positions = cluster_positions(
            map,
            UVec2::new(anchor_tx, anchor_ty),
            herd_size,
            DEER_HERD_RADIUS_TILES,
            rng,
        );

        if herd_positions.is_empty() {
            positions.push(anchor_pos);
        } else {
            for pos in herd_positions {
                positions.push(pos);
                if positions.len() == total {
                    return positions;
                }
            }
        }

        attempts += 1;
        if attempts > total * 2 {
            break;
        }
    }

    positions
}

fn fallback_random_walkable(map: &WorldMap, rng: &mut impl rand::Rng, count: usize) -> Vec<Vec2> {
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

// ─── Uniform layout ───────────────────────────────────────────────────────

fn compute_uniform_layout(config: &WorldSpawnConfig) -> SpawnLayout {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
    let mut layout = SpawnLayout::default();

    for _ in 0..config.humans {
        layout.human_positions.push(random_uniform_pos(&mut rng));
    }
    for _ in 0..config.deer {
        layout.deer_positions.push(random_uniform_pos(&mut rng));
    }
    for _ in 0..config.berry_bushes {
        layout
            .berry_bush_positions
            .push((random_uniform_pos(&mut rng), 5));
    }
    for _ in 0..config.apple_trees {
        layout
            .apple_tree_positions
            .push((random_uniform_pos(&mut rng), 7));
    }
    for _ in 0..config.stone_nodes {
        layout
            .stone_node_positions
            .push((random_uniform_pos(&mut rng), 5));
    }
    for _ in 0..config.wood_logs {
        layout
            .wood_log_positions
            .push((random_uniform_pos(&mut rng), 4));
    }

    layout
}

fn random_uniform_pos(rng: &mut ChaCha8Rng) -> Vec2 {
    use rand::Rng;
    Vec2::new(
        rng.random_range(0.0..UNIFORM_AREA_PX),
        rng.random_range(0.0..UNIFORM_AREA_PX),
    )
}
