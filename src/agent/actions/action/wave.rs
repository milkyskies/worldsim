//! Wave / gesture action — short hand-raise toward a nearby entity.
//!
//! Cross-species communication primitive: humans wave at humans, point at
//! visible threats, gesture toward food sources. The motor produces the
//! gesture; future language layers will attach semantic content.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::constants::actions::wave::DURATION_TICKS;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Manipulation, 0.3)];

pub static WAVE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Wave,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::EntityAffordance,
    base_cost: 0.5,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Social,
    body_channels: CHANNELS,
    posture: None,
    interruptible: true,
    start_log: Some("waved"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[Gate::TargetEntity(
        crate::agent::events::FailureReason::NoTarget,
    )],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
