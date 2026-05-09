//! Dance action — group celebration when mood and companionship are high.
//!
//! Reads:  agent EmotionalState.current_mood, PsychologicalDrives.companionship
//! Writes: SimEvent lifecycle; emotional contagion is left to a downstream
//!         system that keys off active Dance.
//! Upstream: rational/emotional brain proposing Dance when conditions allow
//! Downstream: future joy-contagion field that radiates from dancing agents

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::constants::actions::dance::{DURATION_TICKS, MIN_COMPANIONSHIP, MIN_MOOD};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Locomotion, 0.6),
    ChannelUsage::new(Channel::Manipulation, 0.3),
];

pub static DANCE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Dance,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::None,
    base_cost: 2.0,
    primitive: ActionPrimitive::Locomote,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.4),
    intent: Intent::Social,
    body_channels: CHANNELS,
    // No Stationary posture — dance moves through space.
    posture: None,
    interruptible: true,
    start_log: Some("started dancing"),
    complete_log: Some("danced"),
    joy_per_sec: 8.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[
        Gate::MoodAtLeast(MIN_MOOD),
        Gate::CompanionshipAtLeast(MIN_COMPANIONSHIP),
    ],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
