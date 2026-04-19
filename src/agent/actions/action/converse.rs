//! Converse action — body-channel marker for being in a conversation.
//!
//! Inserted and removed by [`CommunicationPlugin`](crate::agent::communication::CommunicationPlugin);
//! not proposed by brains. Exists so the conversation occupies the
//! `Vocalization` channel like any other body activity — standard preemption
//! rules end conversations when Sleep / Flee / Fight take over the body.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Vocalization, 0.6),
    ChannelUsage::new(Channel::Focus, 0.6),
    ChannelUsage::new(Channel::Awareness, 0.3),
];

pub static CONVERSE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Converse,
    // Indefinite — only the CommunicationPlugin removes it (or preemption).
    kind: ActionKind::Timed {
        duration_ticks: u32::MAX,
    },
    target_source: TargetSource::None,
    base_cost: 1.0,
    primitive: ActionPrimitive::Vocalize,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Social,
    body_channels: CHANNELS,
    // Posture-agnostic: humans talk mid-walk, deer call while grazing.
    posture: None,
    interruptible: true,
    start_log: Some("started talking"),
    complete_log: Some("stopped talking"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
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
