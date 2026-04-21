//! Integration tests for the Warmth drive and WarmUp action.
//!
//! Covers the end-to-end chain the #409 issue demands:
//! urgency producer → goal formulation → intent routing → planner → action.
//! Scenario tests run on seeded TestWorld fixtures so behaviour is
//! deterministic tick by tick.

use bevy::math::Vec2;
use worldsim::agent::body::need::{Need, NeedKind};
use worldsim::agent::brains::proposal::Intent;
use worldsim::agent::brains::thinking::Goal;
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use worldsim::agent::nervous_system::urgency::UrgencySource;
use worldsim::testing::{AgentConfig, TestWorld};

// ─── Unit: the drive→intent→satisfier routing closes over Warmth ────────────

#[test]
fn warmth_urgency_routes_to_satisfy_warmth_intent() {
    // Arbitration's dedup key is `Intent::from_urgency_source`. If this
    // mapping is wrong a warmth-driven plan competes with hunger / thirst
    // plans instead of being collapsed into its own intent lane.
    assert_eq!(
        Intent::from_urgency_source(UrgencySource::Warmth),
        Intent::SatisfyWarmth
    );
}

#[test]
fn warmth_need_kind_satisfier_is_warm_up() {
    // NeedKind is the canonical identifier threaded through satiation,
    // SimEvent logs, and `goal_for_urgency`. Breaking this breaks all of
    // them at once.
    assert_eq!(
        NeedKind::Warmth.satisfier(),
        Some(worldsim::agent::actions::ActionType::WarmUp)
    );
}

#[test]
fn warmth_need_kind_satiation_gate_matches_drink() {
    // WarmUp cycles chain-fire unless there's an upper satiation gate.
    // 0.95 matches Drink / Sleep / Rest — beside a fire, warmth tops up
    // and the brain stops re-proposing.
    assert!((NeedKind::Warmth.satiation_threshold() - 0.95).abs() < 1e-6);
}

// ─── Unit: goal formulation ─────────────────────────────────────────────────

#[test]
fn warmth_urgency_formulates_warmth_body_state_goal() {
    // The rational brain's goal-for-urgency hook drives the GOAP chain.
    // For Warmth, the goal shape must be `(Self, Warmth, 100)` — a pure
    // body-state target — so WarmUp's effect closes it directly.
    let plan_memory = worldsim::agent::brains::plan_memory::PlanMemory::default();
    let ontology = worldsim::agent::mind::knowledge::setup_ontology();
    let mind = MindGraph::new(ontology);

    let goal: Goal = worldsim::agent::brains::rational::goal_for_urgency(
        UrgencySource::Warmth,
        0.8,
        &plan_memory,
        &mind,
    )
    .expect("Warmth urgency must produce a goal");

    assert_eq!(goal.conditions.len(), 1);
    let condition = &goal.conditions[0];
    assert_eq!(condition.subject, Some(Node::Self_));
    assert_eq!(condition.predicate, Some(Predicate::Warmth));
    assert!(matches!(condition.object, Some(Value::Quantity(_))));
}

// ─── Scenario: drain + recovery loop near a heat source ─────────────────────

#[test]
fn warmth_drains_when_exposed() {
    // Exposed agent (no heat, no shelter) must cool. Pin the transform
    // each tick so the AI's own wandering doesn't bounce them into a
    // non-exposure state.
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(1000.0, 1000.0),
        warmth: 0.8,
        ..Default::default()
    });
    let before = world.agent_warmth(agent);
    for _ in 0..200 {
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(1000.0, 1000.0, 0.0);
        world.tick(1);
    }
    let after = world.agent_warmth(agent);
    assert!(
        after < before,
        "exposed agent should cool (before={before:.3}, after={after:.3})"
    );
}

#[test]
fn warmth_recovers_when_next_to_campfire() {
    // Recovery branch of tick_warmth: a cold agent pinned inside a
    // campfire's HeatSource radius tops up every tick. Pinning is the
    // only way to isolate the warmth system from agent AI movement.
    let mut world = TestWorld::with_seed(0);
    world.spawn_campfire(Vec2::new(0.0, 0.0));
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.2,
        ..Default::default()
    });
    let before = world.agent_warmth(agent);
    for _ in 0..200 {
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(0.0, 0.0, 0.0);
        world.tick(1);
    }
    let after = world.agent_warmth(agent);
    assert!(
        after > before,
        "agent next to campfire should warm up (before={before:.3}, after={after:.3})"
    );
}

