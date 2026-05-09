//! Sit action — seated idle stance.
//!
//! Visual differentiator from Idle: agents at rest sit instead of standing,
//! making settlements look lived-in. Mechanically identical to Idle (no
//! channels, indefinite, posture-stationary) — the renderer keys off the
//! action type to pick the seated sprite (#255).

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Locomotion, 0.1)];

pub static SIT_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Sit,
    kind: ActionKind::Timed {
        duration_ticks: u32::MAX,
    },
    target_source: TargetSource::None,
    base_cost: 0.2,
    primitive: ActionPrimitive::Rest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("sat down"),
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
