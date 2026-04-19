//! Rest action — sit-and-recover. Milder than Sleep, gentler than Idle.
//!
//! Rest keeps alertness roughly flat, stays reactive to threats, and
//! recovers stamina slower than Sleep but faster than Idle. The Survival
//! brain proposes Rest for mild Stamina urgency where Sleep would be overkill.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{ChannelSlices, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Hooks, PlanValidity, SatiationGate,
    TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Predicate;
use crate::constants::actions::rest::COMPLETE_AEROBIC_FRACTION;

pub static REST_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Rest,
    // Indefinite: the brain replaces Rest with something else once stamina
    // recovers or a stronger drive takes priority.
    kind: ActionKind::Timed {
        duration_ticks: u32::MAX,
    },
    target_source: TargetSource::None,
    base_cost: 0.2,
    primitive: ActionPrimitive::Rest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.4),
    intent: Intent::Fatigue,
    // Legs planted. Focus/Awareness/Vocalization stay free so the resting
    // agent can still watch the world or hold a conversation.
    body_channels: ChannelSlices::NONE,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("sitting to rest"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::Stamina,
        value: 100.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[],
    satiation: Some(SatiationGate::StaminaAerobic),
    completion: CompletionPredicate::AerobicAtLeast(COMPLETE_AEROBIC_FRACTION),
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
