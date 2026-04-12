//! Drink action — drink water from a known drinkable tile.
//!
//! Declares `TargetSource::TileWithTrait(Drinkable)` so the rational brain
//! enumerates one Drink target per known water tile (asserted by water
//! perception as `Tile(?) HasTrait Drinkable`). The default
//! `to_template_for_target` default implementation auto-injects a `self_at(tile)` precondition,
//! and the regressive planner chains `Walk → Drink` via the implicit walk
//! generator. No manual preconditions or `to_template` override needed.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{ActionPrimitive, Behavior, IntensityPolicy, TargetSelector};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, TargetSource,
};
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, Node, Predicate, Triple, Value};
use crate::constants::actions::drink::{DURATION_TICKS, STAMINA_GAIN, THIRST_REDUCTION};
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

    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            ActionPrimitive::Ingest,
            TargetSelector::InPlace,
            IntensityPolicy::Fixed(0.0),
            crate::agent::actions::motor::Intent::Thirst,
        )
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: DURATION_TICKS,
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Consumption only — drinking uses the mouth/jaws directly. Animals
        // without hands can lap from water surfaces the same as humans.
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Consumption, 0.8)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Head-down at the waterline — stationary until done.
        Some(Posture::Stationary)
    }

    fn target_source(&self) -> TargetSource {
        TargetSource::TileWithTrait(Concept::Drinkable)
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
        // THIRST_REDUCTION is the legacy "how much thirst drops" value.
        // Hydration is the inverted satisfaction, so Drink adds that
        // much hydration (clamped to 100).
        ctx.physical.hydration = (ctx.physical.hydration + THIRST_REDUCTION).min(100.0);
        ctx.physical.stamina.adjust_aerobic(STAMINA_GAIN);
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("drank water")
    }
}

#[cfg(test)]
mod tests {
    use super::is_adjacent_to_water;
    use crate::testing::{AgentConfig, TestWorld};
    use crate::world::map::{TILE_SIZE, TileType, WorldMap};
    use bevy::math::{IVec2, Vec2};

    fn grass_map(width: u32, height: u32) -> WorldMap {
        use crate::world::map::{CHUNK_SIZE, Chunk};
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

    fn tile_center(tx: i32, ty: i32) -> Vec2 {
        Vec2::new(
            tx as f32 * TILE_SIZE + TILE_SIZE / 2.0,
            ty as f32 * TILE_SIZE + TILE_SIZE / 2.0,
        )
    }

    #[test]
    fn water_directly_adjacent_is_detected() {
        let mut map = grass_map(16, 16);
        map.set_tile(5, 5, TileType::ShallowWater);
        assert!(is_adjacent_to_water(tile_center(4, 5), &map));
    }

    #[test]
    fn water_diagonally_adjacent_is_detected() {
        let mut map = grass_map(16, 16);
        map.set_tile(5, 5, TileType::Water);
        assert!(is_adjacent_to_water(tile_center(4, 4), &map));
    }

    #[test]
    fn water_two_tiles_away_is_not_detected() {
        let mut map = grass_map(16, 16);
        map.set_tile(7, 5, TileType::ShallowWater);
        assert!(!is_adjacent_to_water(tile_center(4, 5), &map));
    }

    #[test]
    fn agent_at_map_origin_does_not_panic_on_negative_neighbors() {
        let map = grass_map(16, 16);
        assert!(!is_adjacent_to_water(tile_center(0, 0), &map));
    }

    #[test]
    fn no_water_anywhere_returns_false() {
        let map = grass_map(16, 16);
        assert!(!is_adjacent_to_water(tile_center(8, 8), &map));
    }

    /// Regression for #213: a thirsty agent standing next to a water tile
    /// should plan and execute Drink directly. Re-enabled by #219, which
    /// switched Drink to `TargetSource::TileWithTrait(Drinkable)` so the
    /// rational brain enumerates the water tile as a planning target.
    #[test]
    fn thirsty_agent_near_water_drinks() {
        let mut world = TestWorld::with_seed(42);

        // Place water tiles adjacent to where the agent will be.
        // Agent at tile (2, 2) → world pos ~(40, 40). Water at tile (3, 2).
        world.set_tile(3, 2, TileType::ShallowWater);

        let agent = world.spawn_agent(AgentConfig {
            pos: Vec2::new(40.0, 40.0),
            hydration: 10.0,
            ..Default::default()
        });

        world.tick(200);

        assert!(
            world.agent_thirst(agent) < 50.0,
            "Agent should have drunk water and reduced thirst, but thirst is {:.0}",
            world.agent_thirst(agent),
        );
    }

    #[test]
    fn drink_action_is_registered() {
        let world = TestWorld::new();
        assert!(world.has_registered_action(crate::agent::actions::ActionType::Drink));
    }
}
