//! Verifies that despawning a target entity mid-action cancels the action cleanly
//! rather than letting it tick to completion with a missing target.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::actions::registry::{ActionState, ActiveActions};
use worldsim::agent::events::{FailureReason, SimEvent};
use worldsim::testing::{AgentConfig, TestWorld};

#[test]
fn despawned_target_cancels_running_action() {
    let mut world = TestWorld::with_seed(42);

    let agent = world.spawn_agent(AgentConfig::default());
    let tree = world.spawn_apple_tree(Vec2::new(32.0, 32.0), 3);

    // Inject a long-running Harvest action targeting the tree directly into ActiveActions,
    // bypassing the brain so we can test the execution system in isolation.
    {
        let mut active = world
            .app_mut()
            .world_mut()
            .get_mut::<ActiveActions>(agent)
            .expect("agent should have ActiveActions");
        let state = ActionState::new(ActionType::Harvest, 0)
            .with_target_entity(tree)
            .with_duration(100);
        active.insert(state);
    }

    assert!(
        world
            .app()
            .world()
            .get::<ActiveActions>(agent)
            .map(|a| a.contains(ActionType::Harvest))
            .unwrap_or(false),
        "Harvest should be running before despawn"
    );

    // Despawn the target entity mid-execution.
    world.app_mut().world_mut().despawn(tree);

    // One tick should detect the despawn and cancel the action.
    world.tick(1);

    let still_harvesting = world
        .app()
        .world()
        .get::<ActiveActions>(agent)
        .map(|a| a.contains(ActionType::Harvest))
        .unwrap_or(false);
    assert!(
        !still_harvesting,
        "Harvest should be cancelled after target despawn"
    );

    let got_target_gone = world.sim_events().all().iter().any(|e| {
        matches!(
            e,
            SimEvent::ActionFailed {
                reason: FailureReason::TargetGone,
                ..
            }
        )
    });
    assert!(
        got_target_gone,
        "ActionFailed(TargetGone) should be emitted when target despawns mid-action"
    );
}
