//! Explore action - open-ended curiosity wandering toward stale chunks.

use crate::agent::actions::ActionType;
use crate::agent::actions::action::search_utils::{sample_walkable_scored, staleness_penalty};
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, LegCompleteContext, LegResult, TargetSource};
use crate::agent::mind::explored_tiles::ExploredTiles;
use crate::world::map::WorldMap;
use bevy::prelude::Vec2;

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Locomotion, 0.4),
    ChannelUsage::new(Channel::Focus, 0.15),
    ChannelUsage::new(Channel::Awareness, 0.2),
];

pub static EXPLORE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Explore,
    kind: ActionKind::Ambient,
    target_source: TargetSource::None,
    base_cost: 3.0,
    primitive: ActionPrimitive::Locomote,
    target_selector: TargetSelector::UnknownArea,
    intensity: IntensityPolicy::Normal,
    intent: Intent::Curiosity,
    body_channels: CHANNELS,
    posture: Some(Posture::Moving),
    interruptible: true,
    start_log: Some("wandering somewhere new"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_leg_complete: Some(explore_on_leg_complete),
        ..Hooks::EMPTY
    },
    recipe: None,
};

fn explore_on_leg_complete(ctx: &mut LegCompleteContext) -> LegResult {
    match pick_explore_target(
        ctx.agent_position,
        ctx.explored,
        ctx.world_map,
        ctx.current_tick,
        ctx.rng,
    ) {
        Some(pos) => LegResult::NextLeg(pos),
        None => LegResult::Complete,
    }
}

/// Score-and-pick a staleness-aware walkable target. Lower is better.
pub fn pick_explore_target(
    current_pos: Vec2,
    explored: &ExploredTiles,
    world_map: &WorldMap,
    current_tick: u64,
    rng: &mut dyn rand::RngCore,
) -> Option<Vec2> {
    sample_walkable_scored(current_pos, world_map, 10, rng, |_pos, chunk| {
        staleness_penalty(explored, chunk, current_tick)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let mut explored = ExploredTiles::default();
        explored.mark_explored((0, 0), 100);

        let current_pos = Vec2::new(8.0, 8.0);
        let current_tick = 100;

        let mut avoided_fresh_chunk = 0;
        for seed in 0..50u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let Some(target) =
                pick_explore_target(current_pos, &explored, &map, current_tick, &mut rng)
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
