//! Integration tests for the RestQuality drive and the RestInShelter /
//! BuildLeanTo / BuildHouse actions.
//!
//! Mirrors `test_warmth_drive.rs` end-to-end:
//! urgency producer → goal formulation → intent routing → planner →
//! action. Confirms the second instance of the drive-to-plan pattern
//! established in #409 (Warmth) extends cleanly to a build-shelter loop.

use bevy::math::Vec2;
use worldsim::agent::body::need::NeedKind;
use worldsim::agent::brains::proposal::Intent;
use worldsim::agent::brains::thinking::Goal;
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use worldsim::agent::nervous_system::urgency::UrgencySource;
use worldsim::testing::{AgentConfig, TestWorld};

// ─── Unit: drive→intent→satisfier routing closes over RestQuality ───────────

#[test]
fn rest_quality_urgency_routes_to_satisfy_rest_quality_intent() {
    assert_eq!(
        Intent::from_urgency_source(UrgencySource::RestQuality),
        Intent::SatisfyRestQuality
    );
}

#[test]
fn rest_quality_need_kind_satisfier_is_rest_in_shelter() {
    assert_eq!(
        NeedKind::RestQuality.satisfier(),
        Some(worldsim::agent::actions::ActionType::RestInShelter)
    );
}

#[test]
fn rest_quality_need_kind_satiation_gate_matches_warmth() {
    // 0.95 mirrors Warmth / Drink / Sleep: high enough that re-entry
    // hysteresis prevents chain-fire when the body dips slightly below
    // the auto-complete threshold.
    assert!((NeedKind::RestQuality.satiation_threshold() - 0.95).abs() < 1e-6);
}

// ─── Unit: goal formulation ─────────────────────────────────────────────────

#[test]
fn rest_quality_urgency_formulates_rest_quality_body_state_goal() {
    let plan_memory = worldsim::agent::brains::plan_memory::PlanMemory::default();
    let ontology = worldsim::agent::mind::knowledge::setup_ontology();
    let mind = MindGraph::new(ontology);

    let goal: Goal = worldsim::agent::brains::rational::goal_for_urgency(
        UrgencySource::RestQuality,
        0.8,
        &plan_memory,
        &mind,
    )
    .expect("RestQuality urgency must produce a goal");

    assert_eq!(goal.conditions.len(), 1);
    let condition = &goal.conditions[0];
    assert_eq!(condition.subject, Some(Node::Self_));
    assert_eq!(condition.predicate, Some(Predicate::RestQuality));
    assert!(matches!(condition.object, Some(Value::Quantity(_))));
}

// ─── Scenario: drain + recovery loop near a lean-to ─────────────────────────

#[test]
fn awake_agent_drains_rest_quality_baseline() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(1000.0, 1000.0),
        rest_quality: 0.8,
        ..Default::default()
    });
    let before = world.agent_rest_quality(agent);
    for _ in 0..200 {
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(1000.0, 1000.0, 0.0);
        world.tick(1);
    }
    let after = world.agent_rest_quality(agent);
    assert!(
        after < before,
        "awake agent should slowly lose rest-quality (before={before:.3}, after={after:.3})"
    );
}

#[test]
fn agent_near_lean_to_recovers_rest_quality() {
    let mut world = TestWorld::with_seed(0);
    world.spawn_lean_to(Vec2::new(0.0, 0.0));
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        rest_quality: 0.2,
        ..Default::default()
    });

    let before = world.agent_rest_quality(agent);
    for _ in 0..600 {
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(0.0, 0.0, 0.0);
        world.tick(1);
    }
    let after = world.agent_rest_quality(agent);

    assert!(
        after > before,
        "agent near a lean-to should gain rest-quality \
         (before={before:.3}, after={after:.3})"
    );
}

#[test]
fn house_recovers_rest_quality_faster_than_lean_to() {
    fn run(spawn_shelter: impl Fn(&mut TestWorld, Vec2)) -> f32 {
        let mut world = TestWorld::with_seed(0);
        spawn_shelter(&mut world, Vec2::new(0.0, 0.0));
        let agent = world.spawn_agent(AgentConfig {
            pos: Vec2::new(0.0, 0.0),
            rest_quality: 0.2,
            ..Default::default()
        });
        for _ in 0..200 {
            world.get_mut::<bevy::prelude::Transform>(agent).translation =
                bevy::prelude::Vec3::new(0.0, 0.0, 0.0);
            world.tick(1);
        }
        world.agent_rest_quality(agent)
    }

    let after_lean_to = run(|w, p| {
        w.spawn_lean_to(p);
    });
    let after_house = run(|w, p| {
        w.spawn_house(p);
    });

    assert!(
        after_house > after_lean_to,
        "house (PROTECTION 2.5) must recover rest-quality faster than \
         a lean-to (PROTECTION 1.5); got house={after_house:.3} lean_to={after_lean_to:.3}"
    );
}

