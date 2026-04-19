//! Action system - unified declarative actions.
//!
//! Each action is declarative data: kind, duration, effects, body channels.
//! Multiple actions run in parallel via [`channel::Channel`] capability slots.

pub mod action;
pub mod channel;
pub mod definition;
pub mod generic_action;
pub mod motor;
pub mod registry;
pub mod types;

pub use channel::{Channel, ChannelCapacities, ChannelLoad, ChannelUsage};
pub use definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    Recipe, RuntimeOp, SatiationGate, TargetEffects,
};
pub use generic_action::GenericAction;
pub use motor::{ActionPrimitive, Behavior, IntensityPolicy, Intent, PsychEffect, TargetSelector};
pub use registry::{
    Action, ActionContext, ActionKind, ActionRegistry, ActionState, ActiveActions, RuntimeEffects,
    TargetCandidate, TargetSource,
};
pub use types::*;
