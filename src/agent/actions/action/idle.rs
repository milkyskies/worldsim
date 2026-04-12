//! Idle action - the default "do nothing" state.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{ChannelSlices, ChannelUsage, Posture};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};

pub struct IdleAction;

impl Action for IdleAction {
    fn action_type(&self) -> ActionType {
        ActionType::Idle
    }

    fn name(&self) -> &'static str {
        "Idle"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: u32::MAX,
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Idle claims no body part — the stationary stance is expressed
        // through `posture()` rather than a Locomotion marker.
        ChannelSlices::NONE
    }

    fn posture(&self) -> Option<Posture> {
        // Legs planted, body stationary — the canonical idle stance.
        // Mutexes against Walk/Wander/Flee at the posture gate so the
        // "idle while patrolling" bug class is impossible by construction.
        Some(Posture::Stationary)
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            ..Default::default()
        }
    }
}
