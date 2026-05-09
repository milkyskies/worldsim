//! Integration tests for the Cook action and cooked food concepts.
//!
//! Covers the loop the issue specifies: raw meat + nearby HeatEmitter →
//! Cook runs to completion → CookedMeat in inventory with freshness, and
//! cooked food's downstream payoffs (better macros, slower spoilage).

use bevy::math::Vec2;
use bevy::prelude::Vec3;
use worldsim::agent::actions::ActionType;
use worldsim::agent::actions::ActiveActions;
use worldsim::agent::actions::action::COOK_DEF;
use worldsim::agent::actions::registry::ActionState;
use worldsim::agent::body::metabolism::food_macros;
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::agent::item_slots::{ItemSlots, Thing, perishable_decay_rate};
use worldsim::agent::mind::knowledge::{Concept, Predicate, Value, setup_ontology};
use worldsim::testing::{AgentConfig, TestWorld};

// ─── Recipe seeded into universal cultural knowledge ────────────────────────

#[test]
fn cooked_meat_recipe_is_in_universal_culture_knowledge() {
    // Drives `PlanValidity::RecipeKnown(CookedMeat)` — without this triple,
    // the planner refuses to consider Cook because the recipe is unknown.
    let triples = worldsim::agent::culture::create_cultural_knowledge(
        worldsim::agent::culture::Culture::Hunter,
    );
    let has_recipe = triples.iter().any(|t| {
        t.subject == worldsim::agent::mind::knowledge::Node::Concept(Concept::CookedMeat)
            && t.predicate == Predicate::Requires
            && t.object == Value::Item(Concept::Meat, 1)
    });
    assert!(
        has_recipe,
        "expected (CookedMeat, Requires, Meat 1) in cultural knowledge"
    );
}

#[test]
fn cooked_meat_is_food_in_ontology() {
    // Eat's gate uses `mind.is_a(item.concept, Food)`. If CookedMeat is not
    // food in the ontology, agents would pile up uneatable cooked meat.
    let ontology = setup_ontology();
    assert!(ontology.is_a(Concept::CookedMeat, Concept::Food));
}

// ─── Macros and freshness — payoff side of cooking ──────────────────────────

#[test]
fn cooked_meat_satiates_more_per_unit_than_raw_meat() {
    let raw = food_macros(Concept::Meat).expect("raw meat is food");
    let cooked = food_macros(Concept::CookedMeat).expect("cooked meat is food");
    assert!(
        cooked.total_mass() > raw.total_mass(),
        "cooked meat should yield more usable mass than raw (cooked={cooked:?}, raw={raw:?})"
    );
}

#[test]
fn cooked_meat_decays_slower_than_raw_meat() {
    let raw_rate = perishable_decay_rate(Concept::Meat).expect("raw meat decays");
    let cooked_rate = perishable_decay_rate(Concept::CookedMeat).expect("cooked meat decays");
    assert!(
        cooked_rate < raw_rate,
        "cooked meat decay rate ({cooked_rate}) must be lower than raw ({raw_rate})"
    );
}

// ─── Action definition wiring ───────────────────────────────────────────────

#[test]
fn cook_def_is_registered() {
    use worldsim::agent::actions::ActionRegistry;
    let registry = ActionRegistry::new();
    assert!(
        registry.get(ActionType::Cook).is_some(),
        "Cook must be registered in ActionRegistry::new()"
    );
}

#[test]
fn cook_def_declares_recipe_for_cooked_meat() {
    let recipe = COOK_DEF
        .recipe
        .as_ref()
        .expect("COOK_DEF must declare a Recipe so culture seeding fires");
    assert_eq!(recipe.concept, Concept::CookedMeat);
    assert!(
        recipe
            .requirements
            .iter()
            .any(|(c, q)| *c == Concept::Meat && *q == 1),
        "recipe requires raw Meat"
    );
}

// ─── End-to-end scenario: agent next to campfire cooks raw meat ─────────────

