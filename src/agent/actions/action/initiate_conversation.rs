//! InitiateConversation action — walk to a partner to start a conversation.
//!
//! Proposed by brains, owned by [`ConversePlugin`](crate::agent::engagement::converse::ConversePlugin).
//! The action itself contains no on-completion logic — it's a Movement marker
//! that walks the agent toward their partner. A dedicated plugin polling
//! system swaps InitiateConversation → Converse on arrival at CONVERSATION_RANGE.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Locomotion, 1.0)];

pub static INITIATE_CONVERSATION_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::InitiateConversation,
    // Movement: walk to the partner. The ConversePlugin intercepts
    // arrival at CONVERSATION_RANGE before the standard arrival check fires.
    kind: ActionKind::Movement,
    // Implicit: proposed by the emotional brain, not enumerated by the
    // rational brain (which skips Implicit sources during target enumeration).
    target_source: TargetSource::Implicit,
    base_cost: 1.0,
    primitive: ActionPrimitive::Locomote,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Normal,
    intent: Intent::Social,
    body_channels: CHANNELS,
    posture: Some(Posture::Moving),
    interruptible: true,
    start_log: Some("approaching to talk"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::Always,
    gates: &[
        Gate::TargetEntity(crate::agent::events::FailureReason::NoTarget),
        Gate::TargetNotEngaged,
    ],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