// ─── Scenario: warmth stays in bounds under invariants ──────────────────────

#[test]
fn warmth_never_exceeds_one() {
    // Invariant: warmth must clamp into [0, 1] regardless of how many
    // recovery ticks run next to a fire. `PhysicalNeeds::warmth` goes
    // through `Need::top_up` which clamps at 1.0, but running the sim
    // for a long stretch against a hot campfire is the real proof.
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.95,
        ..Default::default()
    });
    world.spawn_campfire(Vec2::new(0.0, 0.0));
    world.tick(500);
    let w = world.agent_warmth(agent);
    assert!(
        (0.0..=1.0).contains(&w),
        "warmth must stay in [0, 1] (got {w})"
    );
}

// ─── Unit: Need primitive respects invariants under warmth usage ────────────

#[test]
fn warmth_need_clamps_at_one() {
    let mut n = Need::new(0.9);
    n.top_up(0.3);
    assert_eq!(n.value, 1.0);
}

#[test]
fn warmth_need_clamps_at_zero() {
    let mut n = Need::new(0.1);
    n.drain(0.5);
    assert_eq!(n.value, 0.0);
}

// ─── Planner: warmth goal closes the full build chain ───────────────────────

/// Regression test for the #409 pattern-establishing chain. A cold agent
/// with wood in inventory and NO known campfire must still close the
/// planner's warmth goal by chaining WarmUp → (Self, Near, Campfire) →
/// Build. Without Build being reachable from the warmth goal, an isolated
/// cold agent that knows the recipe would wander looking for an existing
/// fire forever instead of making one.
#[test]
fn cold_agent_with_wood_plans_build_for_warmth_goal() {
    use worldsim::agent::actions::{ActionRegistry, ActionType, TargetCandidate};
    use worldsim::agent::brains::planner::{PlanCostContext, regressive_plan};
    use worldsim::agent::brains::thinking::TriplePattern;
    use worldsim::agent::mind::knowledge::{Quantity, Triple, setup_ontology};

    let ontology = setup_ontology();
    let mut mind = MindGraph::new(ontology);

    // Agent knows the recipe (Cultural knowledge seeds this at spawn).
    mind.assert(Triple::new(
        Node::Concept(Concept::Campfire),
        Predicate::Requires,
        Value::Item(Concept::Wood, 3),
    ));

    // Agent's current tile.
    mind.assert(Triple::new(
        Node::Self_,
        Predicate::LocatedAt,
        Value::Tile((0, 0)),
    ));

    // Agent is carrying the full Build recipe amount. The planner's
    // at-least quantity rule lets stored Item(Wood, n) satisfy any
    // precondition asking for <= n units.
    mind.assert(Triple::new(
        Node::Self_,
        Predicate::Contains,
        Value::Item(
            Concept::Wood,
            worldsim::constants::actions::build::CAMPFIRE_WOOD_REQUIRED,
        ),
    ));

    let registry = ActionRegistry::new();
    let build_template = registry
        .get(ActionType::Build)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let warm_up_template = registry
        .get(ActionType::WarmUp)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let available = vec![build_template, warm_up_template];

    // Warmth body-state goal — exactly what goal_for_urgency produces for
    // UrgencySource::Warmth.
    let goal = Goal {
        conditions: vec![TriplePattern::self_has(
            Predicate::Warmth,
            Value::Quantity(Quantity::Exact(100.0)),
        )],
        priority: 80.0,
    };

    let (plan, stats) = regressive_plan(&mind, &goal, &available, &PlanCostContext::neutral());
    let plan = plan.unwrap_or_else(|| {
        panic!(
            "Planner must close warmth goal via WarmUp + Build chain; unmet: {:?}",
            stats.best_unmet_goals
        )
    });

    assert!(
        plan.iter().any(|a| a.action_type == ActionType::Build),
        "Plan must include Build — a cold agent with wood but no known \
         campfire should proactively build one to satisfy Warmth.\n\
         Plan: {:?}",
        plan.iter().map(|a| a.action_type).collect::<Vec<_>>()
    );
    assert!(
        plan.iter().any(|a| a.action_type == ActionType::WarmUp),
        "Plan must include WarmUp — Build alone only produces Near-Campfire; \
         it's WarmUp that closes the warmth body-state goal."
    );

    // Execution order: Build must come before WarmUp (you can't warm up
    // at a campfire that doesn't exist yet).
    let build_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::Build)
        .unwrap();
    let warm_up_idx = plan
        .iter()
        .position(|a| a.action_type == ActionType::WarmUp)
        .unwrap();
    assert!(
        build_idx < warm_up_idx,
        "Build must execute before WarmUp (build_idx={build_idx}, warm_up_idx={warm_up_idx})",
    );
}