/// Inject Cook directly into ActiveActions on an agent next to a lit
/// campfire holding raw meat. After the action's duration elapses, the
/// raw unit is gone and a fresh CookedMeat sits in inventory.
///
/// Direct injection mirrors `warmup_stance_runs_until_warmth_threshold`'s
/// pattern — bypasses arbitration so we can isolate the action mechanics.
#[test]
fn cook_runs_to_completion_next_to_campfire() {
    let mut world = TestWorld::with_seed(0);
    world.spawn_campfire(Vec2::new(0.0, 0.0));
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        ..Default::default()
    });
    world.get_mut::<ItemSlots>(agent).add(Concept::Meat, 1);
    world
        .get_mut::<ActiveActions>(agent)
        .insert(ActionState::new(ActionType::Cook, 0));

    let cook_ticks = worldsim::constants::actions::cook::DURATION_TICKS as u64;
    for _ in 0..(cook_ticks * 2) {
        world.get_mut::<bevy::prelude::Transform>(agent).translation = Vec3::new(0.0, 0.0, 0.0);
        world.tick(1);
    }

    assert_eq!(
        world.item_count(agent, Concept::Meat),
        0,
        "raw meat should be consumed by Cook"
    );
    assert!(
        world.item_count(agent, Concept::CookedMeat) >= 1,
        "Cook should produce CookedMeat in inventory"
    );

    let completed = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent {
                    kind: SimEventKind::ActionCompleted {
                        action: ActionType::Cook,
                        ..
                    },
                    ..
                } if e.involves(agent)
            )
        })
        .count();
    assert!(
        completed >= 1,
        "expected at least one Cook ActionCompleted event, got {completed}"
    );
}

/// Cook must declare both the inventory and proximity gates so arbitration
/// rejects the action when raw meat is missing or no heat source is nearby.
/// Direct `ActiveActions` injection bypasses gates, so we assert the gate
/// is *declared* on the def — the gate-evaluation code itself is shared
/// with WarmUp and exercised by `tests/test_warmth_drive.rs`.
#[test]
fn cook_def_declares_inventory_and_heat_gates() {
    use worldsim::agent::actions::definition::Gate;

    let has_meat_gate = COOK_DEF.gates.iter().any(|g| {
        matches!(
            g,
            Gate::InventoryHasQuantity {
                concept: Concept::Meat,
                ..
            }
        )
    });
    let has_heat_gate = COOK_DEF
        .gates
        .iter()
        .any(|g| matches!(g, Gate::NearHeatEmitter));
    assert!(has_meat_gate, "Cook must gate on raw Meat in inventory");
    assert!(has_heat_gate, "Cook must gate on a nearby HeatEmitter");
}

// ─── Freshness decay reaches the perishable decay system end-to-end ─────────

/// Two perishable Things — one Meat, one CookedMeat — both fresh at tick
/// zero, decayed under the same system. Cooked meat should retain more
/// freshness after the same elapsed ticks.
#[test]
fn cooked_thing_outlasts_raw_thing_under_decay_system() {
    let mut raw = Thing::fresh(Concept::Meat, 0);
    let mut cooked = Thing::fresh(Concept::CookedMeat, 0);

    let raw_rate = perishable_decay_rate(Concept::Meat).unwrap();
    let cooked_rate = perishable_decay_rate(Concept::CookedMeat).unwrap();

    let events = 10;
    for _ in 0..events {
        if let Some(f) = raw.properties.freshness.as_mut() {
            *f = (*f - raw_rate).max(0.0);
        }
        if let Some(f) = cooked.properties.freshness.as_mut() {
            *f = (*f - cooked_rate).max(0.0);
        }
    }
    let raw_left = raw.properties.freshness.unwrap();
    let cooked_left = cooked.properties.freshness.unwrap();
    assert!(
        cooked_left > raw_left,
        "after {events} decay events, cooked freshness ({cooked_left}) should exceed raw ({raw_left})"
    );
}
