//! Integration tests for the FoodSecurity drive and the
//! StockChest / BuildStorageChest actions. Mirrors
//! `test_warmth_drive.rs` and `test_rest_quality_drive.rs` end-to-end.

use bevy::math::Vec2;
use worldsim::agent::body::need::NeedKind;
use worldsim::agent::brains::proposal::Intent;
use worldsim::agent::brains::thinking::Goal;
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use worldsim::agent::nervous_system::urgency::UrgencySource;
use worldsim::testing::{AgentConfig, TestWorld};

// ─── Unit: drive→intent→satisfier routing closes over FoodSecurity ──────────

#[test]
fn food_security_urgency_routes_to_satisfy_food_security_intent() {
    assert_eq!(
        Intent::from_urgency_source(UrgencySource::FoodSecurity),
        Intent::SatisfyFoodSecurity
    );
}

#[test]
fn food_security_need_kind_satisfier_is_stock_chest() {
    assert_eq!(
        NeedKind::FoodSecurity.satisfier(),
        Some(worldsim::agent::actions::ActionType::StockChest)
    );
}

#[test]
fn food_security_need_kind_satiation_threshold_matches_pattern() {
    assert!((NeedKind::FoodSecurity.satiation_threshold() - 0.95).abs() < 1e-6);
}

// ─── Unit: goal formulation ─────────────────────────────────────────────────

#[test]
fn food_security_urgency_formulates_food_security_body_state_goal() {
    let plan_memory = worldsim::agent::brains::plan_memory::PlanMemory::default();
    let ontology = worldsim::agent::mind::knowledge::setup_ontology();
    let mind = MindGraph::new(ontology);

    let goal: Goal = worldsim::agent::brains::rational::goal_for_urgency(
        UrgencySource::FoodSecurity,
        0.8,
        &plan_memory,
        &mind,
    )
    .expect("FoodSecurity urgency must produce a goal");

    assert_eq!(goal.conditions.len(), 1);
    let condition = &goal.conditions[0];
    assert_eq!(condition.subject, Some(Node::Self_));
    assert_eq!(condition.predicate, Some(Predicate::FoodSecurity));
    assert!(matches!(condition.object, Some(Value::Quantity(_))));
}

// ─── Scenario: drain + recovery ─────────────────────────────────────────────

#[test]
fn agent_with_no_chest_drains_food_security() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(1000.0, 1000.0),
        food_security: 0.8,
        ..Default::default()
    });
    let before = world.agent_food_security(agent);
    for _ in 0..200 {
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(1000.0, 1000.0, 0.0);
        world.tick(1);
    }
    let after = world.agent_food_security(agent);
    assert!(
        after < before,
        "agent without chest or surplus should slowly lose food-security \
         (before={before:.3}, after={after:.3})"
    );
}

#[test]
fn agent_near_empty_chest_does_not_recover_food_security() {
    let mut world = TestWorld::with_seed(0);
    world.spawn_storage_chest(Vec2::new(0.0, 0.0));
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        food_security: 0.5,
        ..Default::default()
    });

    let before = world.agent_food_security(agent);
    for _ in 0..200 {
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(0.0, 0.0, 0.0);
        world.tick(1);
    }
    let after = world.agent_food_security(agent);

    assert!(
        after <= before,
        "an empty chest must give no recovery — drive's whole point is \
         access to a stockpile (before={before:.3}, after={after:.3})"
    );
}

#[test]
fn agent_near_stocked_chest_recovers_food_security() {
    use worldsim::agent::item_slots::{ItemSlots, Thing};
    use worldsim::agent::mind::knowledge::Concept;

    let mut world = TestWorld::with_seed(0);
    let chest = world.spawn_storage_chest(Vec2::new(0.0, 0.0));
    // Pre-stock the chest with a few apples so the body system grants
    // recovery via proximity.
    {
        let mut slots = world.get_mut::<ItemSlots>(chest);
        slots.add_thing(Thing::new(Concept::Apple));
        slots.add_thing(Thing::new(Concept::Apple));
    }
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        food_security: 0.2,
        ..Default::default()
    });

    let before = world.agent_food_security(agent);
    for _ in 0..600 {
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(0.0, 0.0, 0.0);
        world.tick(1);
    }
    let after = world.agent_food_security(agent);

    assert!(
        after > before,
        "agent near a stocked chest should gain food-security \
         (before={before:.3}, after={after:.3})"
    );
}

