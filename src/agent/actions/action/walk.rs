//! Walk action - move to a specific target.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{BodyChannel, ChannelUsage};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};
use crate::agent::brains::thinking::ActionTemplate;
use crate::agent::mind::knowledge::{Node, Predicate, Triple, Value};
use crate::constants::actions::walk::{ALERTNESS_PER_SEC, ENERGY_PER_SEC, HUNGER_PER_SEC};
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;

pub struct WalkAction;

impl Action for WalkAction {
    fn action_type(&self) -> ActionType {
        ActionType::Walk
    }

    fn name(&self) -> &'static str {
        "Walk"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Movement
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(BodyChannel::Legs, 0.4)];
        CHANNELS
    }

    // Walk effects depend on target - set LocatedAt to destination
    // This is overridden in to_template for specific destinations
    fn plan_effects(&self) -> Vec<Triple> {
        vec![] // No static effects - bound dynamically
    }

    fn target_type(&self) -> crate::agent::actions::registry::TargetType {
        crate::agent::actions::registry::TargetType::Position
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            hunger_per_sec: HUNGER_PER_SEC,
            alertness_per_sec: ALERTNESS_PER_SEC,
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("moving to target")
    }

    /// Custom template generation for walk - includes destination in effects
    fn to_template(
        &self,
        target_entity: Option<Entity>,
        target_position: Option<Vec2>,
    ) -> ActionTemplate {
        let effects = if let Some(pos) = target_position {
            // Convert world position to tile
            let tile = (
                (pos.x / TILE_SIZE).floor() as i32,
                (pos.y / TILE_SIZE).floor() as i32,
            );
            vec![Triple::new(
                Node::Self_,
                Predicate::LocatedAt,
                Value::Tile(tile),
            )]
        } else {
            vec![]
        };

        ActionTemplate {
            name: self.name().to_string(),
            action_type: self.action_type(),
            target_entity,
            target_position,
            preconditions: self.preconditions(),
            effects,
            consumes: vec![],
            base_cost: 0.0,
        }
    }
}
