//! Cook action — transform raw food into cooked variants near a heat source.
//!
//! Reads:  agent inventory (raw food), MindGraph (HeatEmitting belief gate)
//! Writes: agent inventory (cooked Thing with freshness), SimEvent::ActionCompleted
//! Upstream: rational brain GOAP planner (chains Cook before Eat for storage / quality)
//! Downstream: eat_on_complete (cooked food yields better macros via `food_macros`)

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Hooks, Pattern, PlanValidity,
    Recipe, TargetEffects,
};
use crate::agent::actions::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use crate::agent::actions::registry::{ActionKind, CompletionContext, TargetSource};
use crate::agent::item_slots::{Thing, perishable_decay_rate};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::cook::{DURATION_TICKS, RAW_REQUIRED};

const CHANNELS: &[ChannelUsage] = &[
    ChannelUsage::new(Channel::Manipulation, 0.6),
    ChannelUsage::new(Channel::Focus, 0.2),
];

const COOKED_MEAT_REQUIREMENTS: &[(Concept, u32)] = &[(Concept::Meat, RAW_REQUIRED)];
const COOKED_MEAT_PROVIDES: &[Concept] = &[Concept::Food, Concept::Edible];

pub static COOK_DEF: ActionDefinition = ActionDefinition {
    action_type: ActionType::Cook,
    kind: ActionKind::Timed {
        duration_ticks: DURATION_TICKS,
    },
    target_source: TargetSource::None,
    base_cost: 3.0,
    primitive: ActionPrimitive::Manipulate,
    target_selector: TargetSelector::InPlace,
    intensity: IntensityPolicy::Fixed(0.0),
    intent: Intent::Goal,
    body_channels: CHANNELS,
    posture: Some(Posture::Stationary),
    interruptible: true,
    start_log: Some("started cooking"),
    complete_log: Some("cooked food"),
    joy_per_sec: 0.0,
    stomach_carbs_per_sec: 0.0,
    preconditions: &[
        Pattern::SelfContains {
            concept: Concept::Meat,
            quantity: RAW_REQUIRED,
        },
        Pattern::SelfNearConcept(Concept::Campfire),
    ],
    plan_effects: &[EffectTemplate::SelfContains {
        concept: Concept::CookedMeat,
        quantity: 1,
    }],
    plan_consumes: &[Pattern::SelfContains {
        concept: Concept::Meat,
        quantity: RAW_REQUIRED,
    }],
    target_effects: TargetEffects::Static,
    plan_validity: PlanValidity::RecipeKnown(Concept::CookedMeat),
    gates: &[
        Gate::InventoryHasQuantity {
            concept: Concept::Meat,
            quantity: RAW_REQUIRED,
        },
        Gate::NearHeatEmitter,
    ],
    satiation: None,
    completion: CompletionPredicate::Never,
    on_complete_ops: &[],
    hooks: Hooks {
        on_complete: Some(cook_on_complete),
        ..Hooks::EMPTY
    },
    recipe: Some(Recipe {
        concept: Concept::CookedMeat,
        requirements: COOKED_MEAT_REQUIREMENTS,
        provides: COOKED_MEAT_PROVIDES,
        build_time_ticks: DURATION_TICKS,
    }),
};

/// Consume one raw cookable from inventory and add the cooked variant with
/// freshness stamped at completion. Mirrors `harvest_on_complete`'s
/// `Thing::fresh` path so cooked items participate in the perishable decay
/// system from tick zero.
fn cook_on_complete(ctx: &mut CompletionContext) {
    let Some((raw, cooked)) = next_cookable(ctx) else {
        return;
    };
    if !ctx.inventory.remove(raw, RAW_REQUIRED) {
        return;
    }
    let thing = if perishable_decay_rate(cooked).is_some() {
        Thing::fresh(cooked, ctx.tick)
    } else {
        Thing::new(cooked)
    };
    ctx.inventory.add_thing(thing);
}

