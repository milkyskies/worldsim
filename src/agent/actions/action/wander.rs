//! Wander action - random movement.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};
use crate::constants::actions::wander::{
    ALERTNESS_PER_SEC, BASE_COST, GLUCOSE_DRAIN_PER_SEC, STAMINA_PER_SEC,
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
            // Wandering is a mild curiosity satisfier — the agent
            // drifts through local space and passively takes in
            // what's around. Weaker than Explore (which actively
            // seeks novelty) and Observe (which focuses on one thing).
            stimulation_per_sec: 0.02,
            // Solo wandering erodes social satisfaction — drifting
            // alone makes you want company.
            companionship_per_sec: -0.006,
            ..Default::default()
        }
    }
}
