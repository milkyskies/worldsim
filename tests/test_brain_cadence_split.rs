//! Regression tests for the per-responsibility cadence split (#424).
//!
//! Two invariants fall out of the split:
//!
//! 1. `BrainState.winner` (refreshed every tick by `arbitrate_every_tick`)
//!    must agree with what `ActiveActions` is actually running — staleness
//!    between the brain's recorded decision and the body's current action
//!    should never exceed 1 tick.
//! 2. The expensive regressive planner (heavy GOAP search) must NOT fire
//!    while a live plan already covers the current goal. `needs_replan_for`
//!    is the gate that makes this true.
//!
//! Together these guard the refactor's value prop: keep cheap things
//! fresh, run expensive things only when needed.

use bevy::prelude::*;
use worldsim::agent::actions::{ActionRegistry, ActionType, ActiveActions};
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::brains::plan_memory::{PlanMemory, PlanSource, PlanState};
use worldsim::agent::brains::proposal::{BrainState, BrainType};
use worldsim::agent::mind::knowledge::{
    Concept, Metadata, Node as MindNode, Predicate, Triple, Value,
};
use worldsim::testing::{AgentConfig, TestWorld};

/// Arbitration runs every tick, so the brain's recorded winner and the
/// body's actually-running primary action should never disagree for
/// more than one tick. Pre-#424 these could drift up to 60 ticks apart
/// because arbitration was staggered.
#[test]
fn brain_state_winner_agrees_with_active_actions_every_tick() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());
    world.spawn_apple_tree(Vec2::new(100.0, 100.0), 5);

    // Let the agent's brains settle into a steady decision.
    world.tick(10);

    // Sample the invariant across many ticks. Every sample must satisfy
    // "the primary running action type is present in chosen_actions
    // (or chosen_actions is empty)." The inverse — a stale decision
    // that differs from ActiveActions for multiple ticks — is the bug
    // #424 closes.
    let registry_snapshot = ActionRegistry::new();
    for _ in 0..30 {
        world.tick(1);

        let app = world.app();
        let brain_state = app
            .world()
            .get::<BrainState>(agent)
            .expect("agent should have BrainState");
        let active = app
            .world()
            .get::<ActiveActions>(agent)
            .expect("agent should have ActiveActions");

        let primary_type = active.primary(&registry_snapshot).map(|s| s.action_type);
        let chosen_types: Vec<ActionType> = brain_state
            .chosen_actions
            .iter()
            .map(|a| a.action_type)
            .collect();

        if let Some(primary) = primary_type
            && primary != ActionType::Idle
        {
            assert!(
                chosen_types.contains(&primary) || chosen_types.is_empty(),
                "primary running action {primary:?} must appear in BrainState.chosen_actions \
                 (or chosen_actions is empty between decisions) — got chosen={chosen_types:?}",
            );
        }
    }
}

/// `regressive_plan` — the expensive GOAP search — must stay silent
/// once a live plan covers the current CNS goal. Exercise the natural
/// flow: a hungry agent with pre-seeded knowledge of an apple tree
/// forms one plan, then keeps executing it without the planner firing
/// again until the plan completes or is invalidated.
///
/// `plans_generated_total` counts every call into `regressive_plan`.
/// After the first plan is generated, it must not grow across an
/// entire cooldown window while the walk-step is still in flight.
#[test]
fn regressive_planner_skipped_when_live_plan_covers_goal() {
    let mut world = TestWorld::with_seed(42);

    // Spawn an apple tree far from the agent's start position so the
    // Walk step stays in flight for the entire observation window —
    // the plan stays Executing, `needs_replan_for` stays false, and
    // the planner should stay silent.
    let tree = world.spawn_apple_tree(Vec2::new(600.0, 600.0), 10);

    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        metabolism: Metabolism::at_urgency(0.8),
        knowledge: vec![Triple::with_meta(
            MindNode::Entity(tree),
            Predicate::Contains,
            Value::Item(Concept::Apple, 5),
            Metadata::semantic(0),
        )],
        ..Default::default()
    });

    // Let CNS formulate the hunger goal and the planner form its first
    // plan. One cooldown window (~60 ticks) is enough for the planner
    // to fire.
    world.tick(70);

    let baseline = world.get::<PlanMemory>(agent).plans_generated_total;
    assert!(
        baseline >= 1,
        "agent should have generated at least one plan after a cooldown window \
         (plans_generated_total={baseline})",
    );
    let has_live_plan = world.get::<PlanMemory>(agent).plans.iter().any(|p| {
        matches!(p.source, PlanSource::Brain(BrainType::Rational))
            && !p.steps.is_empty()
            && p.state == PlanState::Executing
    });
    assert!(
        has_live_plan,
        "agent should hold a live Rational-sourced Executing plan after first cooldown"
    );

    // Tick another full cooldown. With the Walk step still in flight
    // the plan covers the goal the whole time — `needs_replan_for`
    // stays false and `regressive_plan` must not be invoked.
    world.tick(60);

    let after = world.get::<PlanMemory>(agent).plans_generated_total;
    assert_eq!(
        after, baseline,
        "regressive_plan must stay silent while a live plan covers the current goal \
         (baseline={baseline}, after={after})",
    );
}
