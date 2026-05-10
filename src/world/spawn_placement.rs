//! Resource-aware spawn placement for the initial world population.
//!
//! Reads: WorldMap, TileType (terrain queries only — no ECS state)
//! Writes: nothing — pure functions returning tile coordinates / world positions
//! Upstream: spawner (initial population system)
//! Downstream: spawner uses these to cluster humans, scatter herds, and bias resources to biomes

use bevy::math::{UVec2, Vec2};
use rand::Rng;

use crate::world::map::{TileType, WorldMap};

/// Tunables for the settlement-location search.
#[derive(Clone, Copy, Debug)]
pub struct SettlementSearch {
    /// Maximum tile distance from a water tile for a candidate to count as
    /// "near water". Candidates farther than this are skipped.
    pub max_water_distance: u32,
    /// Minimum number of walkable tiles inside the cluster radius required
    /// for a candidate to be considered habitable.
    pub min_cluster_space: u32,
    /// Square radius (tiles) used when counting cluster space.
    pub cluster_radius: u32,
    /// Optional x-tile range `[min, max)` to constrain the search to one
    /// side of a river. If `None`, the entire map width is searched.
    pub x_range: Option<(u32, u32)>,
}

impl Default for SettlementSearch {
    fn default() -> Self {
        Self {
            max_water_distance: 8,
            min_cluster_space: 12,
            cluster_radius: 3,
            x_range: None,
        }
    }
}

/// Score and pick the best grass tile to anchor a human settlement.
///
/// A good settlement spot is on grass, has water within `max_water_distance`
/// tiles, and has plenty of walkable terrain in its immediate neighborhood.
/// Returns `None` if no candidate clears the minimum thresholds.
pub fn find_settlement_center(map: &WorldMap, search: &SettlementSearch) -> Option<UVec2> {
    let mut best: Option<(UVec2, f32)> = None;
    let (x_min, x_max) = search.x_range.unwrap_or((0, map.width));

    for y in 0..map.height {
        for x in x_min..x_max {
            let Some(tile) = map.get_tile(x, y) else {
                continue;
            };
            if tile != TileType::Grass {
                continue;
            }

            let Some(water_dist) = nearest_water_distance(map, x, y, search.max_water_distance)
            else {
                continue;
            };

            // Don't plant a village on top of the shoreline — leave room for the
            // cluster to spread without spilling into water.
            if water_dist == 0 {
                continue;
            }

            let cluster_space = walkable_neighbors_count(map, x, y, search.cluster_radius);
            if cluster_space < search.min_cluster_space {
                continue;
            }

            // Closer water and more habitable space both raise the score.
            let water_bonus = (search.max_water_distance as f32 - water_dist as f32) * 4.0;
            let score = cluster_space as f32 + water_bonus;

            if best.is_none_or(|(_, b)| score > b) {
                best = Some((UVec2::new(x, y), score));
            }
        }
    }

    best.map(|(pos, _)| pos)
}

/// Generate up to `count` walkable world positions (may be fewer if the
/// attempt budget runs out) clustered around the tile at `center_tile`.
/// Positions are sampled at random tile offsets within `radius` and rejected
/// if they fall on impassable terrain. Shallow water is excluded so spawned
/// entities never start standing in the river.
pub fn cluster_positions(
    map: &WorldMap,
    center_tile: UVec2,
    count: usize,
    radius: u32,
    rng: &mut impl Rng,
) -> Vec<Vec2> {
    let mut positions = Vec::with_capacity(count);
    let max_attempts = count * 30;
    let mut attempts = 0;

    while positions.len() < count && attempts < max_attempts {
        attempts += 1;
        let dx = rng.random_range(-(radius as i32)..=(radius as i32));
        let dy = rng.random_range(-(radius as i32)..=(radius as i32));
        let Some((nx, ny)) = offset(center_tile.x, center_tile.y, dx, dy) else {
            continue;
        };
        let Some(tile) = map.get_tile(nx, ny) else {
            continue;
        };
        if !is_solid_ground(tile) {
            continue;
        }
        positions.push(map.tile_to_world(nx as i32, ny as i32));
    }

    positions
}

