//! Sleep actions - sleeping and waking up.

use crate::agent::actions::ActionType;
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};
use crate::agent::mind::knowledge::{Node, Predicate, Triple, Value};

pub struct SleepAction;

impl Action for SleepAction {
    fn action_type(&self) -> ActionType {
        ActionType::Sleep
    }

    fn name(&self) -> &'static str {
        "Sleep"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: u32::MAX,
        }
    }

    // Planning: Sleep leads to full energy
    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(Node::Self_, Predicate::Energy, Value::Int(100))]
    }

    fn cost(&self) -> f32 {
        0.1 // Sleeping is low cost when tired
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: 20.0,
            hunger_per_sec: 0.2,
            alertness_per_sec: -50.0,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("falling asleep")
    }
}