#[test]
fn food_security_never_exceeds_one() {
    use worldsim::agent::item_slots::{ItemSlots, Thing};
    use worldsim::agent::mind::knowledge::Concept;

    let mut world = TestWorld::with_seed(0);
    let chest = world.spawn_storage_chest(Vec2::new(0.0, 0.0));
    {
        let mut slots = world.get_mut::<ItemSlots>(chest);
        slots.add_thing(Thing::new(Concept::Apple));
    }
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        food_security: 0.95,
        ..Default::default()
    });
    world.tick(500);
    let fs = world.agent_food_security(agent);
    assert!(
        (0.0..=1.0).contains(&fs),
        "food-security must stay in [0, 1] (got {fs})"
    );
}

// ─── Planner: food-security goal closes the build-chest chain ───────────────

/// Insecure agent with wood + no known chest must close the FoodSecurity
/// goal by chaining StockChest -> (Self, Near, StorageChest) ->
/// BuildStorageChest. Mirrors the warmth + rest-quality build chains.
#[test]
fn insecure_agent_with_wood_plans_build_storage_chest() {
    use worldsim::agent::actions::{ActionRegistry, ActionType, TargetCandidate};
    use worldsim::agent::brains::planner::{PlanCostContext, regressive_plan};
    use worldsim::agent::brains::thinking::TriplePattern;
    use worldsim::agent::mind::knowledge::{Quantity, Triple, setup_ontology};

    let ontology = setup_ontology();
    let mut mind = MindGraph::new(ontology);

    mind.assert(Triple::new(
        Node::Concept(Concept::StorageChest),
        Predicate::Requires,
        Value::Item(
            Concept::Wood,
            worldsim::constants::actions::build::STORAGE_CHEST_WOOD_REQUIRED,
        ),
    ));
    mind.assert(Triple::new(
        Node::Self_,
        Predicate::LocatedAt,
        Value::Tile((0, 0)),
    ));
    let mut inventory = worldsim::agent::item_slots::ItemSlots::agent_carry();
    inventory.add(
        Concept::Wood,
        worldsim::constants::actions::build::STORAGE_CHEST_WOOD_REQUIRED,
    );
    // Carry surplus food so StockChest's SelfContainsFood precondition
    // grounds without the planner needing to chain Harvest first.
    inventory.add(Concept::Apple, 3);

    let registry = ActionRegistry::new();
    let build_chest = registry
        .get(ActionType::BuildStorageChest)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let check_stockpile = registry
        .get(ActionType::StockChest)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let available = vec![build_chest, check_stockpile];

    let goal = Goal {
        conditions: vec![TriplePattern::self_has(
            Predicate::FoodSecurity,
            Value::Quantity(Quantity::Exact(100.0)),
        )],
        priority: 80.0,
    };

    let (plan, stats) = regressive_plan(
        &mind,
        Some(&inventory),
        &goal,
        &available,
        &PlanCostContext::neutral(),
    );
    let plan = plan.unwrap_or_else(|| {
        panic!(
            "Planner must close FoodSecurity goal via StockChest + \
             BuildStorageChest chain; unmet: {:?}",
            stats.best_unmet_goals
        )
    });

    assert!(
        plan.iter()
            .any(|a| a.action_type == ActionType::BuildStorageChest),
        "Plan must include BuildStorageChest. Plan: {:?}",
        plan.iter().map(|a| a.action_type).collect::<Vec<_>>()
    );
    assert!(
        plan.iter().any(|a| a.action_type == ActionType::StockChest),
        "Plan must include StockChest."
    );

    let build_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::BuildStorageChest)
        .unwrap();
    let check_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::StockChest)
        .unwrap();
    assert!(
        build_idx < check_idx,
        "BuildStorageChest must execute before StockChest"
    );
}

// ─── On-complete invariants ──────────────────────────────────────────────────

