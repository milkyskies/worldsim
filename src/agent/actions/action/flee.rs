//! Flee action - run away from threats.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};
use crate::constants::actions::flee::{
    ALERTNESS_PER_SEC, BASE_COST, ENERGY_PER_SEC, HUNGER_PER_SEC,
};

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
        BASE_COST
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Locomotion, 1.0),
            ChannelUsage::new(Channel::FullBody, 0.5),
        ];
        CHANNELS
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            hunger_per_sec: HUNGER_PER_SEC,
            alertness_per_sec: ALERTNESS_PER_SEC,
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("fleeing!")
    }
}
