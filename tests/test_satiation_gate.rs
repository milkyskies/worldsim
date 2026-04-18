//! Unified satiation gate: Eat/Drink/Sleep/Rest all refuse to start when the
//! need they target is already close enough to full. Without the gate,
//! these actions re-fire every duration cycle as long as their
//! precondition (has food, next to water, etc.) holds, producing the
//! "60 Eats in 20 game-min" chain-eating bug observed in #581.

use bevy::prelude::*;
use worldsim::agent::actions::action::{DrinkAction, EatAction, RestAction, SleepAction};
use worldsim::agent::actions::registry::{Action, ActionContext};
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::body::need::{Need, NeedKind};
use worldsim::agent::body::needs::{PhysicalNeeds, Stamina};
use worldsim::agent::events::FailureReason;
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
    }
}

#[test]
fn eat_satiation_reports_stomach_fraction_as_hunger() {
    let mut physical = PhysicalNeeds::default();
    physical.metabolism = Metabolism::well_fed();
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let eat = EatAction;
    let (kind, fullness) = eat.satiation(&ctx).expect("Eat should expose satiation");
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

    let drink = DrinkAction;
    let (kind, fullness) = drink.satiation(&ctx).unwrap();
    assert_eq!(kind, NeedKind::Thirst);
    // Should trip the threshold (0.95).
    assert!(fullness >= kind.satiation_threshold());
}

#[test]
fn drink_allows_when_thirsty() {
    let mut physical = PhysicalNeeds::default();
    physical.hydration = Need::new(0.3);
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let drink = DrinkAction;
    let (_kind, fullness) = drink.satiation(&ctx).unwrap();
    assert!(fullness < NeedKind::Thirst.satiation_threshold());
}

#[test]
fn sleep_refuses_when_already_rested() {
    let physical = PhysicalNeeds::default(); // wakefulness full
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let sleep = SleepAction;
    let (kind, fullness) = sleep.satiation(&ctx).unwrap();
    assert_eq!(kind, NeedKind::Sleep);
    assert!(fullness >= kind.satiation_threshold());
}

#[test]
fn rest_refuses_when_aerobic_full() {
    let mut physical = PhysicalNeeds::default();
    physical.stamina = Stamina::default(); // aerobic_fraction == 1.0
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = ctx_with_needs(&inv, &mind, &map, &physical);

    let rest = RestAction;
    let (kind, fullness) = rest.satiation(&ctx).unwrap();
    assert_eq!(kind, NeedKind::Stamina);
    assert!(fullness >= kind.satiation_threshold());
}

#[test]
fn already_satiated_failure_reason_round_trips() {
    let reason = FailureReason::AlreadySatiated {
        kind: NeedKind::Hunger,
        fullness: 0.9,
    };
    let cloned = reason.clone();
    assert_eq!(reason, cloned);
    // The satiation threshold for Hunger should block at 0.9.
    assert!(0.9 >= NeedKind::Hunger.satiation_threshold());
}
