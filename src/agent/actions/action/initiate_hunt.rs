//! InitiateHunt — walk-to-prey marker proposed by brains to start a
//! [`HuntPlugin`](crate::agent::engagement::hunt::HuntPlugin) engagement.
//!
//! On arrival at strike range the plugin installs `EngagedHunt` and
//! drives the inner pursue/strike loop. Brains never propose `Bite`
//! directly anymore — Hunt owns the strike beat.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Locomotion, 1.0),
    ChannelUsage::new(Channel::Awareness, 0.7),
];

pub static INITIATE_HUNT_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::InitiateHunt,
    kind: ActionKind::Movement,
    target_source: TargetSource::EntityWithTrait(Concept::Prey),
    base_cost: 1.0,
    primitive: ActionPrimitive::Locomote,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Maximal,
    intent: Intent::Hunger,
    body_channels: CHANNELS,
    posture: Some(Posture::Moving),
    interruptible: true,
    start_log: Some("closing on prey"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::FromTargetProduces,
    plan_validity: PlanValidity::TargetProducesFoodOrResource,
    gates: &[Gate::TargetEntity(
        crate::agent::events::FailureReason::NoTarget,
    )],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
