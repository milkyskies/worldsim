//! Action system - unified declarative actions.
//!
//! Each action is declarative data: kind, duration, effects, body channels.
//! Multiple actions run in parallel via [`channel::Channel`] capability slots.

pub mod action;
pub mod channel;
pub mod registry;
pub mod types;

pub use channel::{Channel, ChannelCapacities, ChannelLoad, ChannelUsage};
pub use registry::{
    Action, ActionContext, ActionKind, ActionRegistry, ActionState, ActiveActions, RuntimeEffects,
    TargetCandidate, TargetSource,
};
pub use types::*;
