//! WarmUp action — stance of sitting by a heat source.
//!
//! Does not itself restore warmth; recovery is the proximity effect in
//! `agent::body::warmth::tick_warmth`. Any action near a heat emitter
//! benefits from the same passive rate.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    RuntimeOp, SatiationGate, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::{Concept, Predicate};
use crate::constants::actions::warm_up::{COMPLETE_WARMTH_FRACTION, STAMINA_GAIN};

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Focus, 0.3)];

pub static WARM_UP_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::WarmUp,
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
    start_log: Some("started warming up"),
    complete_log: Some("warmed up"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[Pattern::SelfNearConcept(Concept::Campfire)],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::Warmth,
        value: 100.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[Gate::NearHeatEmitter],
    satiation: Some(SatiationGate::WarmthValue),
    completion: CompletionPredicate::WarmthAtLeast(COMPLETE_WARMTH_FRACTION),
    on_complete_ops: &[RuntimeOp::AdjustAerobic(STAMINA_GAIN)],
    hooks: Hooks::EMPTY,
    recipe: None,
};
