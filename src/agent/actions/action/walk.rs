//! Walk action — move to a specific tile.
//!
//! Walk is `TargetSource::Implicit`: the regressive planner inserts Walk
//! steps directly via `generate_implicit_walk` whenever a `LocatedAt`
//! precondition is unmet, so the rational brain never enumerates Walk
//! targets up front. The planner constructs the `ActionTemplate` itself.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::registry::{Action, ActionKind, RuntimeEffects, TargetSource};

/// Canonical display name for the Walk action. Shared with the planner's
/// implicit-walk template builder so the runtime sees the same name whether
/// the walk was hand-built or planner-generated.
pub const WALK_NAME: &str = "Walk";

pub struct WalkAction;

impl Action for WalkAction {
    fn action_type(&self) -> ActionType {
        ActionType::Walk
    }

    fn name(&self) -> &'static str {
        WALK_NAME
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Movement
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Locomotion, 0.4)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Moving)
    }

    fn target_source(&self) -> TargetSource {
        TargetSource::Implicit
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            ..Default::default()
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("moving to target")
    }
}
