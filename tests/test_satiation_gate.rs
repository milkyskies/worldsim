//! Unified satiation gate: Eat/Drink/Sleep/Rest all refuse to start when the
//! need they target is already close enough to full. Without the gate,
//! these actions re-fire every duration cycle as long as their
//! precondition (has food, next to water, etc.) holds, producing the
//! "60 Eats in 20 game-min" chain-eating bug observed in #581.

use bevy::prelude::*;
use worldsim::agent::actions::GenericAction;
use worldsim::agent::actions::action::{DRINK_DEF, EAT_DEF, REST_DEF, SLEEP_DEF};
use worldsim::agent::actions::registry::{Action, ActionContext};
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::body::need::{Need, NeedKind};
use worldsim::agent::body::needs::{PhysicalNeeds, Stamina};
use worldsim::agent::item_slots::ItemSlots;
use worldsim::agent::mind::knowledge::{MindGraph, setup_ontology};
use worldsim::world::map::{WORLD_HEIGHT, WORLD_WIDTH, WorldMap};

fn ctx_with_needs<'a>(
    inventory: &'a ItemSlots,
    mind: &'a MindGraph,
    world_map: &'a WorldMap,
    physical: &'a PhysicalNeeds,
) -> ActionContext<'a> {
    ActionContext {
        inventory,
        mind,
        world_map,
        target_entity: None,
        target_position: None,
        agent_position: Vec2::ZERO,
        physical: Some(physical),
        drives: None,
        emotional: None,
        current_tick: 0,
    }
}

#[test]
fn eat_satiation_reports_stomach_fraction_as_hunger() {
    let physical = PhysicalNeeds {
        metabolism: Metabolism::well_fed(),
        ..Default::default()
    };
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let eat = GenericAction::new(&EAT_DEF);
    let (kind, fullness) = eat
        .satiation(ctx.physical, Some(ctx.inventory))
        .expect("Eat should expose satiation");
    assert_eq!(kind, NeedKind::Hunger);
    // well-fed metabolism starts at stomach 100/100 = 1.0
    assert!(fullness > 0.95, "stomach should read ~full, got {fullness}");
}

#[test]
fn drink_refuses_when_hydration_full() {
    let physical = PhysicalNeeds::default(); // hydration full, wakefulness full
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let drink = GenericAction::new(&DRINK_DEF);
    let (kind, fullness) = drink.satiation(ctx.physical, Some(ctx.inventory)).unwrap();
    assert_eq!(kind, NeedKind::Thirst);
    // Should trip the threshold (0.95).
    assert!(fullness >= kind.satiation_threshold());
}

#[test]
fn drink_allows_when_thirsty() {
    let physical = PhysicalNeeds {
        hydration: Need::new(0.3),
        ..Default::default()
    };
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let drink = GenericAction::new(&DRINK_DEF);
    let (_kind, fullness) = drink.satiation(ctx.physical, Some(ctx.inventory)).unwrap();
    assert!(fullness < NeedKind::Thirst.satiation_threshold());
}

#[test]
fn sleep_refuses_when_already_rested() {
    let physical = PhysicalNeeds::default(); // wakefulness full
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let sleep = GenericAction::new(&SLEEP_DEF);
    let (kind, fullness) = sleep.satiation(ctx.physical, Some(ctx.inventory)).unwrap();
    assert_eq!(kind, NeedKind::Sleep);
    assert!(fullness >= kind.satiation_threshold());
}

#[test]
fn rest_refuses_when_aerobic_full() {
    let physical = PhysicalNeeds {
        stamina: Stamina::default(), // aerobic_fraction == 1.0
        ..Default::default()
    };
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let rest = GenericAction::new(&REST_DEF);
    let (kind, fullness) = rest.satiation(ctx.physical, Some(ctx.inventory)).unwrap();
    assert_eq!(kind, NeedKind::Stamina);
    assert!(fullness >= kind.satiation_threshold());
}

fn physical_with_stomach(mass: f32) -> PhysicalNeeds {
    PhysicalNeeds {
        metabolism: Metabolism {
            stomach_carbs: mass,
            stomach_fat: 0.0,
            glucose: 10.0,
            reserves: 10.0,
        },
        ..Default::default()
    }
}

