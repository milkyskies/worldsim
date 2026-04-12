//! InitiateConversation action - walk to a partner to start a conversation.
//!
//! This action is **proposed by brains** (emotional brain when social drive is
//! high, rational brain when a goal needs another agent's help) and **owned by
//! the [`CommunicationPlugin`](crate::agent::communication::CommunicationPlugin)**.
//!
//! The action itself contains no on-completion logic — it is a Movement marker
//! that walks the agent toward their partner while occupying the `Legs` channel.
//! A dedicated polling system in `CommunicationPlugin` watches for agents with
//! this action active, checks proximity to the partner each tick, and on
//! arrival swaps `InitiateConversation` for `Converse` in `ActiveActions`,
//! registers a new `Conversation`, and inserts `InConversation` on both
//! participants. After that the standard turn-taking systems take over.
//!
//! This mirrors the [`ConverseAction`](super::ConverseAction) pattern: the
//! action is just a body-channel marker; the plugin owns the lifecycle.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::registry::{Action, ActionContext, ActionKind, TargetSource};
use crate::agent::body::effort::EffortProfile;
use crate::agent::events::FailureReason;

pub struct InitiateConversationAction;

impl Action for InitiateConversationAction {
    fn action_type(&self) -> ActionType {
        ActionType::InitiateConversation
    }

    fn name(&self) -> &'static str {
        "InitiateConversation"
    }

    fn kind(&self) -> ActionKind {
        // Movement: walk to the partner. The CommunicationPlugin intercepts
        // arrival at CONVERSATION_RANGE (32px) before the standard 2px
        // arrival check fires, so this never auto-completes via the movement
        // system on its own.
        ActionKind::Movement
    }

    /// `Implicit` because this action is *proposed by the emotional brain*,
    /// not enumerated by the rational brain. The rational brain skips
    /// `Implicit` sources during target enumeration, so InitiateConversation
    /// never appears in a rational plan and never gets the auto-injected
    /// proximity precondition (the action does its own walking via the
    /// CommunicationPlugin's polling system).
    fn target_source(&self) -> TargetSource {
        TargetSource::Implicit
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Full Locomotion commitment — walking toward a specific person is
        // a directed locomotion task that should hard-conflict with any
        // other Movement action (Explore, Wander, Walk) so the agent doesn't
        // simultaneously try to wander somewhere else and lose its target.
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Locomotion, 1.0)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Moving)
    }

    fn effort_profile(&self) -> EffortProfile {
        EffortProfile {
            locomotion: 0.5,
            cognition: 0.1,
            ..Default::default()
        }
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if ctx.target_entity.is_none() {
            return Err(FailureReason::NoTarget);
        }
        Ok(())
    }

    fn interruptible(&self) -> bool {
        true
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("approaching to talk")
    }
}
