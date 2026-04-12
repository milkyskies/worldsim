//! Flee action - run away from threats.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
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
        1.0
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Locomotion, 1.0),
            ChannelUsage::new(Channel::FullBody, 0.5),
        ];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Moving)
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            alertness_per_sec: 20.0,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("fleeing!")
    }
}
