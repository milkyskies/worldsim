//! Idle action - the default "do nothing" state.

use crate::agent::actions::ActionType;
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

    // Idle reserves no body channels (default `&[]`).

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            glucose_drain_per_sec: 0.1,
            alertness_per_sec: 5.0,
            // Stillness breeds curiosity — an idle agent slowly gets
            // the urge to look around, wander, find something new.
            // Scaled so a fully-satisfied agent takes ~60 seconds of
            // pure Idle to saturate from 0 → 1.
            curiosity_per_sec: 0.015,
            ..Default::default()
        }
    }
}
