//! InitiateHarvest — source-targeting trigger proposed by brains to
//! start a
//! [`HarvestPlugin`](crate::agent::engagement::harvest::HarvestPlugin)
//! engagement.
//!
//! A one-tick `Timed` trigger: the planner auto-injects `Walk` toward
//! the source, and the HarvestPlugin (ordered `.before(tick_actions)`)
//! consumes it on the dispatch tick, installing `EngagedHarvest` and
//! emitting per-yield beats over time until the source is depleted or
//! the agent's inventory is full.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::ChannelSlices;
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;

pub static INITIATE_HARVEST_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::InitiateHarvest,
    kind: ActionKind::Timed { duration_ticks: 1 },
    target_source: TargetSource::EntityAffordance,
    base_cost: 2.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Normal,
    intent: Intent::Goal,
    body_channels: ChannelSlices::NONE,
    posture: None,
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
