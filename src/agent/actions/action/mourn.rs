//! Mourn action — stationary grief processing after a known agent dies.
//!
//! Reads:  agent MindGraph for `(?event, Action, Death)` triples
//! Writes: SimEvent lifecycle; future grief-resolution can stamp the agent's
//!         emotional state on completion
//! Upstream: emotional brain proposing Mourn when a death belief is recent
//! Downstream: comfort-from-others behaviors keyed off active Mourn

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{ChannelSlices, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::constants::actions::mourn::DURATION_TICKS;

pub static MOURN_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Mourn,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::None,
    base_cost: 0.5,
    primitive: ActionPrimitive::Rest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: ChannelSlices::NONE,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("mourning"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[Gate::KnowsRecentDeath],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
