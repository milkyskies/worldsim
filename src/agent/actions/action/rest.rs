//! Rest action — sit-and-recover. Milder than Sleep, gentler than Idle.
//!
//! Rest is the behaviour a mildly tired agent chooses when they need some
//! stamina back but don't want to lose consciousness. Compared to Sleep:
//!   - Rest keeps alertness roughly flat (actually gains slightly), so
//!     the agent stays reactive to threats and conversation.
//!   - Rest recovers stamina slower than Sleep but faster than Idle.
//! Compared to Idle:
//!   - Idle is the true "standing still" default; Rest is active recovery.
//!
//! Mapping: Survival brain proposes Rest for mild Stamina urgency where
//! Sleep would be overkill (#386).

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{ChannelSlices, ChannelUsage};
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
        // Rest is a stationary posture — legs planted, recovering.
        // Saturating Locomotion mutexes Rest against every Movement
        // action via the normal channel arbitration, which is what
        // prevents the "resting + patrolling" nonsense from #386.
        // Cognition and Vocalization stay free so a resting agent
        // can still watch the world or hold a conversation.
        ChannelSlices::STATIONARY
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            stamina_per_sec: 8.0,        // slower than Sleep (+20), faster than Idle (0)
            alertness_per_sec: 2.0,      // gentle alertness gain, no crash
            glucose_drain_per_sec: 0.15, // mild metabolic cost — less than Idle
            // Mild curiosity drift — less than Idle (the agent is
            // partly focused on recovering) but non-zero.
            curiosity_per_sec: 0.008,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("sitting to rest")
    }
}
