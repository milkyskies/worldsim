//! Explore action - movement to find resources.

use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};

pub struct ExploreAction;

impl Action for ExploreAction {
    fn action_type(&self) -> ActionType {
        ActionType::Explore
    }

    fn name(&self) -> &'static str {
        "Explore"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Movement
    }

    fn cost(&self) -> f32 {
        3.0 // Lower than wander when goal-seeking
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: -0.25,
            hunger_per_sec: 2.5,
            alertness_per_sec: 5.0,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("exploring for resources")
    }
}