#[test]
fn rest_quality_never_exceeds_one() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        rest_quality: 0.95,
        ..Default::default()
    });
    world.spawn_lean_to(Vec2::new(0.0, 0.0));
    world.tick(500);
    let rq = world.agent_rest_quality(agent);
    assert!(
        (0.0..=1.0).contains(&rq),
        "rest-quality must stay in [0, 1] (got {rq})"
    );
}

// ─── Planner: rest-quality goal closes the build-shelter chain ──────────────

/// A poorly-rested agent with wood in inventory and NO known lean-to must
/// close the planner's RestQuality goal by chaining
/// RestInShelter → (Self, Near, LeanTo) → BuildLeanTo. Mirrors the Warmth
/// build-chain regression test (#409).
#[test]
fn unrested_agent_with_wood_plans_build_lean_to_for_rest_quality_goal() {
    use worldsim::agent::actions::{ActionRegistry, ActionType, TargetCandidate};
    use worldsim::agent::brains::planner::{PlanCostContext, regressive_plan};
    use worldsim::agent::brains::thinking::TriplePattern;
    use worldsim::agent::mind::knowledge::{Quantity, Triple, setup_ontology};

    let ontology = setup_ontology();
    let mut mind = MindGraph::new(ontology);

    // Cultural knowledge: lean-to recipe.
    mind.assert(Triple::new(
        Node::Concept(Concept::LeanTo),
        Predicate::Requires,
        Value::Item(
            Concept::Wood,
            worldsim::constants::actions::build::LEAN_TO_WOOD_REQUIRED,
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
            worldsim::constants::actions::build::LEAN_TO_WOOD_REQUIRED,
        ),
    ));

    let registry = ActionRegistry::new();
    let build_lean_to = registry
        .get(ActionType::BuildLeanTo)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let rest_in_shelter = registry
        .get(ActionType::RestInShelter)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let available = vec![build_lean_to, rest_in_shelter];

    let goal = Goal {
        conditions: vec![TriplePattern::self_has(
            Predicate::RestQuality,
            Value::Quantity(Quantity::Exact(100.0)),
        )],
        priority: 80.0,
    };

    let (plan, stats) = regressive_plan(&mind, &goal, &available, &PlanCostContext::neutral());
    let plan = plan.unwrap_or_else(|| {
        panic!(
            "Planner must close RestQuality goal via RestInShelter + BuildLeanTo chain; unmet: {:?}",
            stats.best_unmet_goals
        )
    });

    assert!(
        plan.iter()
            .any(|a| a.action_type == ActionType::BuildLeanTo),
        "Plan must include BuildLeanTo. Plan: {:?}",
        plan.iter().map(|a| a.action_type).collect::<Vec<_>>()
    );
    assert!(
        plan.iter()
            .any(|a| a.action_type == ActionType::RestInShelter),
        "Plan must include RestInShelter — BuildLeanTo alone only produces \
         Near-LeanTo; RestInShelter closes the rest-quality body-state goal."
    );

    let build_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::BuildLeanTo)
        .unwrap();
    let rest_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::RestInShelter)
        .unwrap();
    assert!(
        build_idx < rest_idx,
        "BuildLeanTo must execute before RestInShelter (build_idx={build_idx}, \
         rest_idx={rest_idx})"
    );
}

