//! Tests for the Deposit and Take actions (#62).
//!
//! Validates that both actions are polymorphic across `ItemSlots` shapes:
//! Deposit lands in the first slot whose filter and access permit, Take pulls
//! from the first slot whose `extract_access` is not `None`. Construction
//! site slots (sealed extract) are explicitly tested as a non-extractable
//! target.

use bevy::prelude::*;
use worldsim::agent::actions::registry::{
    ActionContext, ActionRegistry, CompletionContext, SpawnRequest,
};
use worldsim::agent::actions::types::ActionType;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::item_slots::{Access, ItemSlots, Slot, SlotFilter, SlotRole};
use worldsim::agent::mind::knowledge::{Concept, MindGraph, setup_ontology};
use worldsim::world::map::{WORLD_HEIGHT, WORLD_WIDTH, WorldMap};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn empty_mind() -> MindGraph {
    MindGraph::new(setup_ontology())
}

fn empty_world_map() -> WorldMap {
    WorldMap::new(WORLD_WIDTH, WORLD_HEIGHT)
}

fn construction_site_with_wood_slot(required: u32) -> ItemSlots {
    ItemSlots {
        slots: vec![Slot::construction(Concept::Wood, required)],
    }
}

fn run_on_complete(
    action: &dyn worldsim::agent::actions::registry::Action,
    inventory: &mut ItemSlots,
    target_inventory: Option<&mut ItemSlots>,
) {
    let mut physical = PhysicalNeeds::default();
    let mut spawn_requests: Vec<SpawnRequest> = Vec::new();
    let mind = empty_mind();
    let mut ctx = CompletionContext {
        physical: &mut physical,
        inventory,
        drives: None,
        mind: &mind,
        skills: None,
        target_inventory,
        target_entity: None,
        tick: 0,
        agent_position: Vec2::ZERO,
        spawn_requests: &mut spawn_requests,
    };
    action.on_complete(&mut ctx);
}

// ═══════════════════════════════════════════════════════════════════════════
// Registration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deposit_action_is_registered() {
    let registry = ActionRegistry::new();
    assert!(
        registry.get(ActionType::Deposit).is_some(),
        "Deposit must be registered"
    );
}

