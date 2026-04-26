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
    HUMAN_SPAWN_COUNT, MAX_SPAWN_ATTEMPTS, SECOND_GROUP_SPAWN_COUNT, SETTLEMENT_BERRY_BUSH_COUNT,
    SETTLEMENT_FOOD_RADIUS_TILES, STONE_NODE_SPAWN_COUNT, WOLF_MIN_DISTANCE_FROM_SETTLEMENT,
    WOLF_PACK_RADIUS_TILES, WOLF_PACK_SIZE, WOLF_SPAWN_COUNT, WOOD_LOG_SPAWN_COUNT,
};
use crate::world::map::{
    DEFAULT_TERRAIN_SEED, TileType, WORLD_HEIGHT, WORLD_WIDTH, WorldMap, river_center_x,
};
use crate::world::spawn_placement::{
    SettlementSearch, cluster_positions, find_biome_tile, find_interior_biome_tile,
    find_settlement_center, find_tile_away_from,
};

/// Minimum tile distance from any water for vegetation that should cluster
/// in the interior (apple trees, berry bushes scattered outside settlements).
/// Calibrated for a 512x512 island with a coast band of ~30 tiles.
const VEGETATION_INTERIOR_MIN_WATER_DIST: u32 = 12;

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
    /// Number of humans in the second group, spawned on the opposite side of
    /// the river from the first settlement. Zero disables the second group.
    pub second_humans: usize,
    pub deer: usize,
    pub wolves: usize,
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
            second_humans: SECOND_GROUP_SPAWN_COUNT,
            deer: DEER_SPAWN_COUNT,
            wolves: WOLF_SPAWN_COUNT,
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
    /// Second human cluster, spawned on the opposite side of the river from
    /// [`Self::human_positions`]. Empty when the config disables it or when
    /// no suitable site was found on the other side.
    pub second_human_positions: Vec<Vec2>,
    /// Deer positions grouped by herd. Each inner `Vec` is one herd that
    /// should be mutually introduced to each other at spawn so herd cohesion
    /// (#260) has a real relationship graph to decay against.
    pub deer_herds: Vec<Vec<Vec2>>,
    /// Wolf positions grouped by pack. Same herd-cohesion story — pack-mates
    /// are introduced at spawn with high Affection so the flocking drive
    /// pulls them together.
    pub wolf_packs: Vec<Vec<Vec2>>,
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

    // Second human group across the river: strangers separated by a natural
    // barrier so stranger dynamics and cultural divergence can emerge.
    if config.second_humans > 0
        && let Some(first) = settlement
        && let Some(second_center) = find_second_settlement_across_river(map, first)
    {
        layout.second_human_positions = cluster_positions(
            map,
            second_center,
            config.second_humans,
            HUMAN_CLUSTER_RADIUS_TILES,
            &mut rng,
        );
    }

    // Apple trees cluster in the island interior, away from the coast.
    for _ in 0..config.apple_trees {
        if let Some(pos) = find_interior_biome_tile(
            map,
            &mut rng,
            &[TileType::Grass],
            VEGETATION_INTERIOR_MIN_WATER_DIST,
            MAX_SPAWN_ATTEMPTS,
        ) {
            layout.apple_tree_positions.push((pos, 5));
        }
    }

    // Remaining berry bushes scatter across the interior, away from the coast.
    let scattered = config.berry_bushes.saturating_sub(if settlement.is_some() {
        SETTLEMENT_BERRY_BUSH_COUNT
    } else {
        0
    });
    for _ in 0..scattered {
        if let Some(pos) = find_interior_biome_tile(
            map,
            &mut rng,
            &[TileType::Grass],
            VEGETATION_INTERIOR_MIN_WATER_DIST,
            MAX_SPAWN_ATTEMPTS,
        ) {
            layout.berry_bush_positions.push((pos, 4));
        }
    }

    // Stone nodes spawn on rocky terrain (Rock OR Gravel) so resources stay
    // reliable even on small maps where full Rock peaks may not form.
    for _ in 0..config.stone_nodes {
        if let Some(pos) = find_biome_tile(
            map,
            &mut rng,
            &[TileType::Rock, TileType::Gravel],
            MAX_SPAWN_ATTEMPTS,
        ) {
            layout.stone_node_positions.push((pos, 5));
        }
    }

    // Wood logs scatter across grass.
    for _ in 0..config.wood_logs {
        if let Some(pos) = find_biome_tile(map, &mut rng, &[TileType::Grass], MAX_SPAWN_ATTEMPTS) {
            layout.wood_log_positions.push((pos, 4));
        }
    }

    // Deer in small herds kept away from the settlement.
    layout.deer_herds = compute_deer_herd_positions(map, settlement, config.deer, &mut rng);

    // Wolves in packs in deep forest, well away from the settlement.
    layout.wolf_packs = compute_wolf_pack_positions(map, settlement, config.wolves, &mut rng);

    layout
}

