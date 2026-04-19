//! Drink action — drink water from a known drinkable tile.
//!
//! `TargetSource::TileWithTrait(Drinkable)` lets the rational brain enumerate
//! one Drink target per known water tile. The default `to_template_for_target`
//! auto-injects a `self_at(tile)` precondition and the regressive planner
//! chains `Walk → Drink` via the implicit walk generator.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, PlanValidity, RuntimeOp,
    SatiationGate, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::{Concept, Predicate};
use crate::constants::actions::drink::{DURATION_TICKS, STAMINA_GAIN, THIRST_REDUCTION};

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Consumption, 0.8)];

pub static DRINK_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Drink,
    name: "Drink",
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::TileWithTrait(Concept::Drinkable),
    base_cost: 1.0,
    primitive: ActionPrimitive::Ingest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Thirst,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: None,
    complete_log: Some("drank water"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::Thirst,
        value: 0.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[Gate::AdjacentToWater],
    satiation: Some(SatiationGate::HydrationValue),
    completion: CompletionPredicate::Never,
    on_complete_ops: &[
        RuntimeOp::TopUpHydration(THIRST_REDUCTION),
        RuntimeOp::AdjustAerobic(STAMINA_GAIN),
    ],
    hooks: Hooks::EMPTY,
    recipe: None,
};

/// Re-export for tests that need the adjacency check directly.
pub fn is_adjacent_to_water(
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
    /// should plan and execute Drink directly.
    #[test]
    fn thirsty_agent_near_water_drinks() {
        let mut world = TestWorld::with_seed(42);
        world.set_tile(3, 2, TileType::ShallowWater);
        let agent = world.spawn_agent(AgentConfig {
            pos: Vec2::new(40.0, 40.0),
            hydration: 0.1,
            ..Default::default()
        });
        world.tick(200);
        assert!(
            world.agent_thirst(agent) < 0.5,
            "Agent should have drunk water and reduced thirst, but thirst is {:.2}",
            world.agent_thirst(agent),
        );
    }

    #[test]
    fn drink_action_is_registered() {
        let world = TestWorld::new();
        assert!(world.has_registered_action(crate::agent::actions::ActionType::Drink));
    }
}
