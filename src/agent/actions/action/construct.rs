//! Construct action — work on a construction site that requires labor.
//!
//! Indefinite timed — runs until the site transforms (target despawns) or
//! the agent is interrupted. The `labor_accumulation_system` queries
//! [`ActiveActions`](crate::agent::actions::ActiveActions) for Construct
//! to tick the `LaborAccumulated` counter each tick; `on_complete` is
//! intentionally a no-op because the site's transform is what ends the
//! action.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, TargetSource};
use crate::constants::actions::construct::BASE_COST;

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.8),
    ChannelUsage::new(Channel::Focus, 0.4),
];

pub static CONSTRUCT_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Construct,
    name: "Construct",
    kind: ActionKind::Timed {
        duration_ticks: u32::MAX,
    },
    target_source: TargetSource::EntityAffordance,
    base_cost: BASE_COST,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("started constructing"),
    complete_log: None,
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    // (Self_, Near, concept) derived from (target, Becomes, concept).
    target_effects: TargetEffects::FromTargetBecomes,
    plan_validity: PlanValidity::TargetHasBecomes,
    gates: &[Gate::TargetEntityExists],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks::EMPTY,
    recipe: None,
};
