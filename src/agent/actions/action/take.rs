//! Take action — transfer items from a target entity's slots into the
//! agent's own slots.
//!
//! Polymorphic across chests, dropped piles, furnace outputs, and any other
//! entity with `ItemSlots`. Construction sites are explicitly NOT extractable;
//! their slots have `extract_access: None` and Take silently skips them.

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
use crate::constants::actions::take::DURATION_TICKS;

const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Manipulation, 0.4)];

pub static TAKE_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Take,
    name: "Take",
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::EntityAffordance,
    base_cost: 2.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: None,
    complete_log: Some("took"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    plan_effects: &[],
    plan_consumes: &[],
    target_effects: TargetEffects::FromTargetContains,
    plan_validity: PlanValidity::TargetContainsAny,
    gates: &[Gate::TargetEntityExists],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_complete: Some(take_on_complete),
        target_consumes: Some(take_target_consumes),
        ..Hooks::EMPTY
    },
    recipe: None,
};

fn take_target_consumes(target: &TargetCandidate, _mind: &MindGraph) -> Vec<TriplePattern> {
    match target.as_entity() {
        Some(entity) => vec![TriplePattern::entity_contains(entity)],
        None => vec![],
    }
}

fn take_on_complete(ctx: &mut CompletionContext) {
    let Some(target_inv) = ctx.target_inventory.as_deref_mut() else {
        return;
    };
    let extractable: Vec<Concept> = target_inv
        .all_items()
        .filter(|t| target_inv.can_extract(t.concept))
        .map(|t| t.concept)
        .collect();
    let Some(&concept) = extractable.first() else {
        return;
    };
    while let Some(thing) = target_inv.extract_thing(concept) {
        ctx.inventory.add_thing(thing);
    }
}
