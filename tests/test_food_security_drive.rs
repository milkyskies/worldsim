//! Integration tests for the FoodSecurity drive and the
//! CheckOnStockpile / BuildStorageChest actions. Mirrors
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
fn food_security_need_kind_satisfier_is_check_on_stockpile() {
    assert_eq!(
        NeedKind::FoodSecurity.satisfier(),
        Some(worldsim::agent::actions::ActionType::CheckOnStockpile)
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
fn agent_near_storage_chest_recovers_food_security() {
    let mut world = TestWorld::with_seed(0);
    world.spawn_storage_chest(Vec2::new(0.0, 0.0));
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
        "agent near a storage chest should gain food-security \
         (before={before:.3}, after={after:.3})"
    );
}

#[test]
fn food_security_never_exceeds_one() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        food_security: 0.95,
        ..Default::default()
    });
    world.spawn_storage_chest(Vec2::new(0.0, 0.0));
    world.tick(500);
    let fs = world.agent_food_security(agent);
    assert!(
        (0.0..=1.0).contains(&fs),
        "food-security must stay in [0, 1] (got {fs})"
    );
}

// ─── Planner: food-security goal closes the build-chest chain ───────────────

/// Insecure agent with wood + no known chest must close the FoodSecurity
/// goal by chaining CheckOnStockpile -> (Self, Near, StorageChest) ->
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
    mind.assert(Triple::new(
        Node::Self_,
        Predicate::Contains,
        Value::Item(
            Concept::Wood,
            worldsim::constants::actions::build::STORAGE_CHEST_WOOD_REQUIRED,
        ),
    ));

    let registry = ActionRegistry::new();
    let build_chest = registry
        .get(ActionType::BuildStorageChest)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let check_stockpile = registry
        .get(ActionType::CheckOnStockpile)
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

    let (plan, stats) = regressive_plan(&mind, &goal, &available, &PlanCostContext::neutral());
    let plan = plan.unwrap_or_else(|| {
        panic!(
            "Planner must close FoodSecurity goal via CheckOnStockpile + \
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
        plan.iter()
            .any(|a| a.action_type == ActionType::CheckOnStockpile),
        "Plan must include CheckOnStockpile."
    );

    let build_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::BuildStorageChest)
        .unwrap();
    let check_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::CheckOnStockpile)
        .unwrap();
    assert!(
        build_idx < check_idx,
        "BuildStorageChest must execute before CheckOnStockpile"
    );
}

// ─── On-complete invariants ──────────────────────────────────────────────────

#[test]
fn check_on_stockpile_on_complete_does_not_top_up_food_security() {
    use worldsim::agent::actions::GenericAction;
    use worldsim::agent::actions::action::CHECK_ON_STOCKPILE_DEF;
    use worldsim::agent::actions::registry::{Action, CompletionContext, SpawnRequest};
    use worldsim::agent::body::metabolism::Metabolism;
    use worldsim::agent::body::need::Need;
    use worldsim::agent::body::needs::PhysicalNeeds;
    use worldsim::agent::item_slots::ItemSlots;
    use worldsim::agent::mind::knowledge::setup_ontology;

    let action = GenericAction::new(&CHECK_ON_STOCKPILE_DEF);
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
        "CheckOnStockpile.on_complete must not mutate food_security"
    );
}

#[test]
fn food_security_completion_predicate_fires_on_threshold() {
    use worldsim::agent::actions::GenericAction;
    use worldsim::agent::actions::action::CHECK_ON_STOCKPILE_DEF;
    use worldsim::agent::actions::registry::Action;
    use worldsim::agent::body::need::Need;
    use worldsim::agent::body::needs::PhysicalNeeds;

    let action = GenericAction::new(&CHECK_ON_STOCKPILE_DEF);
    let mut physical = PhysicalNeeds {
        food_security: Need::new(0.5),
        ..Default::default()
    };
    assert!(!action.should_complete(&physical));
    physical.food_security = Need::new(0.95);
    assert!(action.should_complete(&physical));
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
