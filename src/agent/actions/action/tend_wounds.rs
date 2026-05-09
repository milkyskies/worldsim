//! Tend Wounds action — first-aid stance toward a nearby injured agent.
//!
//! v1 ships the action's gating and posture. Mutating the target's `Body`
//! to accelerate healing requires a separate query-conflict-safe pathway
//! (ParamSet or post-completion HealRequest queue) and is filed as a
//! follow-up — same staging strategy that #325 used for Cook before the
//! palatability hooks landed.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::constants::actions::tend_wounds::DURATION_TICKS;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Manipulation, 0.7)];

pub static TEND_WOUNDS_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::TendWounds,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::EntityAffordance,
    base_cost: 2.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Social,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("tending wounds"),
    complete_log: Some("tended wounds"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[
        Gate::TargetEntity(crate::agent::events::FailureReason::NoTarget),
        Gate::TargetIsInjured,
    ],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
