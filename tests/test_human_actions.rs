//! Tests for the #269 action expansion. Covers the only logic worth
//! testing per `.claude/rules/testing.md`: gate hooks that consult new
//! state, and on_complete hooks that mutate inventory.
//!
//! Skipped by design (per the same rule):
//! - "Action X is registered" — transcribes the registry slice
//! - "Action X has gate Y" — transcribes the def, no failure detection
//! - "Action X has channel Z" — same

use bevy::math::Vec2;
use bevy::prelude::Entity;
use worldsim::agent::actions::ActionType;
use worldsim::agent::actions::GenericAction;
use worldsim::agent::actions::action::{FISH_DEF, SHARE_FOOD_DEF};
use worldsim::agent::actions::registry::{
    Action, ActionContext, ActionRegistry, CompletionContext, SpawnRequest,
};
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::body::needs::{PhysicalNeeds, PsychologicalDrives};
use worldsim::agent::events::FailureReason;
use worldsim::agent::item_slots::ItemSlots;
use worldsim::agent::mind::knowledge::{
    Concept, MindGraph, Node, Predicate, Quantity, Triple, Value, setup_ontology,
};
use worldsim::agent::psyche::emotions::EmotionalState;
use worldsim::core::time::GameTime;
use worldsim::world::map::{WORLD_HEIGHT, WORLD_WIDTH, WorldMap};

fn make_ctx<'a>(
    inventory: &'a ItemSlots,
    mind: &'a MindGraph,
    world_map: &'a WorldMap,
) -> ActionContext<'a> {
    ActionContext {
        inventory,
        mind,
        world_map,
        target_entity: None,
        target_position: None,
        agent_position: Vec2::ZERO,
        physical: None,
        drives: None,
        emotional: None,
        current_tick: 0,
        unreachable_tiles: &[],
    }
}

// ─── Nighttime gate ─────────────────────────────────────────────────────────

#[test]
fn stand_watch_rejects_during_daytime_hours() {
    let registry = ActionRegistry::new();
    let action = registry.get(ActionType::StandWatch).unwrap();
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let mut ctx = make_ctx(&inv, &mind, &map);
    // Tick 0 = 06:00 (START_HOUR). Daytime.
    ctx.current_tick = 0;
    assert!(action.can_start(&ctx).is_err());
    // Three game-hours after start = 09:00.
    ctx.current_tick = 3 * GameTime::TICKS_PER_HOUR;
    assert!(action.can_start(&ctx).is_err());
}

#[test]
fn stand_watch_admits_after_night_start_hour() {
    let registry = ActionRegistry::new();
    let action = registry.get(ActionType::StandWatch).unwrap();
    let inv = ItemSlots::agent_carry();
    let mind = mind_with_known_campfire();
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let mut ctx = make_ctx(&inv, &mind, &map);
    // 22:00 — 16 hours after the 06:00 start.
    ctx.current_tick = 16 * GameTime::TICKS_PER_HOUR;
    assert!(
        action.can_start(&ctx).is_ok(),
        "StandWatch should admit at night with a campfire on tile"
    );
}

// ─── Mood / companionship gates ─────────────────────────────────────────────

#[test]
fn dance_rejects_when_mood_is_low() {
    let registry = ActionRegistry::new();
    let action = registry.get(ActionType::Dance).unwrap();
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let drives = PsychologicalDrives::default();
    let emotional = EmotionalState {
        current_mood: 0.2,
        ..Default::default()
    };
    let mut ctx = make_ctx(&inv, &mind, &map);
    ctx.drives = Some(&drives);
    ctx.emotional = Some(&emotional);
    let err = action
        .can_start(&ctx)
        .expect_err("low mood must block Dance");
    assert!(matches!(err, FailureReason::Interrupted));
}

#[test]
fn dance_admits_when_mood_high_and_companionship_satisfied() {
    let registry = ActionRegistry::new();
    let action = registry.get(ActionType::Dance).unwrap();
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let drives = high_companionship();
    let emotional = EmotionalState {
        current_mood: 0.9,
        ..Default::default()
    };
    let mut ctx = make_ctx(&inv, &mind, &map);
    ctx.drives = Some(&drives);
    ctx.emotional = Some(&emotional);
    assert!(action.can_start(&ctx).is_ok());
}

// ─── Mourn gate: episodic Death belief ──────────────────────────────────────

#[test]
fn mourn_rejects_without_death_belief() {
    let registry = ActionRegistry::new();
    let action = registry.get(ActionType::Mourn).unwrap();
    let inv = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = make_ctx(&inv, &mind, &map);
    assert!(action.can_start(&ctx).is_err());
}

#[test]
fn mourn_admits_when_mind_records_a_death() {
    let registry = ActionRegistry::new();
    let action = registry.get(ActionType::Mourn).unwrap();
    let inv = ItemSlots::agent_carry();
    let mut mind = MindGraph::new(setup_ontology());
    let event_node = Node::Entity(Entity::from_bits(99));
    mind.assert(Triple::new(
        event_node,
        Predicate::Action,
        Value::Concept(Concept::Death),
    ));
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let ctx = make_ctx(&inv, &mind, &map);
    assert!(action.can_start(&ctx).is_ok());
}

// ─── TendWounds gate: Lame belief on target ─────────────────────────────────

