//! Walk action - move to a specific target.

use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};
use crate::agent::brains::thinking::ActionTemplate;
use crate::agent::mind::knowledge::{Node, Predicate, Triple, Value};
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
            energy_per_sec: -0.3,
            hunger_per_sec: 0.5,
            alertness_per_sec: 10.0,
            ..Default::default()
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
            const TILE_SIZE: f32 = 16.0;
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
            content: Vec::new(),
            preconditions: self.preconditions(),
            effects,
            base_cost: 0.0,
            topic: None,
        }
    }
}
