//! Spatial index for fast entity proximity queries using the WorldMap chunk grid as buckets.
//!
//! Reads: Transform (via Changed<Transform>), Physical (component marker), WorldMap chunk constants
//! Writes: SpatialIndex resource (buckets + entity_chunk tracking)
//! Upstream: world::map (CHUNK_SIZE, TILE_SIZE constants), world (Physical marker)
//! Downstream: agent::mind::perception (queries SpatialIndex for nearby entities)

use bevy::prelude::*;
use std::collections::HashMap;

use crate::world::map::{CHUNK_SIZE, TILE_SIZE};

pub struct SpatialIndexPlugin;

impl Plugin for SpatialIndexPlugin {
    fn build(&self, app: &mut App) {
        // FixedPreUpdate so perception (FixedUpdate) reads an up-to-date index. Movement
        // mutates Transform in FixedUpdate, so by the next FixedPreUpdate the changes are
        // visible via Changed<Transform>. Nothing in the regular Update schedule moves
        // `Physical` entities, so no PostUpdate run is needed.
        app.insert_resource(SpatialIndex::default())
            .add_systems(FixedPreUpdate, update_spatial_index);
    }
}

/// Spatial acceleration structure that maps chunk coordinates to the entities inside them.
///
/// Keys match `WorldMap.chunks` exactly — same `IVec2` chunk coordinates, same 16×16 tile
/// bucket size. One spatial decomposition for the whole project.
#[derive(Resource, Default)]
pub struct SpatialIndex {
    /// One entry per occupied chunk: chunk coord → entities currently in that chunk.
    buckets: HashMap<IVec2, Vec<Entity>>,
    /// Tracks the chunk each entity was last placed in so we know which bucket to
    /// remove from when an entity moves.
    entity_chunks: HashMap<Entity, IVec2>,
}

impl SpatialIndex {
    /// Update an entity's position in the index.
    ///
    /// The old chunk is derived from the internal tracking map — callers only supply the new chunk.
    pub fn update_entity(&mut self, entity: Entity, new_chunk: IVec2) {
        let old_chunk = self.entity_chunks.get(&entity).copied();

        if old_chunk == Some(new_chunk) {
            // Same bucket — nothing to do.
            return;
        }

        // Remove from old bucket.
        if let Some(old) = old_chunk
            && let Some(bucket) = self.buckets.get_mut(&old)
        {
            bucket.retain(|&e| e != entity);
            if bucket.is_empty() {
                self.buckets.remove(&old);
            }
        }

        // Insert into new bucket.
        self.buckets.entry(new_chunk).or_default().push(entity);
        self.entity_chunks.insert(entity, new_chunk);
    }

    /// Remove an entity from the index entirely (e.g. on despawn).
    pub fn remove_entity(&mut self, entity: Entity) {
        if let Some(chunk) = self.entity_chunks.remove(&entity)
            && let Some(bucket) = self.buckets.get_mut(&chunk)
        {
            bucket.retain(|&e| e != entity);
            if bucket.is_empty() {
                self.buckets.remove(&chunk);
            }
        }
    }

    /// Return all entities in chunks within `radius` world units of `pos`.
    ///
    /// The result is a superset of the true radius set — callers must still do a precise
    /// distance check if exact range matters. The chunk boundary is coarser than the radius
    /// so a few extra entities from adjacent chunks will be included. This is vastly cheaper
    /// than O(all_entities) linear scans.
    pub fn entities_near(&self, pos: Vec2, radius: f32) -> Vec<Entity> {
        let chunk_radius = chunk_radius_for(radius);
        let center_chunk = world_pos_to_chunk(pos);

        let mut result = Vec::new();
        for dy in -chunk_radius..=chunk_radius {
            for dx in -chunk_radius..=chunk_radius {
                let chunk = center_chunk + IVec2::new(dx, dy);
                if let Some(bucket) = self.buckets.get(&chunk) {
                    result.extend_from_slice(bucket);
                }
            }
        }
        result
    }