/// Complement to the above: when a campfire IS already known and on the
/// agent's tile, the planner does NOT plan a Build — it goes straight to
/// WarmUp. Proves the concept-near generator grounds the Near precondition
/// via the existing entity rather than always wanting to produce one.
#[test]
fn agent_on_known_campfire_plans_warm_up_without_build() {
    use bevy::prelude::Entity;
    use worldsim::agent::actions::{ActionRegistry, ActionType, TargetCandidate};
    use worldsim::agent::brains::planner::{PlanCostContext, regressive_plan};
    use worldsim::agent::brains::thinking::TriplePattern;
    use worldsim::agent::mind::knowledge::{Quantity, Triple, setup_ontology};

    let ontology = setup_ontology();
    let mut mind = MindGraph::new(ontology);

    // Agent is sitting on a known campfire entity.
    let campfire = Entity::from_bits(42);
    mind.assert(Triple::new(
        Node::Self_,
        Predicate::LocatedAt,
        Value::Tile((0, 0)),
    ));
    mind.assert(Triple::new(
        Node::Entity(campfire),
        Predicate::IsA,
        Value::Concept(Concept::Campfire),
    ));
    mind.assert(Triple::new(
        Node::Entity(campfire),
        Predicate::LocatedAt,
        Value::Tile((0, 0)),
    ));

    let registry = ActionRegistry::new();
    let build_template = registry
        .get(ActionType::Build)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let warm_up_template = registry
        .get(ActionType::WarmUp)
        .unwrap()
        .to_template_for_target(&TargetCandidate::None, &mind);
    let available = vec![build_template, warm_up_template];

    let goal = Goal {
        conditions: vec![TriplePattern::self_has(
            Predicate::Warmth,
            Value::Quantity(Quantity::Exact(100.0)),
        )],
        priority: 80.0,
    };

    let (plan, _) = regressive_plan(&mind, &goal, &available, &PlanCostContext::neutral());
    let plan = plan.expect("Planner must close warmth goal when already at a campfire");

    assert!(
        plan.iter().any(|a| a.action_type == ActionType::WarmUp),
        "Plan must include WarmUp when sitting on a known campfire"
    );
    assert!(
        !plan.iter().any(|a| a.action_type == ActionType::Build),
        "Plan must NOT include Build — a campfire already exists at the agent's tile.\n\
         Plan: {:?}",
        plan.iter().map(|a| a.action_type).collect::<Vec<_>>()
    );
}

// ─── Proximity warming is action-agnostic ────────────────────────────────

/// A cold agent pinned next to a campfire while Sleep is active still
/// gains warmth — proximity, not the WarmUp action, is the mechanism.
/// Without this, Sleep blocks WarmUp on the channel and the agent would
/// wake up just as cold as they went to bed.
/// Proximity is the warming mechanism, not the action. Pin a cold agent
/// next to a campfire without ever running WarmUp and assert they still
/// gain meaningful warmth — the same passive system that makes sleeping
/// or eating near a fire recoverable.
#[test]
fn proximity_warms_regardless_of_action() {
    let mut world = TestWorld::with_seed(0);
    world.spawn_campfire(Vec2::new(0.0, 0.0));
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.2,
        ..Default::default()
    });

    let before = world.agent_warmth(agent);
    for _ in 0..600 {
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(0.0, 0.0, 0.0);
        world.tick(1);
    }
    let after = world.agent_warmth(agent);

    assert!(
        after > before + 0.1,
        "cold agent pinned to a campfire for 600 ticks must gain meaningful warmth; \
         got before={before:.3}, after={after:.3}"
    );
}

