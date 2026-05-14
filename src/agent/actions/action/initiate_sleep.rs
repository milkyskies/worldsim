//! InitiateSleep — sleepiness-driven marker proposed by the survival
//! brain to start the
//! [`SleepPlugin`](crate::agent::engagement::sleep::SleepPlugin) engagement.
//!
//! It carries no target and no channel claim: it is a pure trigger that
//! `process_initiate_sleep` consumes the same tick it is dispatched
//! (the plugin is ordered `.before(tick_actions)`), swapping it for the
//! `Sleep` beat which claims FullBody 1.0. Posture is left unset so the
//! trigger never loses the posture mutex to a Stationary action like
//! Rest — otherwise a tired-and-sleepy agent would Rest forever and
//! never get the chance to fall asleep.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::ChannelSlices;
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Hooks, PlanValidity, SatiationGate,
    TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Predicate;

pub static INITIATE_SLEEP_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::InitiateSleep,
    // Timed{1} rather than Instant/Movement: a target-less Movement
    // auto-completes in tick_actions before the plugin can intercept it,
    // and Instant would too. One tick is enough — the plugin runs
    // `.before(tick_actions)` and converts it the same tick.
    kind: ActionKind::Timed { duration_ticks: 1 },
    target_source: TargetSource::None,
    base_cost: 0.5,
    primitive: ActionPrimitive::Rest,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Normal,
    intent: Intent::Fatigue,
    body_channels: ChannelSlices::NONE,
    posture: None,
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
    // The sleep-spot scorer rides on the *trigger*, not the beat: the
    // action-prep pass swaps InitiateSleep for a Walk toward the
    // best-scoring nearby tile (near a fire, near kin) and only lets
    // InitiateSleep fire once the agent is already standing on it.
    // Emergency override (about to pass out) is encoded inside
    // `score_sleep_spot` returning uniform zeros so prep hysteresis
    // blocks the swap.
    hooks: Hooks {
        location_preference: Some(super::sleep::score_sleep_spot),
        ..Hooks::EMPTY
    },
    recipe: None,
};