#[test]
fn stock_chest_on_complete_does_not_top_up_food_security() {
    use worldsim::agent::actions::GenericAction;
    use worldsim::agent::actions::action::STOCK_CHEST_DEF;
    use worldsim::agent::actions::registry::{Action, CompletionContext, SpawnRequest};
    use worldsim::agent::body::metabolism::Metabolism;
    use worldsim::agent::body::need::Need;
    use worldsim::agent::body::needs::PhysicalNeeds;
    use worldsim::agent::item_slots::ItemSlots;
    use worldsim::agent::mind::knowledge::setup_ontology;

    let action = GenericAction::new(&STOCK_CHEST_DEF);
    let mut physical = PhysicalNeeds {
        metabolism: Metabolism::empty(),
        ..Default::default()
    };
    physical.food_security = Need::new(0.2);
    let mut inventory = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let mut spawn_requests: Vec<SpawnRequest> = Vec::new();

    let before = physical.food_security.value;
    let mut ctx = CompletionContext {
        physical: &mut physical,
        inventory: &mut inventory,
        drives: None,
        mind: &mind,
        skills: None,
        target_inventory: None,
        target_entity: None,
        tick: 0,
        agent_position: Vec2::ZERO,
        spawn_requests: &mut spawn_requests,
    };
    action.on_complete(&mut ctx);

    assert!(
        (physical.food_security.value - before).abs() < f32::EPSILON,
        "StockChest.on_complete must not mutate food_security"
    );
}

/// `StockChest` is a `Timed` action that completes on its duration timer,
/// not on a body-state predicate (unlike `WarmUp` / `RestInShelter`). The
/// completion is `Never` so the auto-complete check should never fire on
/// arbitrary food-security values.
#[test]
fn stock_chest_does_not_auto_complete_on_food_security_threshold() {
    use worldsim::agent::actions::GenericAction;
    use worldsim::agent::actions::action::STOCK_CHEST_DEF;
    use worldsim::agent::actions::registry::Action;
    use worldsim::agent::body::need::Need;
    use worldsim::agent::body::needs::PhysicalNeeds;

    let action = GenericAction::new(&STOCK_CHEST_DEF);
    let mut physical = PhysicalNeeds {
        food_security: Need::new(0.5),
        ..Default::default()
    };
    assert!(!action.should_complete(&physical));
    physical.food_security = Need::new(0.95);
    assert!(!action.should_complete(&physical));
}

/// `StockChest.on_complete` should move edible items from agent inventory
/// into the chest's `ItemSlots`. Pre-fix the chest stays empty even after
/// a stocking action runs to completion.
#[test]
fn stock_chest_on_complete_moves_food_into_chest() {
    use worldsim::agent::actions::GenericAction;
    use worldsim::agent::actions::action::STOCK_CHEST_DEF;
    use worldsim::agent::actions::registry::{Action, CompletionContext, SpawnRequest};
    use worldsim::agent::body::needs::PhysicalNeeds;
    use worldsim::agent::item_slots::{ItemSlots, Slot, Thing};
    use worldsim::agent::mind::knowledge::{Concept, setup_ontology};

    let action = GenericAction::new(&STOCK_CHEST_DEF);
    let mut physical = PhysicalNeeds::default();
    let mut inventory = ItemSlots::agent_carry();
    inventory.add_thing(Thing::new(Concept::Apple));
    inventory.add_thing(Thing::new(Concept::Apple));

    let mut chest = ItemSlots {
        slots: vec![Slot {
            role: worldsim::agent::item_slots::SlotRole::Free,
            filter: worldsim::agent::item_slots::SlotFilter::Any,
            capacity: Some(20),
            contents: Vec::new(),
            deposit_access: worldsim::agent::item_slots::Access::Public,
            extract_access: worldsim::agent::item_slots::Access::Public,
        }],
    };

    let mind = MindGraph::new(setup_ontology());
    let mut spawn_requests: Vec<SpawnRequest> = Vec::new();
    let mut ctx = CompletionContext {
        physical: &mut physical,
        inventory: &mut inventory,
        drives: None,
        mind: &mind,
        skills: None,
        target_inventory: Some(&mut chest),
        target_entity: None,
        tick: 0,
        agent_position: Vec2::ZERO,
        spawn_requests: &mut spawn_requests,
    };
    action.on_complete(&mut ctx);

    assert_eq!(
        inventory.count(Concept::Apple),
        0,
        "stocking action should empty the agent's apple stash"
    );
    assert_eq!(
        chest.count(Concept::Apple),
        2,
        "both apples should land in the chest"
    );
}

