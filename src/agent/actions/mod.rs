//! Action system - unified declarative actions.
//!
//! Each action is declarative data: kind, duration, effects.
//! The execution system handles all logic based on this data.

pub mod action;
pub mod registry;
pub mod types;

pub use registry::{
    Action, ActionContext, ActionKind, ActionRegistry, ActionState, RuntimeEffects, TargetType,
};
pub use types::*;
