//! Pickup action — lift a single ground item into the agent's inventory.
//!
//! Distinct from `Take` (which empties a container's worth of items) — Pickup
//! is the snappy hauling primitive used in supply chains: walk to a dropped
//! log, pick it up, walk to the camp, deposit. Targets any entity with
//! extractable `ItemSlots` and grabs one Thing.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{
    ActionKind, CompletionContext, TargetCandidate, TargetSource,
};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::mind::knowledge::{Concept, MindGraph};
use crate::constants::actions::pickup::DURATION_TICKS;

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.5),
    ChannelUsage::new(Channel::Locomotion, 0.3),
];

pub static PICKUP_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Pickup,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::EntityAffordance,
    base_cost: 1.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: None,
    complete_log: Some("picked up"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::FromTargetContains,
    plan_validity: PlanValidity::TargetContainsAny,
    gates: &[Gate::TargetEntity(
        crate::agent::events::FailureReason::TargetGone,
    )],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_complete: Some(pickup_on_complete),
        target_consumes: Some(pickup_target_consumes),
        ..Hooks::EMPTY
    },
    recipe: None,
};

fn pickup_target_consumes(target: &TargetCandidate, _mind: &MindGraph) -> Vec<TriplePattern> {
    match target.as_entity() {
        Some(entity) => vec![TriplePattern::entity_contains(entity)],
        None => vec![],
    }
}

/// Take exactly one extractable Thing — the snappy single-item variant of
/// Take's drain-the-container loop.
fn pickup_on_complete(ctx: &mut CompletionContext) {
    let Some(target_inv) = ctx.target_inventory.as_deref_mut() else {
        return;
    };
    let concept: Option<Concept> = target_inv
        .all_items()
        .find(|t| target_inv.can_extract(t.concept))
        .map(|t| t.concept);
    let Some(concept) = concept else { return };
    if let Some(thing) = target_inv.extract_thing(concept) {
        ctx.inventory.add_thing(thing);
    }
}
