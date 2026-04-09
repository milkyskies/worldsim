//! Wander action - random movement.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{BodyChannel, ChannelUsage};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};
use crate::constants::actions::wander::{
    ALERTNESS_PER_SEC, BASE_COST, ENERGY_PER_SEC, HUNGER_PER_SEC,
};

pub struct WanderAction;

impl Action for WanderAction {
    fn action_type(&self) -> ActionType {
        ActionType::Wander
    }

    fn name(&self) -> &'static str {
        "Wander"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Movement
    }

    fn cost(&self) -> f32 {
        BASE_COST
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(BodyChannel::Legs, 0.4)];
        CHANNELS
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            hunger_per_sec: HUNGER_PER_SEC,
            alertness_per_sec: ALERTNESS_PER_SEC,
        }
    }
}