/// House requires BOTH wood AND stone — partial materials mean the planner
/// cannot ground BuildHouse's preconditions and therefore cannot use it as
/// a satisfier for rest-quality. Documents the AND semantics of the
/// preconditions slice.
#[test]
fn agent_with_only_wood_cannot_plan_build_house() {
    use worldsim::agent::actions::{ActionRegistry, ActionType, TargetCandidate};
    use worldsim::agent::brains::planner::{PlanCostContext, regressive_plan};
    use worldsim::agent::brains::thinking::TriplePattern;
    use worldsim::agent::mind::knowledge::{Triple, setup_ontology};

    let ontology = setup_ontology();
    let mut mind = MindGraph::new(ontology);

    // Cultural knowledge: house recipe.
    mind.assert(Triple::new(
        Node::Concept(Concept::House),
        Predicate::Requires,
        Value::Item(
            Concept::Wood,
            worldsim::constants::actions::build::HOUSE_WOOD_REQUIRED,
        ),
    ));
    mind.assert(Triple::new(
        Node::Concept(Concept::House),
        Predicate::Requires,
        Value::Item(
            Concept::Stone,
            worldsim::constants::actions::build::HOUSE_STONE_REQUIRED,
        ),
    ));

    mind.assert(Triple::new(
        Node::Self_,
        Predicate::LocatedAt,
        Value::Tile((0, 0)),
    ));

    // Plenty of wood, no stone.
    mind.assert(Triple::new(
        Node::Self_,
        Predicate::Contains,
        Value::Item(
            Concept::Wood,
            worldsim::constants::actions::build::HOUSE_WOOD_REQUIRED,
        ),
    ));

    let registry = ActionRegistry::new();
    let build_house = registry
        .get(ActionType::BuildHouse)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let rest_in_shelter = registry
        .get(ActionType::RestInShelter)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);

    // Goal that BuildHouse would satisfy if its preconditions could ground.
    let goal = Goal {
        conditions: vec![TriplePattern::self_has(
            Predicate::Near,
            Value::Concept(Concept::House),
        )],
        priority: 80.0,
    };

    let (plan, _) = regressive_plan(
        &mind,
        &goal,
        &[build_house, rest_in_shelter],
        &PlanCostContext::neutral(),
    );

    assert!(
        plan.is_none(),
        "Planner must not close a House goal without stone in inventory"
    );
}

// ─── On-complete invariants ──────────────────────────────────────────────────

/// Like WarmUp, RestInShelter is an intentional stance — recovery happens
/// via passive shelter proximity in `tick_rest_quality`, not via the
/// action's `on_complete`. Validates the action does not double-credit
/// the body state.
#[test]
fn rest_in_shelter_on_complete_does_not_top_up_rest_quality() {
    use worldsim::agent::actions::GenericAction;
    use worldsim::agent::actions::action::REST_IN_SHELTER_DEF;
    use worldsim::agent::actions::registry::{Action, CompletionContext, SpawnRequest};
    use worldsim::agent::body::metabolism::Metabolism;
    use worldsim::agent::body::need::Need;
    use worldsim::agent::body::needs::PhysicalNeeds;
    use worldsim::agent::item_slots::ItemSlots;
    use worldsim::agent::mind::knowledge::setup_ontology;

    let action = GenericAction::new(&REST_IN_SHELTER_DEF);
    let mut physical = PhysicalNeeds {
        metabolism: Metabolism::empty(),
        ..Default::default()
    };
    physical.rest_quality = Need::new(0.2);
    let mut inventory = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let mut spawn_requests: Vec<SpawnRequest> = Vec::new();

    let before = physical.rest_quality.value;
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
        (physical.rest_quality.value - before).abs() < f32::EPSILON,
        "RestInShelter.on_complete must not mutate rest_quality"
    );
}

#[test]
fn rest_quality_completion_predicate_fires_on_threshold() {
    use worldsim::agent::actions::GenericAction;
    use worldsim::agent::actions::action::REST_IN_SHELTER_DEF;
    use worldsim::agent::actions::registry::Action;
    use worldsim::agent::body::need::Need;
    use worldsim::agent::body::needs::PhysicalNeeds;

    let action = GenericAction::new(&REST_IN_SHELTER_DEF);
    let mut physical = PhysicalNeeds {
        rest_quality: Need::new(0.5),
        ..Default::default()
    };
    assert!(!action.should_complete(&physical));
    physical.rest_quality = Need::new(0.95);
    assert!(action.should_complete(&physical));
}

// ─── Lean-to durability invariants ───────────────────────────────────────────

/// Lean-to has `Durability` and decays each tick; with `decay_rate > 0`
/// `durability_system` despawns it once `current` reaches zero.
#[test]
fn lean_to_with_zero_durability_is_despawned() {
    use worldsim::world::property::Durability;

    let mut world = TestWorld::with_seed(0);
    let lean_to = world.spawn_lean_to(Vec2::new(0.0, 0.0));

    // Crush durability so the next few ticks tip it past zero.
    {
        let mut durability = world.get_mut::<Durability>(lean_to);
        durability.current = 0.05;
    }

    world.tick(2000);

    assert!(
        !world.entity_exists(lean_to),
        "lean-to must despawn once durability hits 0"
    );
}