#[test]
fn take_action_is_registered() {
    let registry = ActionRegistry::new();
    assert!(
        registry.get(ActionType::Take).is_some(),
        "Take must be registered"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Deposit
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deposit_transfers_wood_into_construction_slot() {
    let registry = ActionRegistry::new();
    let deposit = registry.get(ActionType::Deposit).unwrap();

    let mut agent_inv = ItemSlots::agent_carry();
    agent_inv.add(Concept::Wood, 3);

    let mut site_inv = construction_site_with_wood_slot(3);

    run_on_complete(deposit, &mut agent_inv, Some(&mut site_inv));

    assert_eq!(
        agent_inv.count(Concept::Wood),
        0,
        "Agent inventory should be empty after deposit"
    );
    assert_eq!(
        site_inv.count(Concept::Wood),
        3,
        "Site should hold 3 wood after deposit"
    );
}

#[test]
fn deposit_into_filtered_slot_skips_unmatched_concept() {
    // Site only accepts Wood. Agent has Stone. Deposit should silently
    // do nothing (no item the target accepts).
    let registry = ActionRegistry::new();
    let deposit = registry.get(ActionType::Deposit).unwrap();

    let mut agent_inv = ItemSlots::agent_carry();
    agent_inv.add(Concept::Stone, 2);

    let mut site_inv = construction_site_with_wood_slot(3);

    run_on_complete(deposit, &mut agent_inv, Some(&mut site_inv));

    assert_eq!(
        agent_inv.count(Concept::Stone),
        2,
        "Agent stone must be untouched when no slot accepts it"
    );
    assert_eq!(site_inv.count(Concept::Wood), 0, "Site must remain empty");
    assert_eq!(
        site_inv.count(Concept::Stone),
        0,
        "Site must not contain stone"
    );
}

#[test]
fn deposit_respects_capacity_limit_with_partial_fill() {
    // Site needs 3 wood. Agent has 5 wood. Deposit should leave 2 with the
    // agent and fill the slot to its capacity of 3.
    let registry = ActionRegistry::new();
    let deposit = registry.get(ActionType::Deposit).unwrap();

    let mut agent_inv = ItemSlots::agent_carry();
    agent_inv.add(Concept::Wood, 5);

    let mut site_inv = construction_site_with_wood_slot(3);

    run_on_complete(deposit, &mut agent_inv, Some(&mut site_inv));

    assert_eq!(
        site_inv.count(Concept::Wood),
        3,
        "Site must fill to its 3-wood capacity"
    );
    assert_eq!(
        agent_inv.count(Concept::Wood),
        2,
        "Agent must keep the 2 surplus wood"
    );
}

#[test]
fn deposit_into_full_slot_does_nothing() {
    let registry = ActionRegistry::new();
    let deposit = registry.get(ActionType::Deposit).unwrap();

    let mut agent_inv = ItemSlots::agent_carry();
    agent_inv.add(Concept::Wood, 1);

    let mut site_inv = construction_site_with_wood_slot(3);
    site_inv.deposit(Concept::Wood, 3, None); // pre-fill to capacity

    run_on_complete(deposit, &mut agent_inv, Some(&mut site_inv));

    assert_eq!(
        agent_inv.count(Concept::Wood),
        1,
        "Agent wood must be untouched when target has no room"
    );
    assert_eq!(site_inv.count(Concept::Wood), 3);
}

#[test]
fn deposit_can_start_requires_target_entity() {
    let registry = ActionRegistry::new();
    let deposit = registry.get(ActionType::Deposit).unwrap();

    let mut inv = ItemSlots::agent_carry();
    inv.add(Concept::Wood, 3);

    let mind = empty_mind();
    let world_map = empty_world_map();

    let no_target = ActionContext {
        inventory: &inv,
        mind: &mind,
        world_map: &world_map,
        target_entity: None,
        target_position: None,
        agent_position: Vec2::ZERO,
        physical: None,
        drives: None,
        emotional: None,
        current_tick: 0,
        unreachable_tiles: &[],
    };
    assert!(deposit.can_start(&no_target).is_err());

    // Synthesize an arbitrary entity id — can_start only checks .is_some()
    let with_target = ActionContext {
        inventory: &inv,
        mind: &mind,
        world_map: &world_map,
        target_entity: Some(Entity::from_bits(1)),
        target_position: None,
        agent_position: Vec2::ZERO,
        physical: None,
        drives: None,
        emotional: None,
        current_tick: 0,
        unreachable_tiles: &[],
    };
    assert!(deposit.can_start(&with_target).is_ok());
}

#[test]
fn deposit_can_start_requires_non_empty_inventory() {
    let registry = ActionRegistry::new();
    let deposit = registry.get(ActionType::Deposit).unwrap();

    let inv = ItemSlots::agent_carry(); // empty
    let mind = empty_mind();
    let world_map = empty_world_map();

    let ctx = ActionContext {
        inventory: &inv,
        mind: &mind,
        world_map: &world_map,
        target_entity: Some(Entity::from_bits(1)),
        target_position: None,
        agent_position: Vec2::ZERO,
        physical: None,
        drives: None,
        emotional: None,
        current_tick: 0,
        unreachable_tiles: &[],
    };
    assert!(
        deposit.can_start(&ctx).is_err(),
        "Deposit must fail when the agent has nothing to deposit"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Take
// ═══════════════════════════════════════════════════════════════════════════

fn public_chest_with(concept: Concept, qty: u32) -> ItemSlots {
    let mut slots = ItemSlots {
        slots: vec![Slot {
            role: SlotRole::Free,
            filter: SlotFilter::Any,
            capacity: None,
            contents: Vec::new(),
            deposit_access: Access::Public,
            extract_access: Access::Public,
        }],
    };
    slots.add(concept, qty);
    slots
}

#[test]
fn take_pulls_apple_from_public_chest() {
    let registry = ActionRegistry::new();
    let take = registry.get(ActionType::Take).unwrap();

    let mut agent_inv = ItemSlots::agent_carry();
    let mut chest_inv = public_chest_with(Concept::Apple, 4);

    run_on_complete(take, &mut agent_inv, Some(&mut chest_inv));

    assert_eq!(
        agent_inv.count(Concept::Apple),
        4,
        "Agent should now hold the 4 apples"
    );
    assert_eq!(
        chest_inv.count(Concept::Apple),
        0,
        "Chest must be empty after the take"
    );
}

#[test]
fn take_skips_construction_slot_with_sealed_extract() {
    // Construction slots have extract_access: None. Take must not pull from them.
    let registry = ActionRegistry::new();
    let take = registry.get(ActionType::Take).unwrap();

    let mut agent_inv = ItemSlots::agent_carry();
    let mut site_inv = construction_site_with_wood_slot(3);
    site_inv.deposit(Concept::Wood, 3, None);

    run_on_complete(take, &mut agent_inv, Some(&mut site_inv));

    assert_eq!(
        agent_inv.count(Concept::Wood),
        0,
        "Agent must not extract from a sealed construction slot"
    );
    assert_eq!(site_inv.count(Concept::Wood), 3, "Site wood must remain");
}

#[test]
fn take_with_no_target_inventory_is_a_noop() {
    // No target_inventory provided. on_complete should silently do nothing
    // rather than panic.
    let registry = ActionRegistry::new();
    let take = registry.get(ActionType::Take).unwrap();

    let mut agent_inv = ItemSlots::agent_carry();

    run_on_complete(take, &mut agent_inv, None);

    assert_eq!(agent_inv.count(Concept::Apple), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Multi-agent collaborative deposit (the #61 → #62 payoff path)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn two_agents_can_each_partially_fill_the_same_construction_site() {
    let registry = ActionRegistry::new();
    let deposit = registry.get(ActionType::Deposit).unwrap();

    let mut alice_inv = ItemSlots::agent_carry();
    alice_inv.add(Concept::Wood, 1);

    let mut bob_inv = ItemSlots::agent_carry();
    bob_inv.add(Concept::Wood, 2);

    // Shared site requiring 3 wood total.
    let mut site_inv = construction_site_with_wood_slot(3);

    run_on_complete(deposit, &mut alice_inv, Some(&mut site_inv));
    assert_eq!(site_inv.count(Concept::Wood), 1);
    assert_eq!(alice_inv.count(Concept::Wood), 0);

    run_on_complete(deposit, &mut bob_inv, Some(&mut site_inv));
    assert_eq!(
        site_inv.count(Concept::Wood),
        3,
        "Site must reach full capacity from two contributors"
    );
    assert_eq!(bob_inv.count(Concept::Wood), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// to_template_for_target proximity injection (auto-injected by #219 default)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deposit_template_includes_target_tile_precondition() {
    use worldsim::agent::actions::TargetCandidate;
    use worldsim::agent::brains::thinking::TriplePattern;
    use worldsim::agent::mind::knowledge::{Node, Predicate, Value, setup_ontology};

    let registry = ActionRegistry::new();
    let deposit = registry.get(ActionType::Deposit).unwrap();
    let mind = worldsim::agent::mind::knowledge::MindGraph::new(setup_ontology());

    // Pixel (64, 64) at TILE_SIZE 16 maps to tile (4, 4)
    let target = TargetCandidate::Entity {
        entity: Entity::from_bits(7),
        pos: Vec2::new(64.0, 64.0),
    };
    let template = deposit.to_template_for_target(&target, &mind);

    let expected = TriplePattern::new(
        Some(Node::Self_),
        Some(Predicate::LocatedAt),
        Some(Value::Tile((4, 4))),
    );
    assert!(
        template.preconditions.contains(&expected),
        "Deposit template must include the auto-injected target tile location precondition"
    );
}

#[test]
fn take_template_includes_target_tile_precondition() {
    use worldsim::agent::actions::TargetCandidate;
    use worldsim::agent::brains::thinking::TriplePattern;
    use worldsim::agent::mind::knowledge::{Node, Predicate, Value, setup_ontology};

    let registry = ActionRegistry::new();
    let take = registry.get(ActionType::Take).unwrap();
    let mind = worldsim::agent::mind::knowledge::MindGraph::new(setup_ontology());

    let target = TargetCandidate::Entity {
        entity: Entity::from_bits(7),
        pos: Vec2::new(32.0, 48.0),
    };
    let template = take.to_template_for_target(&target, &mind);

    let expected = TriplePattern::new(
        Some(Node::Self_),
        Some(Predicate::LocatedAt),
        Some(Value::Tile((2, 3))),
    );
    assert!(
        template.preconditions.contains(&expected),
        "Take template must include the auto-injected target tile location precondition"
    );
}
