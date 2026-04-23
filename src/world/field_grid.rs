//! Sparse chunked scalar field grid.
//!
//! Stores per-tile deltas above an ambient baseline in dense 32×32
//! chunks allocated on demand. `sample_tile` returns `ambient + delta`;
//! untouched cells read as ambient exactly, so day/night ambient swings
//! pass through the grid automatically and the damping pass just pulls
//! deltas toward zero.

use bevy::math::IVec2;
use std::collections::HashMap;

pub const FIELD_CHUNK_SIZE: i32 = 32;
const CHUNK_CELL_COUNT: usize = (FIELD_CHUNK_SIZE * FIELD_CHUNK_SIZE) as usize;

/// A dense 32×32 block of per-tile deltas.
#[derive(Debug, Clone)]
pub struct FieldChunk {
    deltas: [f32; CHUNK_CELL_COUNT],
}

impl FieldChunk {
    fn new() -> Self {
        Self {
            deltas: [0.0; CHUNK_CELL_COUNT],
        }
    }

    fn local_index(local_x: i32, local_y: i32) -> usize {
        debug_assert!((0..FIELD_CHUNK_SIZE).contains(&local_x));
        debug_assert!((0..FIELD_CHUNK_SIZE).contains(&local_y));
        (local_y * FIELD_CHUNK_SIZE + local_x) as usize
    }

    pub fn delta_at(&self, local_x: i32, local_y: i32) -> f32 {
        self.deltas[Self::local_index(local_x, local_y)]
    }

    pub fn set_delta(&mut self, local_x: i32, local_y: i32, delta: f32) {
        self.deltas[Self::local_index(local_x, local_y)] = delta;
    }

    pub fn deltas_mut(&mut self) -> &mut [f32] {
        &mut self.deltas
    }

    pub(crate) fn any_delta_exceeds(&self, epsilon: f32) -> bool {
        self.deltas.iter().any(|d| d.abs() > epsilon)
    }
}

#[derive(Debug, Clone)]
pub struct FieldGrid {
    pub ambient: f32,
    chunks: HashMap<IVec2, FieldChunk>,
}

impl FieldGrid {
    pub fn new(ambient: f32) -> Self {
        Self {
            ambient,
            chunks: HashMap::new(),
        }
    }

    pub fn chunk_of_tile(tile: IVec2) -> (IVec2, IVec2) {
        let chunk = IVec2::new(
            tile.x.div_euclid(FIELD_CHUNK_SIZE),
            tile.y.div_euclid(FIELD_CHUNK_SIZE),
        );
        let local = IVec2::new(
            tile.x.rem_euclid(FIELD_CHUNK_SIZE),
            tile.y.rem_euclid(FIELD_CHUNK_SIZE),
        );
        (chunk, local)
    }

    pub fn sample_tile(&self, tile: IVec2) -> f32 {
        self.ambient + self.delta_at_tile(tile)
    }

    pub fn delta_at_tile(&self, tile: IVec2) -> f32 {
        let (chunk_coord, local) = Self::chunk_of_tile(tile);
        self.chunks
            .get(&chunk_coord)
            .map(|c| c.delta_at(local.x, local.y))
            .unwrap_or(0.0)
    }

    pub fn inject_at_tile(&mut self, tile: IVec2, amount: f32) {
        if amount == 0.0 {
            return;
        }
        let (_, local) = Self::chunk_of_tile(tile);
        let chunk = self.chunk_for_write(tile);
        let current = chunk.delta_at(local.x, local.y);
        chunk.set_delta(local.x, local.y, current + amount);
    }

    pub(crate) fn set_delta_at_tile(&mut self, tile: IVec2, delta: f32) {
        let (_, local) = Self::chunk_of_tile(tile);
        let chunk = self.chunk_for_write(tile);
        chunk.set_delta(local.x, local.y, delta);
    }

    fn chunk_for_write(&mut self, tile: IVec2) -> &mut FieldChunk {
        let (chunk_coord, _) = Self::chunk_of_tile(tile);
        self.chunks
            .entry(chunk_coord)
            .or_insert_with(FieldChunk::new)
    }

    pub fn active_chunks(&self) -> usize {
        self.chunks.len()
    }

    pub fn iter_chunks(&self) -> impl Iterator<Item = (IVec2, &FieldChunk)> {
        self.chunks.iter().map(|(k, v)| (*k, v))
    }

