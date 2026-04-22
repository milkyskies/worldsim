//! Harvest action — gather resources from targets.
//!
//! Complex target-aware logic lives in hooks: target_preconditions chooses
//! between `(target, Contains, ?)` and type-level `Produces` knowledge;
//! on_complete transfers skill-scaled yields with perishable freshness
//! tracking.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, PlanValidity, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{
    ActionKind, CompletionContext, TargetCandidate, TargetSource,
};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::item_slots::{Thing, perishable_decay_rate};
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::agent::skills::SkillKind;
use crate::constants::actions::harvest::DURATION_TICKS;

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.9),
    ChannelUsage::new(Channel::Focus, 0.1),
];

pub static HARVEST_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Harvest,
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
    complete_log: Some("harvested"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[],
    // Fallback placeholder when target has no known Produces; `FromTargetProduces`
    // target_effects override kicks in when the target is known.
    plan_effects: &[EffectTemplate::SelfContains {
        concept: Concept::Apple,
        quantity: 1,
    }],
    plan_consumes: &[],
    target_effects: TargetEffects::FromTargetProduces,
    plan_validity: PlanValidity::TargetProducesFoodOrResource,
    gates: &[Gate::TargetEntity(
        crate::agent::events::FailureReason::TargetGone,
    )],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_complete: Some(harvest_on_complete),
        target_preconditions: Some(harvest_target_preconditions),
        target_consumes: Some(harvest_target_consumes),
        ..Hooks::EMPTY
    },
    recipe: None,
};

/// Concepts the agent believes `entity` currently holds in positive
/// quantity. Shared by Harvest's precondition and consume hooks so a
/// single `Contains` query drives both.
fn entity_positive_contains(
    entity: bevy::prelude::Entity,
    mind: &MindGraph,
) -> Vec<(Concept, u32)> {
    mind.query(Some(&Node::Entity(entity)), Some(Predicate::Contains), None)
        .iter()
        .filter_map(|t| match &t.object {
            Value::Item(concept, qty) if *qty > 0 => Some((*concept, *qty)),
            _ => None,
        })
        .collect()
}

fn harvest_target_preconditions(target: &TargetCandidate, mind: &MindGraph) -> Vec<TriplePattern> {
    let Some(entity) = target.as_entity() else {
        return vec![];
    };

    let has_contains = !entity_positive_contains(entity, mind).is_empty();
    if has_contains {
        return vec![TriplePattern::entity_contains(entity)];
    }

    // No Contains belief but we may know the type's Produces (e.g. BerryBush
    // from cultural knowledge). Trust that and skip the Contains requirement
    // so the planner can still chain Walk → Harvest → Eat.
    let type_produces = mind
        .query(Some(&Node::Entity(entity)), Some(Predicate::IsA), None)
        .iter()
        .any(|t| {
            if let Value::Concept(concept) = t.object {
                !mind
                    .query(
                        Some(&Node::Concept(concept)),
                        Some(Predicate::Produces),
                        None,
                    )
                    .is_empty()
            } else {
                false
            }
        });
    if type_produces {
        return vec![];
    }

    // Fallback: require Contains (may block the plan if decayed).
    vec![TriplePattern::entity_contains(entity)]
}

fn harvest_target_consumes(target: &TargetCandidate, mind: &MindGraph) -> Vec<TriplePattern> {
    let Some(entity) = target.as_entity() else {
        return vec![];
    };
    // One unit per Harvest call, keyed by concept so the planner's
    // quantity-aware consume tracking can chain multiple Harvests on the
    // same target.
    entity_positive_contains(entity, mind)
        .into_iter()
        .map(|(concept, _)| {
            TriplePattern::new(
                Some(Node::Entity(entity)),
                Some(Predicate::Contains),
                Some(Value::Item(concept, 1)),
            )
        })
        .collect()
}

fn harvest_on_complete(ctx: &mut CompletionContext) {
    // Transfer items from target's inventory to agent's inventory.
    // Perishable items get freshness = 1.0 and created_at stamped at harvest
    // time. Skilled harvesters pull more per action — 1 at skill 0.0, 2 by
    // ~0.5, 3 at 1.0 — bounded by what the target actually has.
    let Some(target_inv) = &mut ctx.target_inventory else {
        return;
    };
    let Some(concept) = target_inv.all_items().next().map(|t| t.concept) else {
        return;
    };
    let skill_level = ctx
        .skills
        .map(|s| s.level(SkillKind::Harvesting))
        .unwrap_or(0.0);
    let desired = 1 + (skill_level * 2.0).floor() as u32;
    let available = target_inv.count(concept);
    let actual = desired.min(available);

    for _ in 0..actual {
        if !target_inv.remove(concept, 1) {
            break;
        }
        let thing = if perishable_decay_rate(concept).is_some() {
            Thing::fresh(concept, ctx.tick)
        } else {
            Thing::new(concept)
        };
        ctx.inventory.add_thing(thing);
    }
}