/// Find a random tile of an allowed type at least `min_distance` tiles from
/// `away_from`. Returns the world position (tile center), or `None` if no
/// candidate is found within `max_attempts` rolls.
pub fn find_tile_away_from(
    map: &WorldMap,
    rng: &mut impl Rng,
    allowed: &[TileType],
    away_from: UVec2,
    min_distance: u32,
    max_attempts: usize,
) -> Option<Vec2> {
    let min_sq = (min_distance as i64) * (min_distance as i64);
    for _ in 0..max_attempts {
        let x = rng.random_range(0..map.width);
        let y = rng.random_range(0..map.height);
        let Some(tile) = map.get_tile(x, y) else {
            continue;
        };
        if !allowed.contains(&tile) {
            continue;
        }
        if tile_distance_sq(UVec2::new(x, y), away_from) >= min_sq {
            return Some(map.tile_to_world(x as i32, y as i32));
        }
    }
    None
}

/// Find a random walkable tile of an allowed type. Returns its world position
/// (tile center), or `None` if no candidate is found within `max_attempts` rolls.
pub fn find_biome_tile(
    map: &WorldMap,
    rng: &mut impl Rng,
    allowed: &[TileType],
    max_attempts: usize,
) -> Option<Vec2> {
    for _ in 0..max_attempts {
        let x = rng.random_range(0..map.width);
        let y = rng.random_range(0..map.height);
        let Some(tile) = map.get_tile(x, y) else {
            continue;
        };
        if allowed.contains(&tile) {
            return Some(map.tile_to_world(x as i32, y as i32));
        }
    }
    None
}

/// Find a random water tile (deep or shallow). Returns the world position
/// (tile center) or `None` if no water is present within `max_attempts` rolls.
/// Used by fish spawning — the only callers that *want* to land on water.
pub fn find_water_tile(map: &WorldMap, rng: &mut impl Rng, max_attempts: usize) -> Option<Vec2> {
    for _ in 0..max_attempts {
        let x = rng.random_range(0..map.width);
        let y = rng.random_range(0..map.height);
        let Some(tile) = map.get_tile(x, y) else {
            continue;
        };
        if matches!(tile, TileType::Water | TileType::ShallowWater) {
            return Some(map.tile_to_world(x as i32, y as i32));
        }
    }
    None
}

/// Cluster `count` water-only positions around `center_tile`. Used to drop a
/// school of minnows in roughly the same patch instead of scattering them
/// across every body of water on the map.
pub fn cluster_water_positions(
    map: &WorldMap,
    center_tile: UVec2,
    count: usize,
    radius: u32,
    rng: &mut impl Rng,
) -> Vec<Vec2> {
    let mut positions = Vec::with_capacity(count);
    let max_attempts = count * 30;
    let mut attempts = 0;

    while positions.len() < count && attempts < max_attempts {
        attempts += 1;
        let dx = rng.random_range(-(radius as i32)..=(radius as i32));
        let dy = rng.random_range(-(radius as i32)..=(radius as i32));
        let Some((nx, ny)) = offset(center_tile.x, center_tile.y, dx, dy) else {
            continue;
        };
        let Some(tile) = map.get_tile(nx, ny) else {
            continue;
        };
        if !matches!(tile, TileType::Water | TileType::ShallowWater) {
            continue;
        }
        positions.push(map.tile_to_world(nx as i32, ny as i32));
    }

    positions
}

/// Like [`find_biome_tile`] but rejects positions within `min_water_distance`
/// tiles of any water tile. Used for vegetation that should cluster in the
/// island interior away from the coast.
pub fn find_interior_biome_tile(
    map: &WorldMap,
    rng: &mut impl Rng,
    allowed: &[TileType],
    min_water_distance: u32,
    max_attempts: usize,
) -> Option<Vec2> {
    for _ in 0..max_attempts {
        let x = rng.random_range(0..map.width);
        let y = rng.random_range(0..map.height);
        let Some(tile) = map.get_tile(x, y) else {
            continue;
        };
        if !allowed.contains(&tile) {
            continue;
        }
        // Reject if there's water within min_water_distance tiles.
        if nearest_water_distance(map, x, y, min_water_distance).is_some() {
            continue;
        }
        return Some(map.tile_to_world(x as i32, y as i32));
    }
    None
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// BFS-like ring scan for the nearest water tile around (cx, cy). Returns the
/// Chebyshev distance to the closest water tile, or `None` if none is within
/// `max_radius`.
pub(crate) fn nearest_water_distance(
    map: &WorldMap,
    cx: u32,
    cy: u32,
    max_radius: u32,
) -> Option<u32> {
    for r in 0..=max_radius {
        let r_i = r as i32;
        for dy in -r_i..=r_i {
            for dx in -r_i..=r_i {
                // Only inspect the outer ring at distance r — inner rings were
                // already covered on previous iterations.
                if dx.abs() != r_i && dy.abs() != r_i {
                    continue;
                }
                let Some((nx, ny)) = offset(cx, cy, dx, dy) else {
                    continue;
                };
                if let Some(tile) = map.get_tile(nx, ny)
                    && matches!(tile, TileType::Water | TileType::ShallowWater)
                {
                    return Some(r);
                }
            }
        }
    }
    None
}

fn walkable_neighbors_count(map: &WorldMap, cx: u32, cy: u32, radius: u32) -> u32 {
    let mut count = 0;
    let r = radius as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            let Some((nx, ny)) = offset(cx, cy, dx, dy) else {
                continue;
            };
            if let Some(tile) = map.get_tile(nx, ny)
                && is_solid_ground(tile)
            {
                count += 1;
            }
        }
    }
    count
}

