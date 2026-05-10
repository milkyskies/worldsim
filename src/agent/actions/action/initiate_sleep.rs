//! InitiateSleep — sleepiness-driven marker proposed by the survival
//! brain. Walk-to-spot + start the
//! [`SleepPlugin`](crate::agent::engagement::sleep::SleepPlugin) engagement.
//!
//! On entry the plugin installs `EngagedSleep`, claims FullBody via the
//! `Sleep` beat. The location-preference scoring still runs through the
//! preserved `Sleep` action's `score_sleep_spot` hook.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Hooks, PlanValidity, SatiationGate,
    TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Predicate;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Locomotion, 1.0)];

pub static INITIATE_SLEEP_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::InitiateSleep,
    kind: ActionKind::Movement,
    target_source: TargetSource::None,
    base_cost: 0.5,
    primitive: ActionPrimitive::Locomote,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Normal,
    intent: Intent::Fatigue,
    body_channels: CHANNELS,
    posture: Some(Posture::Moving),
    interruptible: true,
    start_log: Some("settling down to sleep"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[EffectTemplate::SelfNeedExact {
        predicate: Predicate::Wakefulness,
        value: 100.0,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[],
    satiation: Some(SatiationGate::WakefulnessValue),
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
