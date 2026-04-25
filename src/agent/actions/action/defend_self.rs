//! DefendSelf action — retaliatory melee against a non-prey threat.
//!
//! Same combat resolution as Attack (fist Crush damage); enumerates
//! `Dangerous` instead of `Prey` so it can target attackers the agent
//! would never hunt for food.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::defend_self::{BASE_COST, DURATION_TICKS};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.9),
    ChannelUsage::new(Channel::FullBody, 0.7),
    ChannelUsage::new(Channel::Focus, 0.3),
    ChannelUsage::new(Channel::Awareness, 0.5),
];

pub static DEFEND_SELF_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::DefendSelf,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::EntityWithTrait(Concept::Dangerous),
    base_cost: BASE_COST,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Safety,
    body_channels: CHANNELS,
    posture: None,
    interruptible: true,
    start_log: Some("fighting back!"),
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
