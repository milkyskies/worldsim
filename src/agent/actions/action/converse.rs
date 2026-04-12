//! Converse action - the body-channel marker for being in a conversation.
//!
//! This action is **not proposed by brains**. It is inserted into
//! [`ActiveActions`](crate::agent::actions::ActiveActions) by the
//! [`CommunicationPlugin`](crate::agent::communication::CommunicationPlugin)
//! when an agent enters a conversation, and removed when they leave. It exists
//! solely so the conversation occupies the `Vocalization` channel like any
//! other body-using activity, which lets the standard preemption rules end
//! conversations naturally when something more urgent (Sleep, Flee) takes
//! over the body.
//!
//! Because it has no completion duration and no on-complete logic, ticking it
//! is a no-op — the action just sits in the slot until either the
//! [`CommunicationPlugin`] removes it (graceful end) or
//! [`start_actions`](crate::agent::nervous_system::execution::start_actions)
//! preempts it (interruptive end).

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects};

pub struct ConverseAction;

impl Action for ConverseAction {
    fn action_type(&self) -> ActionType {
        ActionType::Converse
    }

    fn name(&self) -> &'static str {
        "Converse"
    }

    fn kind(&self) -> ActionKind {
        // Indefinite — only the CommunicationPlugin removes it (or preemption).
        ActionKind::Timed {
            duration_ticks: u32::MAX,
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Vocalization, 0.6)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Posture-agnostic: humans talk mid-walk, deer call to each
        // other while grazing, a standing group can chat for hours.
        None
    }

    fn interruptible(&self) -> bool {
        // Sleep / Flee / Fight should be able to preempt a conversation.
        true
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            // Being in a conversation drains both social (the whole
            // point) and curiosity (chatting is a form of novelty
            // exchange). Lets an active conversation satisfy both
            // drives, which is why real social time leaves someone
            // feeling "filled up" on both fronts.
            companionship_per_sec: 0.04,
            stimulation_per_sec: 0.02,
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("started talking")
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("stopped talking")
    }
}
