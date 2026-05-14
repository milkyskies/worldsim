//! InitiateFlee — fear-driven marker proposed by survival/emotional
//! brains to start a [`FleePlugin`](crate::agent::engagement::flee::FleePlugin)
//! engagement.
//!
//! Unlike the other Initiate kinds, `InitiateFlee` is itself a Movement
//! action — the agent is already moving away from the threat at the
//! moment of proposal — and the plugin installs `EngagedFlee` on the
//! first tick, then drives the per-tick threat-tracking flee step.

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

pub static INITIATE_FLEE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::InitiateFlee,
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
