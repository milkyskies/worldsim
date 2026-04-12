//! Observe action — stand still and watch a visible target.
//!
//! Observe is the curious-but-stationary counterpart to Explore. An
//! Explore walks somewhere new; an Observe stands where it is and
//! studies something already in view. Good for wolves watching deer
//! from cover, deer tracking a wolf's approach, humans sizing up a
//! stranger they haven't greeted yet.
//!
//! Mapping: Emotional brain proposes Observe for Fun/Boredom urgency
//! when there's a visible entity — curiosity satisfied by watching,
//! not by moving. Falls through to Explore when there's nothing
//! interesting in view.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};

pub struct ObserveAction;

impl Action for ObserveAction {
    fn action_type(&self) -> ActionType {
        ActionType::Observe
    }

    fn name(&self) -> &'static str {
        "Observe"
    }

    fn kind(&self) -> ActionKind {
        // ~2 seconds of sim time (60 ticks/sec). A real "watching
        // glance" is bounded — staring at one thing forever isn't
        // curiosity, it's a bug. After the window the agent naturally
        // moves on and re-evaluates: another novel thing, conversation,
        // exploration, whatever wins arbitration next tick.
        ActionKind::Timed {
            duration_ticks: 120,
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Observing uses cognitive bandwidth — rules out reading,
        // planning-heavy Explore, etc. in parallel. Does not block
        // Locomotion so the agent could in principle walk-and-watch
        // later if a brain pairs them up.
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Cognition, 0.6)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Watching works from a standstill or mid-walk — both are real.
        None
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            alertness_per_sec: 3.0, // attending keeps the agent sharp
            // Strong curiosity drain: watching something novel is the
            // clearest way to satisfy the drive. Scales so a ~2s Observe
            // window (120 ticks) erases ~0.16 of the drive — enough to
            // cross below the `min_threshold: 0.05` gate if curiosity
            // was mild to start with.
            stimulation_per_sec: 0.08,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("watching")
    }
}
