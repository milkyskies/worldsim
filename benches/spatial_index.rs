//! Benchmarks for SpatialIndex proximity queries (#198).
//!
//! Run with: `cargo bench --bench spatial_index`.
//!
//! Measures the before/after improvement for the visual perception hot path:
//! - `entities_near` on the spatial index (new path)
//! - Linear scan over all entities (old path, simulated with a Vec of positions)

use bevy::prelude::{Entity, IVec2, Vec2};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use worldsim::world::map::{CHUNK_SIZE, MAP_CHUNKS_X, MAP_CHUNKS_Y, TILE_SIZE};
use worldsim::world::spatial_index::SpatialIndex;

/// Populate a spatial index with `n` entities spread uniformly across the 8×8 chunk map.
fn populated_index(n: usize) -> SpatialIndex {
    let mut index = SpatialIndex::default();
    for i in 0..n {
        let entity = Entity::from_bits(i as u64 + 1);
        let chunk = IVec2::new(
            (i as i32 % MAP_CHUNKS_X as i32),
            ((i as i32 / MAP_CHUNKS_X as i32) % MAP_CHUNKS_Y as i32),
        );
        index.update_entity(entity, chunk);
    }
    index
}

/// Simulate the old O(n) linear scan: given a list of world positions, collect
/// all that fall within `radius` of the query position.
fn linear_scan(positions: &[Vec2], query_pos: Vec2, radius: f32) -> Vec<usize> {
    positions
        .iter()
        .enumerate()
        .filter(|(_, &pos)| query_pos.distance(pos) <= radius)
        .map(|(i, _)| i)
        .collect()
}

fn bench_entities_near(c: &mut Criterion) {
    let mut group = c.benchmark_group("spatial_index/entities_near");

    // Query position: center of the map.
    let map_world_size = 8.0 * CHUNK_SIZE as f32 * TILE_SIZE;
    let center = Vec2::new(map_world_size / 2.0, map_world_size / 2.0);
    // Perception radius: 128 px (8 tiles) — representative default vision range.
    let radius = 128.0_f32;

    for &n in &[100usize, 1000] {
        let index = populated_index(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("spatial_index", n), &n, |b, _| {
            b.iter(|| {
                let results = index.entities_near(black_box(center), black_box(radius));
                black_box(results);
            });
        });
    }

    group.finish();
}

fn bench_linear_scan_vs_spatial(c: &mut Criterion) {
    let mut group = c.benchmark_group("spatial_index/linear_vs_spatial");

    let map_world_size = 8.0 * CHUNK_SIZE as f32 * TILE_SIZE;
    let center = Vec2::new(map_world_size / 2.0, map_world_size / 2.0);
    let radius = 128.0_f32;

    for &n in &[100usize, 1000] {
        // Build the same entity positions for linear scan comparison.
        let chunk_world = CHUNK_SIZE as f32 * TILE_SIZE;
        let positions: Vec<Vec2> = (0..n)
            .map(|i| {
                let cx = (i % MAP_CHUNKS_X as usize) as f32;
                let cy = ((i / MAP_CHUNKS_X as usize) % MAP_CHUNKS_Y as usize) as f32;
                Vec2::new((cx + 0.5) * chunk_world, (cy + 0.5) * chunk_world)
            })
            .collect();

        let index = populated_index(n);

        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("linear_scan", n), &n, |b, _| {
            b.iter(|| {
                let results =
                    linear_scan(black_box(&positions), black_box(center), black_box(radius));
                black_box(results);
            });
        });

        group.bench_with_input(BenchmarkId::new("spatial_index", n), &n, |b, _| {
            b.iter(|| {
                let results = index.entities_near(black_box(center), black_box(radius));
                black_box(results);
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_entities_near, bench_linear_scan_vs_spatial);
criterion_main!(benches);