#[test]
fn eat_dead_zone_berry_in_seventy_mass_stomach_blocks() {
    use worldsim::agent::mind::knowledge::Concept;

    let physical = physical_with_stomach(70.0);
    let mut inv = ItemSlots::agent_carry();
    inv.add(Concept::Berry, 1);
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let eat = GenericAction::new(&EAT_DEF);
    let (kind, fullness) = eat.satiation(ctx.physical, Some(ctx.inventory)).unwrap();
    assert_eq!(kind, NeedKind::Hunger);
    assert!(
        fullness >= kind.satiation_threshold(),
        "berry (40 mass) into 30 headroom must trip satiation; got fullness={fullness}"
    );
}

#[test]
fn eat_allows_berry_when_stomach_has_room() {
    use worldsim::agent::mind::knowledge::Concept;

    // Headroom 50, berry mass 40 — fits.
    let physical = physical_with_stomach(50.0);
    let mut inv = ItemSlots::agent_carry();
    inv.add(Concept::Berry, 1);
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let eat = GenericAction::new(&EAT_DEF);
    let (_kind, fullness) = eat.satiation(ctx.physical, Some(ctx.inventory)).unwrap();
    assert!(
        fullness < NeedKind::Hunger.satiation_threshold(),
        "50 stomach + 40 berry fits; must not trip satiation; got fullness={fullness}"
    );
}

#[test]
fn eat_allows_apple_when_smaller_than_berry_fits() {
    use worldsim::agent::mind::knowledge::Concept;

    // Apple mass 31 fits easily in 50 headroom.
    let physical = physical_with_stomach(50.0);
    let mut inv = ItemSlots::agent_carry();
    inv.add(Concept::Apple, 1);
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let eat = GenericAction::new(&EAT_DEF);
    let (_kind, fullness) = eat.satiation(ctx.physical, Some(ctx.inventory)).unwrap();
    assert!(
        fullness < NeedKind::Hunger.satiation_threshold(),
        "50 stomach + 31 apple fits; must not trip satiation; got fullness={fullness}"
    );
}

// ─── is_plan_time_viable — the trait-level wrapper shared by survival and
//     rational brains — must report the same true/false as the satiation
//     gate would at runtime. Actions without a satiation gate are always
//     viable.

#[test]
fn is_plan_time_viable_false_for_eat_when_stomach_full() {
    let physical = PhysicalNeeds {
        metabolism: Metabolism::well_fed(),
        ..Default::default()
    };
    let inv = ItemSlots::agent_carry();
    let eat = GenericAction::new(&EAT_DEF);
    assert!(
        !eat.is_plan_time_viable(Some(&physical), Some(&inv)),
        "Eat must not be plan-time viable on a full stomach"
    );
}

#[test]
fn is_plan_time_viable_false_for_drink_when_hydrated() {
    let physical = PhysicalNeeds::default();
    let inv = ItemSlots::agent_carry();
    let drink = GenericAction::new(&DRINK_DEF);
    assert!(!drink.is_plan_time_viable(Some(&physical), Some(&inv)));
}

#[test]
fn is_plan_time_viable_false_for_sleep_when_rested() {
    let physical = PhysicalNeeds::default();
    let inv = ItemSlots::agent_carry();
    let sleep = GenericAction::new(&SLEEP_DEF);
    assert!(!sleep.is_plan_time_viable(Some(&physical), Some(&inv)));
}

#[test]
fn is_plan_time_viable_true_for_eat_when_hungry() {
    use worldsim::agent::mind::knowledge::Concept;
    // Empty-ish stomach with food in inventory.
    let physical = physical_with_stomach(10.0);
    let mut inv = ItemSlots::agent_carry();
    inv.add(Concept::Apple, 1);
    let eat = GenericAction::new(&EAT_DEF);
    assert!(eat.is_plan_time_viable(Some(&physical), Some(&inv)));
}

#[test]
fn is_plan_time_viable_true_for_actions_without_satiation_gate() {
    use worldsim::agent::actions::action::HARVEST_DEF;
    let physical = PhysicalNeeds::default();
    let inv = ItemSlots::agent_carry();
    let harvest = GenericAction::new(&HARVEST_DEF);
    assert!(
        harvest.is_plan_time_viable(Some(&physical), Some(&inv)),
        "actions without a satiation gate are always plan-time viable"
    );
}