/// Finds a settlement site on the opposite side of the river from `first`.
///
/// Determines which side of the river the first settlement is on by sampling
/// `river_center_x` at the first settlement's row, then constrains the
/// settlement search to the opposite half of the map (with a 4-tile buffer
/// away from the river itself so the cluster doesn't spill onto the banks).
fn find_second_settlement_across_river(map: &WorldMap, first: UVec2) -> Option<UVec2> {
    const RIVER_BUFFER_TILES: u32 = 4;

    let river_cx = river_center_x(first.y, map.width, DEFAULT_TERRAIN_SEED);
    let x_range = if first.x < river_cx {
        (river_cx + RIVER_BUFFER_TILES, map.width)
    } else {
        (0, river_cx.saturating_sub(RIVER_BUFFER_TILES))
    };

    find_settlement_center(
        map,
        &SettlementSearch {
            x_range: Some(x_range),
            ..Default::default()
        },
    )
}

fn compute_deer_herd_positions(
    map: &WorldMap,
    settlement: Option<UVec2>,
    total: usize,
    rng: &mut impl rand::Rng,
) -> Vec<Vec<Vec2>> {
    let allowed = [TileType::Grass];
    let mut herds: Vec<Vec<Vec2>> = Vec::new();
    let mut placed = 0usize;
    let mut attempts = 0usize;

    while placed < total {
        let remaining = total - placed;
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
            // Fallback: scatter the remaining deer as singletons so at least
            // they exist. Singletons are their own "herd of one" and skip
            // the mutual-introduction step in the spawner.
            for _ in 0..remaining {
                if let Some(pos) = find_biome_tile(map, rng, &allowed, MAX_SPAWN_ATTEMPTS) {
                    herds.push(vec![pos]);
                }
            }
            return herds;
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
            herds.push(vec![anchor_pos]);
            placed += 1;
        } else {
            // Cap the herd to `remaining` so we don't overshoot `total`.
            let capped: Vec<Vec2> = herd_positions.into_iter().take(remaining).collect();
            placed += capped.len();
            herds.push(capped);
        }

        attempts += 1;
        if attempts > total * 2 {
            break;
        }
    }

    herds
}

fn compute_wolf_pack_positions(
    map: &WorldMap,
    settlement: Option<UVec2>,
    total: usize,
    rng: &mut impl rand::Rng,
) -> Vec<Vec<Vec2>> {
    let allowed = [TileType::Grass];
    let mut packs: Vec<Vec<Vec2>> = Vec::new();
    let mut placed = 0usize;
    let mut attempts = 0usize;

    while placed < total {
        let remaining = total - placed;
        let pack_size = remaining.min(WOLF_PACK_SIZE);

        let anchor = match settlement {
            Some(center) => find_tile_away_from(
                map,
                rng,
                &allowed,
                center,
                WOLF_MIN_DISTANCE_FROM_SETTLEMENT,
                MAX_SPAWN_ATTEMPTS,
            ),
            None => find_biome_tile(map, rng, &allowed, MAX_SPAWN_ATTEMPTS),
        };

        let Some(anchor_pos) = anchor else {
            bevy::log::warn!(
                "wolf spawn: no qualifying tile found ({} wolves unplaced)",
                remaining
            );
            break;
        };

        let (anchor_tx, anchor_ty) = map.world_to_tile(anchor_pos);
        let pack_positions = cluster_positions(
            map,
            UVec2::new(anchor_tx, anchor_ty),
            pack_size,
            WOLF_PACK_RADIUS_TILES,
            rng,
        );

        if pack_positions.is_empty() {
            packs.push(vec![anchor_pos]);
            placed += 1;
        } else {
            let capped: Vec<Vec2> = pack_positions.into_iter().take(remaining).collect();
            placed += capped.len();
            packs.push(capped);
        }

        attempts += 1;
        if attempts > total * 2 {
            bevy::log::warn!(
                "wolf spawn: placement loop exceeded limit ({} wolves unplaced)",
                total - placed
            );
            break;
        }
    }

    packs
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
    // Uniform layout doesn't cluster — each deer/wolf is its own "herd of
    // one" so the spawner's introduction loop trivially skips them.
    for _ in 0..config.deer {
        layout.deer_herds.push(vec![random_uniform_pos(&mut rng)]);
    }
    for _ in 0..config.wolves {
        layout.wolf_packs.push(vec![random_uniform_pos(&mut rng)]);
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