    /// Return all entities currently in the given chunk.
    pub fn entities_in_chunk(&self, chunk: IVec2) -> &[Entity] {
        self.buckets.get(&chunk).map(Vec::as_slice).unwrap_or(&[])
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Internal helpers
// ───────────────────────────────────────────────────────────────────────────

/// Convert a world position to a signed tile coordinate.
pub fn world_pos_to_tile(pos: Vec2) -> IVec2 {
    IVec2::new(
        (pos.x / TILE_SIZE).floor() as i32,
        (pos.y / TILE_SIZE).floor() as i32,
    )
}

/// The world-space center of a tile. Complement of `world_pos_to_tile`.
pub fn tile_center_px(tile: IVec2) -> Vec2 {
    Vec2::new(
        (tile.x as f32 + 0.5) * TILE_SIZE,
        (tile.y as f32 + 0.5) * TILE_SIZE,
    )
}

/// Convert a world position to a chunk coordinate using the same math as `WorldMap`.
pub fn world_pos_to_chunk(pos: Vec2) -> IVec2 {
    let tile = world_pos_to_tile(pos);
    IVec2::new(tile.x / CHUNK_SIZE as i32, tile.y / CHUNK_SIZE as i32)
}

/// How many chunks outward we need to check to cover a circle of `radius` world units.
///
/// One chunk = `CHUNK_SIZE × TILE_SIZE` world units (16 tiles × 16 px = 256 px).
pub fn chunk_radius_for(radius: f32) -> i32 {
    let chunk_world_size = CHUNK_SIZE as f32 * TILE_SIZE;
    (radius / chunk_world_size).ceil() as i32 + 1
}

// ───────────────────────────────────────────────────────────────────────────
// Systems
// ───────────────────────────────────────────────────────────────────────────

/// Listen for `Transform` changes on physical entities and keep the spatial index up to date.
fn update_spatial_index(
    mut index: ResMut<SpatialIndex>,
    moved: Query<(Entity, &Transform), (With<crate::world::Physical>, Changed<Transform>)>,
    mut removed: RemovedComponents<crate::world::Physical>,
) {
    for (entity, transform) in moved.iter() {
        let pos = transform.translation.truncate();
        let new_chunk = world_pos_to_chunk(pos);
        index.update_entity(entity, new_chunk);
    }

    for entity in removed.read() {
        index.remove_entity(entity);
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Tests
// ───────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(id: u32) -> Entity {
        Entity::from_bits(id as u64)
    }

    fn chunk(x: i32, y: i32) -> IVec2 {
        IVec2::new(x, y)
    }

    // ── Acceptance criteria from the issue ──────────────────────────────────

    #[test]
    fn insert_entity_query_nearby_returns_it() {
        let mut index = SpatialIndex::default();
        let e = entity(1);
        // Place entity at chunk (0, 0) — world position (0, 0) maps there.
        index.update_entity(e, chunk(0, 0));

        let results = index.entities_near(Vec2::ZERO, 1.0);
        assert!(results.contains(&e));
    }

    #[test]
    fn move_entity_old_chunk_no_longer_returns_it() {
        let mut index = SpatialIndex::default();
        let e = entity(2);
        index.update_entity(e, chunk(0, 0));
        // Move to chunk (5, 5).
        index.update_entity(e, chunk(5, 5));

        // Old chunk should not return entity.
        let old_results = index.entities_in_chunk(chunk(0, 0));
        assert!(!old_results.contains(&e));

        // New chunk should return entity.
        let new_results = index.entities_in_chunk(chunk(5, 5));
        assert!(new_results.contains(&e));
    }

    #[test]
    fn remove_entity_no_bucket_returns_it() {
        let mut index = SpatialIndex::default();
        let e = entity(3);
        index.update_entity(e, chunk(1, 1));
        index.remove_entity(e);

        let results = index.entities_in_chunk(chunk(1, 1));
        assert!(!results.contains(&e));

        // Wide query should also return nothing.
        let world_pos = Vec2::new(
            1.0 * CHUNK_SIZE as f32 * TILE_SIZE,
            1.0 * CHUNK_SIZE as f32 * TILE_SIZE,
        );
        let far_results = index.entities_near(world_pos, 999.0);
        assert!(!far_results.contains(&e));
    }

    #[test]
    fn query_with_radius_smaller_than_chunk_still_works() {
        let mut index = SpatialIndex::default();
        let e = entity(4);
        // Place entity at the center of chunk (2, 2).
        index.update_entity(e, chunk(2, 2));

        // Query from center of chunk (2, 2) with radius = 1 pixel (< 256 px chunk size).
        let chunk_world_size = CHUNK_SIZE as f32 * TILE_SIZE;
        let center = Vec2::new(2.5 * chunk_world_size, 2.5 * chunk_world_size);
        let results = index.entities_near(center, 1.0);
        assert!(
            results.contains(&e),
            "entity should be found even with tiny radius"
        );
    }

    #[test]
    fn query_spanning_multiple_chunks_returns_entities_from_all() {
        let mut index = SpatialIndex::default();
        let e1 = entity(5);
        let e2 = entity(6);
        let e3 = entity(7);
        index.update_entity(e1, chunk(0, 0));
        index.update_entity(e2, chunk(1, 0));
        index.update_entity(e3, chunk(0, 1));

        // Query from chunk (0,0) center with radius large enough to cover (1,0) and (0,1).
        let chunk_world_size = CHUNK_SIZE as f32 * TILE_SIZE;
        let center = Vec2::new(0.5 * chunk_world_size, 0.5 * chunk_world_size);
        // Radius = 1.5 chunk widths should cover adjacent chunks.
        let radius = chunk_world_size * 1.5;

        let results = index.entities_near(center, radius);
        assert!(results.contains(&e1));
        assert!(results.contains(&e2));
        assert!(results.contains(&e3));
    }

    #[test]
    fn deterministic_same_insertions_same_query_results() {
        let mut index_a = SpatialIndex::default();
        let mut index_b = SpatialIndex::default();
        for i in 1..=10u32 {
            index_a.update_entity(entity(i), chunk((i % 3) as i32, (i % 2) as i32));
            index_b.update_entity(entity(i), chunk((i % 3) as i32, (i % 2) as i32));
        }

        let mut results_a = index_a.entities_near(Vec2::new(100.0, 100.0), 800.0);
        let mut results_b = index_b.entities_near(Vec2::new(100.0, 100.0), 800.0);

        results_a.sort_by_key(|e| e.to_bits());
        results_b.sort_by_key(|e| e.to_bits());
        assert_eq!(results_a, results_b);
    }

    // ── Additional edge cases ────────────────────────────────────────────────

    #[test]
    fn update_entity_to_same_chunk_is_idempotent() {
        let mut index = SpatialIndex::default();
        let e = entity(10);
        index.update_entity(e, chunk(0, 0));
        index.update_entity(e, chunk(0, 0));

        // Should appear exactly once.
        let bucket = index.entities_in_chunk(chunk(0, 0));
        let count = bucket.iter().filter(|&&x| x == e).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn remove_nonexistent_entity_does_not_panic() {
        let mut index = SpatialIndex::default();
        index.remove_entity(entity(999)); // Should not panic.
    }

    #[test]
    fn empty_index_returns_empty_results() {
        let index = SpatialIndex::default();
        let results = index.entities_near(Vec2::new(500.0, 500.0), 1000.0);
        assert!(results.is_empty());
    }

    #[test]
    fn world_pos_to_chunk_maps_correctly() {
        // Position (0, 0) → chunk (0, 0).
        assert_eq!(world_pos_to_chunk(Vec2::ZERO), IVec2::ZERO);

        // Position at exactly one chunk width (256 px) → chunk (1, 0).
        let one_chunk = CHUNK_SIZE as f32 * TILE_SIZE;
        assert_eq!(
            world_pos_to_chunk(Vec2::new(one_chunk, 0.0)),
            IVec2::new(1, 0)
        );

        // Position just inside first chunk → still chunk (0, 0).
        assert_eq!(
            world_pos_to_chunk(Vec2::new(one_chunk - 1.0, 0.0)),
            IVec2::new(0, 0)
        );
    }
}
