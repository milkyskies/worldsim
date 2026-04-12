//! Rest action — sit-and-recover. Milder than Sleep, gentler than Idle.
//!
//! Rest is the behaviour a mildly tired agent chooses when they need some
//! stamina back but don't want to lose consciousness.
//!
//! Compared to Sleep:
//! - Rest keeps alertness roughly flat (actually gains slightly), so the
//!   agent stays reactive to threats and conversation.
//! - Rest recovers stamina slower than Sleep but faster than Idle.
//!
//! Compared to Idle:
//! - Idle is the true "standing still" default; Rest is active recovery.
//!
//! Mapping: Survival brain proposes Rest for mild Stamina urgency where
//! Sleep would be overkill (#386).

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{ChannelSlices, ChannelUsage, Posture};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};

pub struct RestAction;

impl Action for RestAction {
    fn action_type(&self) -> ActionType {
        ActionType::Rest
    }

    fn name(&self) -> &'static str {
        "Rest"
    }

    fn kind(&self) -> ActionKind {
        // Indefinite: the brain replaces Rest with something else once
        // stamina recovers or a stronger drive takes priority.
        ActionKind::Timed {
            duration_ticks: u32::MAX,
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Rest claims no body part — the legs-planted stance is expressed
        // through `posture()`. Cognition and Vocalization stay free so a
        // resting agent can still watch the world or hold a conversation.
        ChannelSlices::NONE
    }

    fn posture(&self) -> Option<Posture> {
        // Legs planted, recovering. The posture gate rejects Walk /
        // Wander / Flee admission while Rest is active, which is what
        // prevents the "resting + patrolling" nonsense from #386.
        Some(Posture::Stationary)
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            alertness_per_sec: 2.0,
            stimulation_per_sec: -0.008,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("sitting to rest")
    }
}
