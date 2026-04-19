//! Bite action — jaws-as-weapon variant of Attack for species with `Channel::Bite`.
//!
//! Shares planning semantics (Prey enumeration, `FromTargetProduces` yield
//! projection, `TargetProducesFoodOrResource` validity) with Attack via the
//! declarative definition. Damage, hit resolution, and death live in
//! `biology::combat`.

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
    ChannelUsage::new(Channel::Bite, 1.0),
    ChannelUsage::new(Channel::FullBody, 0.7),
    ChannelUsage::new(Channel::Focus, 0.3),
    ChannelUsage::new(Channel::Awareness, 0.5),
];

pub static BITE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Bite,
    name: "Bite",
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
    // Posture-agnostic: a charging wolf biting its prey is canonical.
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
    gates: &[Gate::TargetEntityRequired],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
