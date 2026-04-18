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
