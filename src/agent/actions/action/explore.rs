//! Explore action - open-ended curiosity wandering toward stale chunks.

use crate::agent::actions::ActionType;
use crate::agent::actions::action::search_utils::{sample_walkable_scored, staleness_penalty};
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{
    ActionPrimitive, Behavior, IntensityPolicy, Intent, TargetSelector,
};
use crate::agent::actions::registry::{Action, ActionKind, LegCompleteContext, LegResult};
use crate::agent::mind::knowledge::MindGraph;
use crate::world::map::WorldMap;
use bevy::prelude::Vec2;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{
        Metadata, MindGraph, Node, Predicate, Triple, Value, setup_ontology,
    };
    use crate::world::map::{CHUNK_SIZE, WorldMap};
    use crate::world::spatial_index::world_pos_to_chunk;
    use bevy::math::IVec2;
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
