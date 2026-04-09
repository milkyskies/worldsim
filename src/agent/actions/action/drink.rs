//! Drink action - drink water from adjacent water tiles.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{BodyChannel, ChannelUsage};
use crate::agent::actions::registry::{Action, ActionContext, ActionKind, CompletionContext};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Node, Predicate, Triple, Value};
use crate::constants::actions::drink::{DURATION_TICKS, ENERGY_GAIN, THIRST_REDUCTION};
pub struct DrinkAction;

/// Check if any tile adjacent to the given position is a water source.
fn is_adjacent_to_water(
    agent_pos: bevy::math::Vec2,
    world_map: &crate::world::map::WorldMap,
) -> bool {
    let tile_size = crate::world::map::TILE_SIZE;
    let tx = (agent_pos.x / tile_size).floor() as i32;
    let ty = (agent_pos.y / tile_size).floor() as i32;

    for dx in -1..=1 {
        for dy in -1..=1 {
            let nx = tx + dx;
            let ny = ty + dy;
            if nx >= 0
                && ny >= 0
                && let Some(tile) = world_map.get_tile(nx as u32, ny as u32)
                && tile.is_water()
            {
                return true;
            }
        }
    }
    false
}

impl Action for DrinkAction {
    fn action_type(&self) -> ActionType {
        ActionType::Drink
    }

    fn name(&self) -> &'static str {
        "Drink"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: DURATION_TICKS,
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(BodyChannel::Hands, 0.3),
            ChannelUsage::new(BodyChannel::Mouth, 0.8),
        ];
        CHANNELS
    }

    fn preconditions(&self) -> Vec<TriplePattern> {
        vec![]
    }

    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(Node::Self_, Predicate::Thirst, Value::Int(0))]
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if is_adjacent_to_water(ctx.agent_position, ctx.world_map) {
            Ok(())
        } else {
            Err(FailureReason::NoWaterNearby)
        }
    }

    fn on_complete(&self, ctx: &mut CompletionContext) {
        ctx.physical.thirst = (ctx.physical.thirst - THIRST_REDUCTION).max(0.0);
        ctx.physical.energy = (ctx.physical.energy + ENERGY_GAIN).min(100.0);
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("drank water")
    }
}