/// A hungry agent who knows about a chest containing food must close the
/// hunger goal via Take + Eat — not by harvesting fresh food. Verifies
/// the chest is enumerated as a `Take` target and that `Take`'s
/// `FromTargetContains` projection produces `(Self, Contains, Food)`.
#[test]
fn hungry_agent_with_known_stocked_chest_plans_take_then_eat() {
    use bevy::prelude::Entity;
    use worldsim::agent::actions::{ActionRegistry, ActionType, TargetCandidate};
    use worldsim::agent::brains::planner::{PlanCostContext, regressive_plan};
    use worldsim::agent::brains::thinking::TriplePattern;
    use worldsim::agent::mind::knowledge::{Quantity, Triple, setup_ontology};

    let ontology = setup_ontology();
    let mut mind = MindGraph::new(ontology);

    mind.assert(Triple::new(
        Node::Self_,
        Predicate::LocatedAt,
        Value::Tile((0, 0)),
    ));

    // Agent knows about a chest with apples in it.
    let chest = Entity::from_bits(42);
    mind.assert(Triple::new(
        Node::Entity(chest),
        Predicate::IsA,
        Value::Concept(Concept::StorageChest),
    ));
    mind.assert(Triple::new(
        Node::Entity(chest),
        Predicate::LocatedAt,
        Value::Tile((0, 0)),
    ));
    mind.assert(Triple::new(
        Node::Entity(chest),
        Predicate::Contains,
        Value::Item(Concept::Apple, 3),
    ));

    let registry = ActionRegistry::new();
    let chest_target = TargetCandidate::Entity {
        entity: chest,
        pos: Vec2::ZERO,
    };
    let take_template = registry
        .get(ActionType::Take)
        .unwrap()
        .to_template_for_target(&chest_target, &mind);
    let eat_template = registry
        .get(ActionType::Eat)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let available = vec![take_template, eat_template];

    let goal = Goal {
        conditions: vec![TriplePattern::self_has(
            Predicate::Hunger,
            Value::Quantity(Quantity::Exact(0.0)),
        )],
        priority: 90.0,
    };

    let (plan, stats) =
        regressive_plan(&mind, None, &goal, &available, &PlanCostContext::neutral());
    let plan = plan.unwrap_or_else(|| {
        panic!(
            "Planner must close hunger via Take + Eat from a known stocked chest; \
             unmet: {:?}",
            stats.best_unmet_goals
        )
    });

    assert!(
        plan.iter().any(|a| a.action_type == ActionType::Take),
        "Plan must include Take. Plan: {:?}",
        plan.iter().map(|a| a.action_type).collect::<Vec<_>>()
    );
    assert!(
        plan.iter().any(|a| a.action_type == ActionType::Eat),
        "Plan must include Eat."
    );

    let take_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::Take)
        .unwrap();
    let eat_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::Eat)
        .unwrap();
    assert!(
        take_idx < eat_idx,
        "Take must execute before Eat (take_idx={take_idx}, eat_idx={eat_idx})"
    );
}

// ─── Public-access ItemSlots configuration ───────────────────────────────────

/// Storage chest must accept Public deposits and Public extracts so any
/// agent in interaction range can stock or take. Non-storage entities use
/// OwnerOnly extract; the chest must not regress to that default.
#[test]
fn storage_chest_slot_is_public_access() {
    use worldsim::agent::item_slots::{Access, ItemSlots};

    let mut world = TestWorld::with_seed(0);
    let chest = world.spawn_storage_chest(Vec2::new(0.0, 0.0));

    let slots = world.get::<ItemSlots>(chest);
    assert_eq!(
        slots.slots.len(),
        1,
        "storage chest should have exactly one storage slot"
    );
    assert_eq!(slots.slots[0].deposit_access, Access::Public);
    assert_eq!(slots.slots[0].extract_access, Access::Public);
}
