//! Adventure-mode foundation (#767): the `PlayerControlled` marker must
//! suppress AI brain decisions for the marked entity while leaving every
//! other system (biology, perception, action execution) untouched.
//!
//! Two invariants together guarantee "the AI no longer drives this agent":
//!
//! 1. The rational planner stays silent — `last_plan_attempt` is empty
//!    even after enough ticks for the planner to have fired many times.
//! 2. Arbitration leaves `BrainState` empty — proposals, chosen_actions,
//!    and winner all stay at their cleared defaults.
//!
//! And one liveness invariant — the marker must NOT silence the rest of
//! the simulation:
//!
//! 3. Metabolism keeps draining and perception keeps writing beliefs.
//!    The agent is still alive, just not steered by the AI.

use bevy::prelude::*;
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::brains::plan_memory::PlanMemory;
use worldsim::agent::brains::proposal::BrainState;
use worldsim::agent::mind::knowledge::{MindGraph, Node};
use worldsim::agent::player::PlayerControlled;
use worldsim::testing::{AgentConfig, TestWorld};

/// AI suppression: with the marker, neither the planner nor arbitration
/// runs for the agent. Possess at spawn time so we can prove "AI never
/// touched it" rather than the weaker "AI stopped touching it."
#[test]
fn possessed_agent_ai_never_plans_or_arbitrates() {
    let mut world = TestWorld::with_seed(42);

    // High hunger + nearby food — would make the AI plan within a few
    // brain ticks. If the marker fails, we'll see plan_attempts > 0.
    world.spawn_apple_tree(Vec2::new(120.0, 0.0), 5);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::ZERO,
        metabolism: Metabolism::at_urgency(0.8),
        ..Default::default()
    });

    // Possess immediately — before the brain has any chance to run.
    world
        .app_mut()
        .world_mut()
        .entity_mut(agent)
        .insert(PlayerControlled);

    // Tick well past the default 6-tick brain interval so we're sure
    // arbitration *would* have fired several times if the marker
    // weren't suppressing it. 120 / 6 = 20 missed brain ticks.
    world.tick(120);

    // Invariant 1: planner stayed silent.
    let attempts = &world.get::<PlanMemory>(agent).last_plan_attempt;
    assert!(
        attempts.is_empty(),
        "rational planner must not attempt any plans for a PlayerControlled agent — \
         got {attempts:?}"
    );

    // Invariant 2: arbitration left BrainState at its empty default.
    let brain_state = world.get::<BrainState>(agent);
    assert!(
        brain_state.proposals.is_empty(),
        "BrainState.proposals must stay empty — got {:?}",
        brain_state.proposals
    );
    assert!(
        brain_state.chosen_actions.is_empty(),
        "BrainState.chosen_actions must stay empty — got {:?}",
        brain_state.chosen_actions
    );
    assert!(
        brain_state.winner.is_none(),
        "BrainState.winner must stay None — got {:?}",
        brain_state.winner
    );
}

/// The marker is scoped to *decision-making*. Biology and perception
/// still run — the player's body still gets thirsty, still sees the
/// world, still ages. Without this guarantee we'd be running a frozen
/// agent, not a controllable one.
#[test]
fn possessed_agent_biology_and_perception_still_tick() {
    let mut world = TestWorld::with_seed(42);
    let tree = world.spawn_apple_tree(Vec2::new(60.0, 0.0), 5);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::ZERO,
        metabolism: Metabolism::at_urgency(0.3),
        ..Default::default()
    });

    world
        .app_mut()
        .world_mut()
        .entity_mut(agent)
        .insert(PlayerControlled);

    // Hydration is the cleanest probe: `tick_metabolism` drains it
    // monotonically with no top-up source (unlike glucose, which is
    // refilled from stomach digestion and reserve mobilization).
    let hydration_before = world.get::<PhysicalNeeds>(agent).hydration.value;

    world.tick(120);

    let hydration_after = world.get::<PhysicalNeeds>(agent).hydration.value;
    assert!(
        hydration_after < hydration_before,
        "PlayerControlled must not silence biology — hydration should still drain \
         (before={hydration_before}, after={hydration_after})"
    );

    // Perception kept running — the nearby tree should be in the
    // MindGraph by now.
    let mind = world.get::<MindGraph>(agent);
    let saw_tree = mind.iter().any(|t| t.subject == Node::Entity(tree));
    assert!(
        saw_tree,
        "PlayerControlled must not silence perception — agent should still write \
         beliefs about visible entities into MindGraph"
    );
}

/// Player input drives the agent end-to-end: a held WASD key produces a
/// Walk action that goes through the existing execution pipeline and
/// actually moves the agent. Exercises the full bridge: read input →
/// build Walk template → start_actions admits it → movement system
/// updates the Transform.
#[test]
fn held_d_key_walks_player_east() {
    use bevy::input::ButtonInput;

    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(200.0, 200.0),
        ..Default::default()
    });

    world
        .app_mut()
        .world_mut()
        .entity_mut(agent)
        .insert(PlayerControlled);

    // TestWorld's MinimalPlugins doesn't include InputPlugin, so we
    // install the keyboard resource by hand.
    let mut input = ButtonInput::<KeyCode>::default();
    input.press(KeyCode::KeyD);
    world.app_mut().world_mut().insert_resource(input);

    let start_x = world.get::<Transform>(agent).translation.x;

    // Long enough for the agent to traverse multiple tiles, so we're
    // observing actual movement and not a sub-tile tremor.
    world.tick(120);

    let end_x = world.get::<Transform>(agent).translation.x;
    assert!(
        end_x > start_x + 16.0,
        "holding D should walk the player meaningfully east \
         (start_x={start_x}, end_x={end_x})"
    );
    let end_y = world.get::<Transform>(agent).translation.y;
    assert!(
        (end_y - 200.0).abs() < 8.0,
        "horizontal-only input should keep y near the start row \
         (start_y=200, end_y={end_y})"
    );
}

/// `release` reverses possession: the marker is removed and the AI
/// resumes deciding for the agent. Without this we'd have a one-way
/// ticket — once possessed, always possessed.
#[test]
fn release_restores_ai_control() {
    let mut world = TestWorld::with_seed(42);
    world.spawn_apple_tree(Vec2::new(120.0, 0.0), 5);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::ZERO,
        metabolism: Metabolism::at_urgency(0.8),
        ..Default::default()
    });

    // Possess, tick, then release.
    world
        .app_mut()
        .world_mut()
        .entity_mut(agent)
        .insert(PlayerControlled);
    world.tick(60);
    world
        .app_mut()
        .world_mut()
        .entity_mut(agent)
        .remove::<PlayerControlled>();

    // After release the AI should resume — give it enough ticks for
    // the next brain cycle plus perception/wakeup propagation.
    world.tick(120);

    let attempts = &world.get::<PlanMemory>(agent).last_plan_attempt;
    assert!(
        !attempts.is_empty(),
        "after PlayerControlled is removed, the rational planner must resume \
         attempting plans — got an empty last_plan_attempt map"
    );
}
