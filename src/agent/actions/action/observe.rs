//! Observe action — stand (or walk) and watch.
//!
//! A bounded 2-second glance. The curious-but-stationary counterpart to
//! Explore: an Observe studies something already in view. After the window
//! the agent re-evaluates against whatever wins arbitration next tick.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Focus, 0.3),
    ChannelUsage::new(Channel::Awareness, 0.6),
];

pub static OBSERVE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Observe,
    name: "Observe",
    // ~2 seconds of sim time (60 ticks/sec). Watching one thing forever
    // isn't curiosity, it's a bug.
    kind: ActionKind::Timed {
        duration_ticks: 120,
    },
    target_source: TargetSource::None,
    base_cost: 1.0,
    primitive: ActionPrimitive::Observe,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Ambient,
    intent: Intent::Curiosity,
    body_channels: CHANNELS,
    // Posture-agnostic: watching works from a standstill or mid-walk.
    posture: None,
    interruptible: true,
    start_log: Some("watching"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
