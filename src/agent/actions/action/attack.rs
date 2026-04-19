//! Attack action — melee combat against prey.
//!
//! Damage, dodge, death, and meat deposit all live in
//! `biology::combat::resolve_combat_hits`, which consumes the
//! `SimEvent::ActionCompleted` this action emits. The action itself is
//! just the timed marker + target enumeration.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::attack::{BASE_COST, DURATION_TICKS};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.9),
    ChannelUsage::new(Channel::FullBody, 0.7),
    ChannelUsage::new(Channel::Focus, 0.3),
    ChannelUsage::new(Channel::Awareness, 0.5),
];

pub static ATTACK_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Attack,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::EntityWithTrait(Concept::Prey),
    base_cost: BASE_COST,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Safety,
    body_channels: CHANNELS,
    // Posture-agnostic: punch while walking, grapple while charging, strike
    // from a standstill — Attack claims full body via FullBody 0.7 but
    // doesn't pick a posture.
    posture: None,
    interruptible: true,
    start_log: None,
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::FromTargetProduces,
    plan_validity: PlanValidity::TargetProducesFoodOrResource,
    gates: &[Gate::TargetEntity(
        crate::agent::events::FailureReason::NoTarget,
    )],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
