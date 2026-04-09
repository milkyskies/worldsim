//! Verifies that when the brain specifies a target position for a movement
//! action, the execution system honours that position rather than silently
//! discarding or overwriting it.

use bevy::prelude::*;
use worldsim::agent::TargetPosition;
use worldsim::agent::actions::ActionType;
use worldsim::agent::actions::registry::ActiveActions;
use worldsim::agent::brains::proposal::{BrainPowers, BrainState, BrainType};
use worldsim::agent::brains::thinking::ActionTemplate;
use worldsim::testing::{AgentConfig, TestWorld};

fn make_walk_template(target: Vec2) -> ActionTemplate {
    ActionTemplate {
        name: "Walk".into(),
        action_type: ActionType::Walk,
        target_entity: None,
        target_position: Some(target),
        preconditions: vec![],
        effects: vec![],
        consumes: vec![],
        base_cost: 1.0,
    }
}

#[test]
fn brain_walk_target_is_used_by_execution() {
    let brain_target = Vec2::new(200.0, 200.0);

    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });

    // Inject a BrainState that specifies a Walk to brain_target, bypassing
    // the actual brain cycle so we can test start_actions in isolation.
    {
        let mut brain_state = world
            .app_mut()
            .world_mut()
            .get_mut::<BrainState>(agent)
            .expect("agent should have BrainState");
        brain_state.chosen_actions = vec![make_walk_template(brain_target)];
        brain_state.winner = Some(BrainType::Rational);
        brain_state.powers = BrainPowers {
            survival: 0.0,
            emotional: 0.0,
            rational: 1.0,
        };
    }

    // One tick: start_actions should admit the Walk and set TargetPosition.
    world.tick(1);

    let target_pos = world
        .app()
        .world()
        .get::<TargetPosition>(agent)
        .expect("agent should have TargetPosition");

    assert_eq!(
        target_pos.0,
        Some(brain_target),
        "execution system should use the brain's Walk target, not discard it"
    );

    let is_walking = world
        .app()
        .world()
        .get::<ActiveActions>(agent)
        .map(|a| a.contains(ActionType::Walk))
        .unwrap_or(false);

    assert!(is_walking, "Walk action should be running after one tick");
}
