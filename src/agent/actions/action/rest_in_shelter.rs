//! RestInShelter action — stance of resting inside a shelter.
//!
//! Mirrors `WarmUp`: the action itself does not restore rest-quality —
//! recovery happens passively in `agent::body::rest_quality::tick_rest_quality`
//! whenever a sleeping agent is within range of a `ShelterProvider`. The
//! action just keeps the agent in place near a shelter so the recovery
//! window has time to apply.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    SatiationGate, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::{Concept, Predicate};
use crate::constants::actions::rest_in_shelter::COMPLETE_REST_QUALITY_FRACTION;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Focus, 0.3)];

pub static REST_IN_SHELTER_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::RestInShelter,
    kind: ActionKind::Timed {
        duration_ticks: u32::MAX,
    },
    target_source: TargetSource::None,
    base_cost: 1.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("started resting in shelter"),
    complete_log: Some("rested in shelter"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[Pattern::SelfNearConcept(Concept::LeanTo)],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::RestQuality,
        value: 100.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[Gate::NearShelterProvider],
    satiation: Some(SatiationGate::RestQualityValue),
    completion: CompletionPredicate::RestQualityAtLeast(COMPLETE_REST_QUALITY_FRACTION),
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