    pub(crate) fn chunks_mut(&mut self) -> &mut HashMap<IVec2, FieldChunk> {
        &mut self.chunks
    }

    pub fn set_ambient(&mut self, ambient: f32) {
        self.ambient = ambient;
    }

    pub fn prune_equilibrated(&mut self, epsilon: f32) {
        self.chunks
            .retain(|_, chunk| chunk.any_delta_exceeds(epsilon));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_unallocated_tile_returns_ambient() {
        let grid = FieldGrid::new(22.0);
        assert_eq!(grid.sample_tile(IVec2::new(100, 100)), 22.0);
        assert_eq!(grid.active_chunks(), 0);
    }

    #[test]
    fn inject_at_tile_reads_back() {
        let mut grid = FieldGrid::new(5.0);
        grid.inject_at_tile(IVec2::new(3, 4), 10.0);
        assert_eq!(grid.sample_tile(IVec2::new(3, 4)), 15.0);
    }

    #[test]
    fn inject_allocates_exactly_one_chunk_per_region() {
        let mut grid = FieldGrid::new(0.0);
        grid.inject_at_tile(IVec2::new(0, 0), 1.0);
        grid.inject_at_tile(IVec2::new(5, 5), 1.0);
        grid.inject_at_tile(IVec2::new(31, 31), 1.0);
        assert_eq!(grid.active_chunks(), 1);
        grid.inject_at_tile(IVec2::new(32, 0), 1.0);
        assert_eq!(grid.active_chunks(), 2);
    }

    #[test]
    fn repeated_injection_accumulates_delta() {
        let mut grid = FieldGrid::new(10.0);
        grid.inject_at_tile(IVec2::new(0, 0), 1.0);
        grid.inject_at_tile(IVec2::new(0, 0), 2.0);
        grid.inject_at_tile(IVec2::new(0, 0), 3.0);
        assert_eq!(grid.sample_tile(IVec2::new(0, 0)), 16.0);
        assert_eq!(grid.delta_at_tile(IVec2::new(0, 0)), 6.0);
    }

    #[test]
    fn negative_tile_coords_work() {
        let mut grid = FieldGrid::new(0.0);
        grid.inject_at_tile(IVec2::new(-1, -1), 5.0);
        assert_eq!(grid.sample_tile(IVec2::new(-1, -1)), 5.0);
        assert_eq!(grid.active_chunks(), 1);
    }

    /// The whole point of delta storage: shifting ambient moves all cells
    /// uniformly without clobbering local perturbations.
    #[test]
    fn ambient_shift_preserves_relative_perturbations() {
        let mut grid = FieldGrid::new(5.0);
        grid.inject_at_tile(IVec2::new(0, 0), 20.0);
        assert_eq!(grid.sample_tile(IVec2::new(0, 0)), 25.0);

        grid.set_ambient(22.0);

        assert_eq!(grid.sample_tile(IVec2::new(0, 0)), 42.0);
        assert_eq!(grid.sample_tile(IVec2::new(1, 0)), 22.0);
        assert_eq!(grid.sample_tile(IVec2::new(100, 100)), 22.0);
    }

    #[test]
    fn prune_equilibrated_drops_zero_delta_chunks() {
        let mut grid = FieldGrid::new(10.0);
        grid.inject_at_tile(IVec2::new(0, 0), 0.001);
        grid.inject_at_tile(IVec2::new(100, 100), 20.0);
        assert_eq!(grid.active_chunks(), 2);
        grid.prune_equilibrated(0.01);
        assert_eq!(grid.active_chunks(), 1);
    }

    #[test]
    fn chunk_of_tile_roundtrips() {
        for tile in [
            IVec2::new(0, 0),
            IVec2::new(31, 31),
            IVec2::new(32, 32),
            IVec2::new(-1, -1),
            IVec2::new(-32, -32),
            IVec2::new(100, -50),
        ] {
            let (chunk, local) = FieldGrid::chunk_of_tile(tile);
            let reconstructed = IVec2::new(
                chunk.x * FIELD_CHUNK_SIZE + local.x,
                chunk.y * FIELD_CHUNK_SIZE + local.y,
            );
            assert_eq!(reconstructed, tile, "roundtrip failed for {tile:?}");
        }
    }
}
