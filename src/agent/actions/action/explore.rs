//! Explore action - open-ended curiosity wandering toward stale chunks.
//!
//! Reads: MindGraph (Chunk Explored timestamps), WorldMap, LegCompleteContext
//! Writes: LegResult (next target or complete)
//! Upstream: brains::emotional (proposes on Curiosity urgency), nervous_system::execution (dispatch)
//! Downstream: nervous_system::execution (runs the action, calls on_leg_complete)

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{
    ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
};
use crate::agent::actions::registry::{Action, ActionKind, LegCompleteContext, LegResult};
use crate::agent::mind::knowledge::{MindGraph, Node, Predicate, Value};
use crate::world::map::WorldMap;
use crate::world::spatial_index::world_pos_to_chunk;
use bevy::math::IVec2;
use bevy::prelude::Vec2;
use rand::Rng;

pub struct ExploreAction;

impl Action for ExploreAction {
    fn action_type(&self) -> ActionType {
        ActionType::Explore
    }

    fn name(&self) -> &'static str {
        "Explore"
    }

    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            ActionPrimitive::Locomote,
            TargetSelector::UnknownArea,
            IntensityPolicy::Normal,
            Intent::Curiosity,
        )
    }

    fn kind(&self) -> ActionKind {
        // Ambient: never self-completes; preemption by a higher-urgency
        // plan is the only exit.
        ActionKind::Ambient
    }

    fn cost(&self) -> f32 {
        3.0
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Locomotion, 0.4),
            ChannelUsage::new(Channel::Focus, 0.15),
            ChannelUsage::new(Channel::Awareness, 0.2),
        ];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Moving)
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("wandering somewhere new")
    }

    fn on_leg_complete(&self, ctx: &mut LegCompleteContext) -> LegResult {
        match pick_explore_target(
            ctx.agent_position,
            ctx.mind,
            ctx.world_map,
            ctx.current_tick,
            ctx.rng,
        ) {
            Some(pos) => LegResult::NextLeg(pos),
            None => LegResult::Complete,
        }
    }
}

/// Score-and-pick a staleness-aware walkable target. Lower is better.
pub fn pick_explore_target(
    current_pos: Vec2,
    mind: &MindGraph,
    world_map: &WorldMap,
    current_tick: u64,
    rng: &mut dyn rand::RngCore,
) -> Option<Vec2> {
    sample_walkable_scored(current_pos, world_map, 10, rng, |_pos, chunk| {
        staleness_penalty(mind, chunk, current_tick)
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{Metadata, Triple, setup_ontology};
    use crate::world::map::{CHUNK_SIZE, TileType};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn walkable_map() -> WorldMap {
        let size = CHUNK_SIZE * 4;
        let mut map = WorldMap::new(size, size);
        for cx in 0..4i32 {
            for cy in 0..4i32 {
                map.chunks
                    .entry(IVec2::new(cx, cy))
                    .or_insert_with(|| crate::world::map::Chunk::new(cx, cy));
            }
        }
        for x in 0..size {
            for y in 0..size {
                map.set_tile(x, y, TileType::Grass);
            }
        }
        map
    }

    #[test]
    fn explore_follow_up_leg_uses_staleness_scorer() {
        let map = walkable_map();
        let mut mind = MindGraph::new(setup_ontology());

        mind.assert(Triple::with_meta(
            Node::Chunk((0, 0)),
            Predicate::Explored,
            Value::Boolean(true),
            Metadata::semantic(100),
        ));

        let current_pos = Vec2::new(8.0, 8.0);
        let current_tick = 100;

        let mut avoided_fresh_chunk = 0;
        for seed in 0..50u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let Some(target) =
                pick_explore_target(current_pos, &mind, &map, current_tick, &mut rng)
            else {
                continue;
            };
            if world_pos_to_chunk(target) != IVec2::ZERO {
                avoided_fresh_chunk += 1;
            }
        }

        assert!(
            avoided_fresh_chunk >= 45,
            "staleness-aware picker must steer away from recently-explored chunks; \
             avoided fresh chunk only {avoided_fresh_chunk}/50 runs"
        );
    }
}
