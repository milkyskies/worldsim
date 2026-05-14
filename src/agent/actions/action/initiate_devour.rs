//! InitiateDevour — corpse-targeting trigger proposed by brains to start
//! a [`DevourPlugin`](crate::agent::engagement::devour::DevourPlugin)
//! engagement.
//!
//! A one-tick `Timed` trigger: the planner auto-injects `Walk` toward
//! the corpse to satisfy the proximity precondition, and the
//! DevourPlugin (ordered `.before(tick_actions)`) consumes it on the
//! dispatch tick, installing `EngagedDevour` and running the
//! bite-cooldown loop. Pack feeding emerges naturally: multiple
//! predators independently initiate against the same corpse.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::ChannelSlices;
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, PlanValidity,
    SatiationGate, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::{Concept, Predicate};

pub static INITIATE_DEVOUR_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::InitiateDevour,
    kind: ActionKind::Timed { duration_ticks: 1 },
    target_source: TargetSource::DeadEntityWithTrait(Concept::Carrion),
    base_cost: 1.0,
    primitive: ActionPrimitive::Ingest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Normal,
    intent: Intent::Hunger,
    body_channels: ChannelSlices::NONE,
    posture: None,
    interruptible: true,
    start_log: Some("approaching carcass"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::Hunger,
        value: 0.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::TargetContainsEdible,
    gates: &[Gate::TargetEntity(
        crate::agent::events::FailureReason::NoTarget,
    )],
    satiation: Some(SatiationGate::HungerStomach),
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
