//! WakeUp action — 30-tick transition out of Sleep.
//!
//! Only FullBody is claimed; Focus is NOT claimed because cognitive channels
//! scale to zero during sleep and requiring Focus would make WakeUp
//! inadmissible — the agent needs to wake up to regain focus, not the other
//! way around.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::FullBody, 0.4)];

pub static WAKE_UP_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::WakeUp,
    kind: ActionKind::Timed { duration_ticks: 30 },
    target_source: TargetSource::None,
    base_cost: 1.0,
    primitive: ActionPrimitive::Rest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.3),
    intent: Intent::Fatigue,
    body_channels: CHANNELS,
    // Stationary: you don't wake up mid-walk.
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: None,
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[EffectTemplate::SelfHasTrait(Concept::Awake)],
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
