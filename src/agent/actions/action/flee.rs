//! Flee action - run away from threats.

use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};

pub struct FleeAction;

impl Action for FleeAction {
    fn action_type(&self) -> ActionType {
        ActionType::Flee
    }

    fn name(&self) -> &'static str {
        "Flee"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Movement
    }

    fn cost(&self) -> f32 {
        1.0 // Fleeing is urgent
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: -0.5,
            hunger_per_sec: 3.0,
            alertness_per_sec: 20.0,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("fleeing!")
    }
}