#[test]
fn tend_wounds_admits_only_when_target_is_lame() {
    let registry = ActionRegistry::new();
    let action = registry.get(ActionType::TendWounds).unwrap();
    let inv = ItemSlots::agent_carry();
    let target = Entity::from_bits(7);
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);

    // Without the Lame belief — gate must reject.
    let healthy_mind = MindGraph::new(setup_ontology());
    let mut ctx = make_ctx(&inv, &healthy_mind, &map);
    ctx.target_entity = Some(target);
    assert!(action.can_start(&ctx).is_err());

    // With the Lame belief — gate must admit.
    let mut wounded_mind = MindGraph::new(setup_ontology());
    wounded_mind.assert(Triple::new(
        Node::Entity(target),
        Predicate::HasTrait,
        Value::Concept(Concept::Lame),
    ));
    let mut ctx2 = make_ctx(&inv, &wounded_mind, &map);
    ctx2.target_entity = Some(target);
    assert!(action.can_start(&ctx2).is_ok());
}

// ─── ShareFood gate: affection threshold ────────────────────────────────────

#[test]
fn share_food_rejects_when_target_affection_below_threshold() {
    let registry = ActionRegistry::new();
    let action = registry.get(ActionType::ShareFood).unwrap();
    let mut inv = ItemSlots::agent_carry();
    inv.add(Concept::Apple, 1);
    let mind = MindGraph::new(setup_ontology());
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let mut ctx = make_ctx(&inv, &mind, &map);
    ctx.target_entity = Some(Entity::from_bits(11));
    let err = action
        .can_start(&ctx)
        .expect_err("share food must block strangers");
    assert!(matches!(err, FailureReason::Interrupted));
}

#[test]
fn share_food_admits_when_target_affection_high_enough() {
    let registry = ActionRegistry::new();
    let action = registry.get(ActionType::ShareFood).unwrap();
    let mut inv = ItemSlots::agent_carry();
    inv.add(Concept::Apple, 1);
    let target = Entity::from_bits(11);
    let mut mind = MindGraph::new(setup_ontology());
    mind.assert(Triple::new(
        Node::Entity(target),
        Predicate::Affection,
        Value::Quantity(Quantity::Exact(0.8)),
    ));
    let map = WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT);
    let mut ctx = make_ctx(&inv, &mind, &map);
    ctx.target_entity = Some(target);
    assert!(action.can_start(&ctx).is_ok());
}

// ─── ShareFood on_complete: actual food transfer ────────────────────────────

#[test]
fn share_food_transfers_one_item_to_target_inventory() {
    let action = GenericAction::new(&SHARE_FOOD_DEF);
    let mind = MindGraph::new(setup_ontology());
    let mut physical = PhysicalNeeds {
        metabolism: Metabolism::well_fed(),
        ..Default::default()
    };
    let mut giver_inv = ItemSlots::agent_carry();
    giver_inv.add(Concept::Apple, 2);
    let mut recipient_inv = ItemSlots::agent_carry();
    let mut spawns: Vec<SpawnRequest> = Vec::new();

    let mut ctx = CompletionContext {
        physical: &mut physical,
        inventory: &mut giver_inv,
        drives: None,
        mind: &mind,
        skills: None,
        target_inventory: Some(&mut recipient_inv),
        target_entity: Some(Entity::from_bits(11)),
        tick: 0,
        agent_position: Vec2::ZERO,
        spawn_requests: &mut spawns,
    };
    action.on_complete(&mut ctx);

    assert_eq!(giver_inv.count(Concept::Apple), 1, "giver loses one apple");
    assert_eq!(
        recipient_inv.count(Concept::Apple),
        1,
        "recipient gains one apple"
    );
}

// ─── Fish on_complete: stamps freshness ─────────────────────────────────────

#[test]
fn fish_on_complete_adds_fresh_fish_with_creation_tick() {
    let action = GenericAction::new(&FISH_DEF);
    let mind = MindGraph::new(setup_ontology());
    let mut physical = PhysicalNeeds::default();
    let mut inv = ItemSlots::agent_carry();
    let mut spawns: Vec<SpawnRequest> = Vec::new();

    let mut ctx = CompletionContext {
        physical: &mut physical,
        inventory: &mut inv,
        drives: None,
        mind: &mind,
        skills: None,
        target_inventory: None,
        target_entity: None,
        tick: 5_000,
        agent_position: Vec2::ZERO,
        spawn_requests: &mut spawns,
    };
    action.on_complete(&mut ctx);

    let caught = inv
        .all_items()
        .find(|t| t.concept == Concept::Fish)
        .expect("Fish on_complete must add a Fish item");
    assert_eq!(caught.properties.freshness, Some(1.0));
    assert_eq!(caught.properties.created_at, Some(5_000));
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn mind_with_known_campfire() -> MindGraph {
    use worldsim::agent::mind::knowledge::Predicate as P;
    let mut mind = MindGraph::new(setup_ontology());
    let agent_tile = Value::Tile((0, 0));
    mind.assert(Triple::new(Node::Self_, P::LocatedAt, agent_tile.clone()));
    let campfire_entity = Entity::from_bits(42);
    let campfire = || Node::Entity(campfire_entity);
    mind.assert(Triple::new(
        campfire(),
        P::IsA,
        Value::Concept(Concept::Campfire),
    ));
    mind.assert(Triple::new(
        campfire(),
        P::HasTrait,
        Value::Concept(Concept::HeatEmitting),
    ));
    mind.assert(Triple::new(campfire(), P::LocatedAt, agent_tile));
    mind
}

fn high_companionship() -> PsychologicalDrives {
    use worldsim::agent::body::need::Need;
    PsychologicalDrives {
        companionship: Need::new(0.9),
        ..Default::default()
    }
}
