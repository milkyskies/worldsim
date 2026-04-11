//! Explore action - movement to find resources.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};
use crate::constants::actions::explore::{
    ALERTNESS_PER_SEC, BASE_COST, GLUCOSE_DRAIN_PER_SEC, STAMINA_PER_SEC,
};

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
        BASE_COST
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Locomotion, 0.4)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Moving)
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            stamina_per_sec: STAMINA_PER_SEC,
            glucose_drain_per_sec: GLUCOSE_DRAIN_PER_SEC,
            alertness_per_sec: ALERTNESS_PER_SEC,
            // Exploring is peak novelty-seeking — the agent is actively
            // covering new ground. Strongest curiosity satisfier.
            curiosity_per_sec: -0.05,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("exploring for resources")
    }
}
