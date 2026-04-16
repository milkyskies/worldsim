use crate::agent::mind::knowledge::{MindGraph, Node, Predicate, Value};
use crate::world::map::WorldMap;
use crate::world::spatial_index::world_pos_to_chunk;
use bevy::math::IVec2;
use bevy::prelude::Vec2;
use rand::Rng;

/// Penalty term for chunks the MindGraph marks as recently-`Explored`.
/// Decays as `1000 / (age + 1)` so a fresh visit is a big hit and a very
/// old visit is almost free. Chunks with no `Explored` triple return 0.
pub fn staleness_penalty(mind: &MindGraph, chunk: IVec2, current_tick: u64) -> f32 {
    let triples = mind.query(
        Some(&Node::Chunk((chunk.x, chunk.y))),
        Some(Predicate::Explored),
        None,
    );
    if let Some(triple) = triples.first()
        && let Value::Boolean(true) = triple.object
    {
        let age = (current_tick as i32 - triple.meta.timestamp as i32).max(0) as f32;
        1000.0 / (age + 1.0)
    } else {
        0.0
    }
}

/// Rejection-sample walkable tiles and keep the lowest-scoring one. The
/// caller's `score_fn` receives both the candidate position and its
/// chunk coordinate; a small distance term is added automatically so
/// candidates tied on the caller's score prefer the nearer one. Shared
/// between Explore and LookFor so both pickers use the same sampling
/// discipline.
pub fn sample_walkable_scored<F>(
    current_pos: Vec2,
    world_map: &WorldMap,
    samples: u32,
    rng: &mut dyn rand::RngCore,
    mut score_fn: F,
) -> Option<Vec2>
where
    F: FnMut(Vec2, IVec2) -> f32,
{
    let mut best_target: Option<Vec2> = None;
    let mut best_score = f32::MAX;
    let (map_w, map_h) = world_map.pixel_bounds();

    for _ in 0..samples {
        let test_pos = Vec2::new(rng.random_range(0.0..map_w), rng.random_range(0.0..map_h));
        if !world_map.is_walkable(test_pos) {
            continue;
        }
        let chunk = world_pos_to_chunk(test_pos);
        let mut score = score_fn(test_pos, chunk);
        score += current_pos.distance(test_pos) / 5000.0;
        if score < best_score {
            best_score = score;
            best_target = Some(test_pos);
        }
    }
    best_target
}
