//! Sleep actions - sleeping and waking up.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{BodyChannel, ChannelUsage};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};
use crate::agent::mind::knowledge::{Node, Predicate, Triple, Value};
use crate::constants::actions::sleep::{
    ALERTNESS_PER_SEC, BASE_COST, ENERGY_PER_SEC, HUNGER_PER_SEC,
};

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
        BASE_COST
    }

    fn body_channels(&self) -> Vec<ChannelUsage> {
        // Sleep occupies the entire body - hard-conflicts with everything
        // except pure-cognitive actions, which is what clears the slot list.
        vec![ChannelUsage::new(BodyChannel::FullBody, 1.0)]
    }

    fn interruptible(&self) -> bool {
        // Sleep yields only on hard preemption (e.g. extreme fear / starvation).
        false
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            hunger_per_sec: HUNGER_PER_SEC,
            alertness_per_sec: ALERTNESS_PER_SEC,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("falling asleep")
    }
}