/// First raw concept in inventory that has a known cooked counterpart.
/// Centralised so future raw→cooked mappings (Fish→CookedFish, etc.) extend
/// in one place.
fn next_cookable(ctx: &CompletionContext) -> Option<(Concept, Concept)> {
    ctx.inventory
        .all_items()
        .find_map(|item| cooked_variant(item.concept).map(|cooked| (item.concept, cooked)))
}

/// Static raw→cooked mapping. v1 covers `Meat → CookedMeat`; future raw
/// foods plug in here without touching the planner contract.
pub(crate) fn cooked_variant(raw: Concept) -> Option<Concept> {
    match raw {
        Concept::Meat => Some(Concept::CookedMeat),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::registry::SpawnRequest;
    use crate::agent::body::needs::PhysicalNeeds;
    use crate::agent::item_slots::ItemSlots;
    use crate::agent::mind::knowledge::{MindGraph, setup_ontology};
    use bevy::prelude::Vec2;

    fn make_ctx<'a>(
        physical: &'a mut PhysicalNeeds,
        inventory: &'a mut ItemSlots,
        mind: &'a MindGraph,
        spawn_requests: &'a mut Vec<SpawnRequest>,
    ) -> CompletionContext<'a> {
        CompletionContext {
            physical,
            inventory,
            drives: None,
            mind,
            skills: None,
            target_inventory: None,
            target_entity: None,
            tick: 100,
            agent_position: Vec2::ZERO,
            spawn_requests,
        }
    }

    #[test]
    fn cooked_variant_maps_meat_to_cooked_meat() {
        assert_eq!(cooked_variant(Concept::Meat), Some(Concept::CookedMeat));
    }

    #[test]
    fn cooked_variant_returns_none_for_non_cookable() {
        assert_eq!(cooked_variant(Concept::Wood), None);
        assert_eq!(cooked_variant(Concept::Apple), None);
    }

    #[test]
    fn cook_on_complete_consumes_meat_and_adds_cooked_meat() {
        let mut physical = PhysicalNeeds::default();
        let mut inventory = ItemSlots::agent_carry();
        inventory.add(Concept::Meat, 2);
        let mind = MindGraph::new(setup_ontology());
        let mut spawn_requests = Vec::new();
        let mut ctx = make_ctx(&mut physical, &mut inventory, &mind, &mut spawn_requests);

        cook_on_complete(&mut ctx);

        assert_eq!(inventory.count(Concept::Meat), 1);
        assert_eq!(inventory.count(Concept::CookedMeat), 1);
    }

    #[test]
    fn cook_on_complete_stamps_cooked_thing_with_freshness_at_tick() {
        let mut physical = PhysicalNeeds::default();
        let mut inventory = ItemSlots::agent_carry();
        inventory.add(Concept::Meat, 1);
        let mind = MindGraph::new(setup_ontology());
        let mut spawn_requests = Vec::new();
        let mut ctx = make_ctx(&mut physical, &mut inventory, &mind, &mut spawn_requests);

        cook_on_complete(&mut ctx);

        let cooked = inventory
            .all_items()
            .find(|t| t.concept == Concept::CookedMeat)
            .expect("cooked meat present after Cook completes");
        assert_eq!(cooked.properties.freshness, Some(1.0));
        assert_eq!(cooked.properties.created_at, Some(100));
    }

    #[test]
    fn cook_on_complete_is_noop_without_cookable_input() {
        let mut physical = PhysicalNeeds::default();
        let mut inventory = ItemSlots::agent_carry();
        inventory.add(Concept::Apple, 3);
        let mind = MindGraph::new(setup_ontology());
        let mut spawn_requests = Vec::new();
        let mut ctx = make_ctx(&mut physical, &mut inventory, &mind, &mut spawn_requests);

        cook_on_complete(&mut ctx);

        assert_eq!(inventory.count(Concept::Apple), 3);
        assert_eq!(inventory.count(Concept::CookedMeat), 0);
    }
}
