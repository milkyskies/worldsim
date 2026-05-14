//! InitiateHunt — prey-targeting trigger proposed by brains to start a
//! [`HuntPlugin`](crate::agent::engagement::hunt::HuntPlugin) engagement.
//!
//! It is a one-tick `Timed` trigger, not a Movement: the regressive
//! planner auto-injects a `Walk` toward the prey's tile to satisfy the
//! proximity precondition (`Walk → InitiateHunt`), and the HuntPlugin —
//! ordered `.before(tick_actions)` — consumes it the same tick it
//! dispatches, installing `EngagedHunt` and taking over the inner
//! pursue/strike loop. Brains never propose `Bite` directly; Hunt owns
//! the strike beat.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::ChannelSlices;
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::agent::mind::knowledge::Concept;

pub static INITIATE_HUNT_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::InitiateHunt,
    kind: ActionKind::Timed { duration_ticks: 1 },
    target_source: TargetSource::EntityWithTrait(Concept::Prey),
    base_cost: 1.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Maximal,
    intent: Intent::Hunger,
    body_channels: ChannelSlices::NONE,
    posture: None,
    interruptible: true,
    start_log: Some("closing on prey"),
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
