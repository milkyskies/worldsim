//! Action system - unified declarative actions.
//!
//! Each action is declarative data: kind, duration, effects, body channels.
//! Multiple actions run in parallel via [`channel::BodyChannel`] resources.

pub mod action;
pub mod channel;
pub mod registry;
pub mod types;

pub use channel::{BodyChannel, ChannelLoad, ChannelUsage, ConflictKind};
pub use registry::{
    Action, ActionContext, ActionKind, ActionRegistry, ActionState, ActiveActions, RuntimeEffects,
    TargetType,
};
pub use types::*;
