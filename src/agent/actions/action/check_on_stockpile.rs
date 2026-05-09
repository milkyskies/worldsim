//! CheckOnStockpile action. Recovery happens passively in
//! `body::food_security::tick_food_security`; this action just pins the
//! agent near a `StorageChest` so the recovery window applies. Same
//! shape as `RestInShelter` for shelter and `WarmUp` for heat.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    SatiationGate, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::{Concept, Predicate};
use crate::constants::actions::check_on_stockpile::COMPLETE_FOOD_SECURITY_FRACTION;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Focus, 0.3)];

pub static CHECK_ON_STOCKPILE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::CheckOnStockpile,
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
    start_log: Some("started checking on stockpile"),
    complete_log: Some("checked on stockpile"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[Pattern::SelfNearConcept(Concept::StorageChest)],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::FoodSecurity,
        value: 100.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[Gate::NearStorageChest],
    satiation: Some(SatiationGate::FoodSecurityValue),
    completion: CompletionPredicate::FoodSecurityAtLeast(COMPLETE_FOOD_SECURITY_FRACTION),
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