/// WarmUp completing no longer mutates `physical.warmth` — the action
/// is an intentional stance now, proximity is the mechanism.
#[test]
fn warmup_on_complete_does_not_top_up_warmth() {
    use worldsim::agent::actions::GenericAction;
    use worldsim::agent::actions::action::WARM_UP_DEF;
    use worldsim::agent::actions::registry::{Action, CompletionContext, SpawnRequest};
    use worldsim::agent::body::metabolism::Metabolism;
    use worldsim::agent::body::needs::PhysicalNeeds;
    use worldsim::agent::item_slots::ItemSlots;
    use worldsim::agent::mind::knowledge::setup_ontology;

    let warm_up = GenericAction::new(&WARM_UP_DEF);
    let mut physical = PhysicalNeeds {
        metabolism: Metabolism::empty(),
        ..Default::default()
    };
    physical.warmth = Need::new(0.2);
    let mut inventory = ItemSlots::agent_carry();
    let mind = MindGraph::new(setup_ontology());
    let mut spawn_requests: Vec<SpawnRequest> = Vec::new();

    let before = physical.warmth.value;
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
    warm_up.on_complete(&mut ctx);

    assert!(
        (physical.warmth.value - before).abs() < f32::EPSILON,
        "WarmUp.on_complete must not mutate warmth; before={before:.3} after={:.3}",
        physical.warmth.value
    );
}

/// `WarmthAtLeast` predicate reads `physical.warmth.value` directly —
/// true once the agent crosses the threshold, false below.
#[test]
fn warmth_completion_predicate_fires_on_threshold() {
    use worldsim::agent::actions::GenericAction;
    use worldsim::agent::actions::action::WARM_UP_DEF;
    use worldsim::agent::actions::registry::Action;
    use worldsim::agent::body::needs::PhysicalNeeds;

    let warm_up = GenericAction::new(&WARM_UP_DEF);
    let mut physical = PhysicalNeeds::default();
    physical.warmth = Need::new(0.5);
    assert!(!warm_up.should_complete(&physical));
    physical.warmth = Need::new(0.95);
    assert!(warm_up.should_complete(&physical));
}

/// WarmUp injected directly into ActiveActions + pinned next to a campfire
/// runs to the self-completion threshold — not fixed-duration exit. Pre-fix,
/// the action would complete every 15 ticks with warmth barely above start;
/// post-fix, the single stance runs until warmth >= 0.9 and then ends.
#[test]
fn warmup_stance_runs_until_warmth_threshold() {
    use worldsim::agent::actions::ActionType;
    use worldsim::agent::actions::ActiveActions;
    use worldsim::agent::actions::registry::ActionState;
    use worldsim::agent::events::{SimEvent, SimEventKind};

    let mut world = TestWorld::with_seed(0);
    world.spawn_campfire(Vec2::new(0.0, 0.0));
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.2,
        ..Default::default()
    });

    world
        .get_mut::<ActiveActions>(agent)
        .insert(ActionState::new(ActionType::WarmUp, 0));

    for _ in 0..4000 {
        world.get_mut::<bevy::prelude::Transform>(agent).translation =
            bevy::prelude::Vec3::new(0.0, 0.0, 0.0);
        world.tick(1);
    }

    let completions = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent {
                    kind: SimEventKind::ActionCompleted {
                        action: ActionType::WarmUp,
                        ..
                    },
                    ..
                } if e.involves(agent)
            )
        })
        .count();

    let final_warmth = world.agent_warmth(agent);

    assert!(
        final_warmth >= 0.85,
        "agent should warm up to near-full; got {final_warmth:.3}"
    );
    // With fixed 15-tick cycles (pre-fix) we'd expect 100+ completions in
    // 4000 ticks. With self-completion at warmth >= 0.9 we expect 1 (maybe
    // 2 if the stance re-enters after drain dips it below threshold).
    assert!(
        completions <= 2,
        "WarmUp should self-complete once warmth crosses 0.9, not fire on a 15-tick loop; got {completions}"
    );
}