fn offset(cx: u32, cy: u32, dx: i32, dy: i32) -> Option<(u32, u32)> {
    let nx = (cx as i32) + dx;
    let ny = (cy as i32) + dy;
    if nx < 0 || ny < 0 {
        return None;
    }
    Some((nx as u32, ny as u32))
}

fn tile_distance_sq(a: UVec2, b: UVec2) -> i64 {
    let dx = a.x as i64 - b.x as i64;
    let dy = a.y as i64 - b.y as i64;
    dx * dx + dy * dy
}

/// Solid ground for entity placement: walkable land, never water or shallow water.
/// Entities should not be initialized standing in any kind of water.
fn is_solid_ground(tile: TileType) -> bool {
    !matches!(tile, TileType::Water | TileType::ShallowWater)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::map::{CHUNK_SIZE, Chunk};
    use bevy::math::IVec2;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    fn empty_map(width: u32, height: u32) -> WorldMap {
        let mut map = WorldMap::new(width, height);
        let chunks_x = width.div_ceil(CHUNK_SIZE);
        let chunks_y = height.div_ceil(CHUNK_SIZE);
        for cy in 0..chunks_y as i32 {
            for cx in 0..chunks_x as i32 {
                map.chunks.insert(IVec2::new(cx, cy), Chunk::new(cx, cy));
            }
        }
        map
    }

    fn fill_with(map: &mut WorldMap, tile: TileType) {
        for y in 0..map.height {
            for x in 0..map.width {
                map.set_tile(x, y, tile);
            }
        }
    }

    #[test]
    fn settlement_returns_none_when_no_water_in_range() {
        let mut map = empty_map(CHUNK_SIZE, CHUNK_SIZE);
        fill_with(&mut map, TileType::Grass);

        let result = find_settlement_center(&map, &SettlementSearch::default());
        assert!(result.is_none());
    }

    #[test]
    fn settlement_picks_grass_tile_near_water() {
        let mut map = empty_map(CHUNK_SIZE, CHUNK_SIZE);
        fill_with(&mut map, TileType::Grass);
        // Drop a water column on the far left of the map.
        for y in 0..map.height {
            map.set_tile(0, y, TileType::Water);
        }

        let center = find_settlement_center(&map, &SettlementSearch::default())
            .expect("settlement should be found near water column");

        // The chosen tile is on grass.
        assert_eq!(map.get_tile(center.x, center.y), Some(TileType::Grass));
        // It must lie within the configured water-distance budget.
        assert!(center.x <= SettlementSearch::default().max_water_distance);
    }

    #[test]
    fn settlement_does_not_sit_directly_on_shoreline() {
        let mut map = empty_map(CHUNK_SIZE, CHUNK_SIZE);
        fill_with(&mut map, TileType::Grass);
        for y in 0..map.height {
            map.set_tile(0, y, TileType::Water);
        }

        let center = find_settlement_center(&map, &SettlementSearch::default())
            .expect("settlement should be found");

        // The chosen tile must have a non-water neighbor in every direction
        // (i.e. it sits at least one tile inland).
        assert!(center.x >= 1, "settlement should not be directly on shore");
    }

    #[test]
    fn settlement_rejects_locations_with_too_little_cluster_space() {
        let mut map = empty_map(CHUNK_SIZE, CHUNK_SIZE);
        fill_with(&mut map, TileType::Water);
        // A single grass tile next to water has no room for a cluster.
        map.set_tile(5, 5, TileType::Grass);

        let result = find_settlement_center(&map, &SettlementSearch::default());
        assert!(result.is_none());
    }

    #[test]
    fn cluster_positions_returns_only_walkable_tiles() {
        let mut map = empty_map(CHUNK_SIZE, CHUNK_SIZE);
        fill_with(&mut map, TileType::Grass);
        // Block out half the world with water.
        for y in 0..map.height {
            for x in 0..(map.width / 2) {
                map.set_tile(x, y, TileType::Water);
            }
        }

        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let positions = cluster_positions(&map, UVec2::new(12, 8), 6, 4, &mut rng);

        assert!(!positions.is_empty());
        for pos in &positions {
            let tile = map.tile_at(*pos).expect("position must be in bounds");
            assert!(tile.is_walkable());
            assert_ne!(tile, TileType::ShallowWater);
            assert_ne!(tile, TileType::Water);
        }
    }

    #[test]
    fn cluster_positions_stays_within_radius() {
        let mut map = empty_map(CHUNK_SIZE, CHUNK_SIZE);
        fill_with(&mut map, TileType::Grass);

        let mut rng = ChaCha8Rng::seed_from_u64(11);
        let center_tile = UVec2::new(8, 8);
        let radius = 3;
        let positions = cluster_positions(&map, center_tile, 8, radius, &mut rng);

        let center_world = map.tile_to_world(center_tile.x as i32, center_tile.y as i32);
        for pos in positions {
            let dx = (pos.x - center_world.x).abs();
            let dy = (pos.y - center_world.y).abs();
            // radius tiles in either direction is the worst case.
            let max = (radius as f32 + 0.5) * crate::world::map::TILE_SIZE;
            assert!(dx <= max && dy <= max, "pos {pos:?} outside radius");
        }
    }

    #[test]
    fn find_tile_away_from_respects_min_distance() {
        let mut map = empty_map(CHUNK_SIZE, CHUNK_SIZE);
        fill_with(&mut map, TileType::Grass);

        let mut rng = ChaCha8Rng::seed_from_u64(99);
        let origin = UVec2::new(8, 8);
        let min_distance = 6;

        let pos = find_tile_away_from(
            &map,
            &mut rng,
            &[TileType::Grass],
            origin,
            min_distance,
            200,
        )
        .expect("should find a far tile on a uniform grass map");

        let (tx, ty) = map.world_to_tile(pos);
        let dx = tx as i64 - origin.x as i64;
        let dy = ty as i64 - origin.y as i64;
        assert!(dx * dx + dy * dy >= (min_distance as i64).pow(2));
    }

    #[test]
    fn find_interior_biome_tile_rejects_tiles_near_water() {
        // 32x32 map with water on the outer 8 tiles; interior 16x16 is grass.
        let mut map = empty_map(32, 32);
        fill_with(&mut map, TileType::Grass);
        for y in 0..32 {
            for x in 0..32 {
                if !(8..24).contains(&x) || !(8..24).contains(&y) {
                    map.set_tile(x, y, TileType::Water);
                }
            }
        }

        let mut rng = ChaCha8Rng::seed_from_u64(123);
        let pos = find_interior_biome_tile(&map, &mut rng, &[TileType::Grass], 5, 500)
            .expect("should find a deep-interior grass tile");

        let (tx, ty) = map.world_to_tile(pos);
        // Must be at least 5 tiles from any water in every direction:
        // grass band is 8..24, so interior with 5-tile buffer is 13..19.
        assert!(
            (13..19).contains(&tx),
            "x={tx} too close to water (need 13..19)"
        );
        assert!(
            (13..19).contains(&ty),
            "y={ty} too close to water (need 13..19)"
        );
    }

    #[test]
    fn find_interior_biome_tile_returns_none_if_no_interior() {
        // All grass is too close to water — should give up.
        let mut map = empty_map(16, 16);
        fill_with(&mut map, TileType::Grass);
        // Water column down the middle splits the map.
        for y in 0..16 {
            map.set_tile(8, y, TileType::Water);
        }

        let mut rng = ChaCha8Rng::seed_from_u64(7);
        // No tile is more than 8 from the water column.
        let result = find_interior_biome_tile(&map, &mut rng, &[TileType::Grass], 12, 200);
        assert!(result.is_none());
    }

    #[test]
    fn find_biome_tile_returns_only_allowed_types() {
        let mut map = empty_map(CHUNK_SIZE, CHUNK_SIZE);
        fill_with(&mut map, TileType::Rock);
        // Sprinkle some grass tiles.
        for x in 4..8 {
            for y in 4..8 {
                map.set_tile(x, y, TileType::Grass);
            }
        }

        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let pos = find_biome_tile(&map, &mut rng, &[TileType::Grass], 200)
            .expect("should find a grass tile");
        assert_eq!(map.tile_at(pos), Some(TileType::Grass));
    }
}
