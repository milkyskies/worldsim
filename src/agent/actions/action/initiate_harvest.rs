//! InitiateHarvest — walk-to-source marker proposed by brains to start a
//! [`HarvestPlugin`](crate::agent::engagement::harvest::HarvestPlugin) engagement.
//!
//! On arrival the plugin installs `EngagedHarvest` and emits per-yield
//! beats over time, terminating when the source is depleted or the
//! agent's inventory is full.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Locomotion, 1.0)];

pub static INITIATE_HARVEST_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::InitiateHarvest,
    kind: ActionKind::Movement,
    target_source: TargetSource::EntityAffordance,
    base_cost: 2.0,
    primitive: ActionPrimitive::Locomote,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Normal,
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Moving),
    interruptible: true,
    start_log: Some("approaching to harvest"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[EffectTemplate::SelfContains {
        concept: Concept::Apple,
        quantity: 1,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::FromTargetProduces,
    plan_validity: PlanValidity::TargetProducesFoodOrResource,
    gates: &[Gate::TargetEntity(
        crate::agent::events::FailureReason::TargetGone,
    )],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
