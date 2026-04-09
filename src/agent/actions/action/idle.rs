//! Idle action - the default "do nothing" state.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::ChannelUsage;
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

    fn body_channels(&self) -> Vec<ChannelUsage> {
        // Idle reserves no body channels - it can coexist with anything.
        Vec::new()
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            hunger_per_sec: 0.5,
            alertness_per_sec: 5.0,
            ..Default::default()
        }
    }
}
