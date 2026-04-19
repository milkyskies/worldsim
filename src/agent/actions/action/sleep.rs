//! Sleep action — indefinite unconsciousness that recovers wakefulness.
//!
//! Sleep claims only FullBody; blocking every other action is enforced by
//! an explicit short-circuit in `start_actions` rather than spreading 1.0
//! across every channel, which would refuse Sleep on any species whose
//! per-channel capacity doesn't match the human default.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Hooks, PlanValidity, SatiationGate,
    TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Predicate;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::FullBody, 1.0)];

pub static SLEEP_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Sleep,
    kind: ActionKind::Timed {
        duration_ticks: u32::MAX,
    },
    target_source: TargetSource::None,
    base_cost: 0.1,
    primitive: ActionPrimitive::Rest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(1.0),
    intent: Intent::Fatigue,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    // Interruptible: WakeUp has to preempt via the normal channel-admission
    // path (both touch FullBody) and `interruptible = false` would deadlock
    // that. Casual eviction is blocked at a higher layer by the Sleep
    // short-circuit in `start_actions`.
    interruptible: true,
    start_log: Some("falling asleep"),
    complete_log: None,
    joy_per_sec: 2.0,
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
