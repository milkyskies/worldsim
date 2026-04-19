//! Flee action - run away from threats.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Locomotion, 1.0),
    ChannelUsage::new(Channel::FullBody, 0.5),
    ChannelUsage::new(Channel::Awareness, 0.7),
];

pub static FLEE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Flee,
    name: "Flee",
    kind: ActionKind::Movement,
    target_source: TargetSource::None,
    base_cost: 1.0,
    primitive: ActionPrimitive::Locomote,
    target_selector: TargetSelector::ThreatAvoidant,
    intensity: IntensityPolicy::Maximal,
    intent: Intent::Safety,
    body_channels: CHANNELS,
    posture: Some(Posture::Moving),
    interruptible: true,
    start_log: Some("fleeing!"),
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
