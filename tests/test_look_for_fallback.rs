//! End-to-end fallback: hungry agent with an empty MindGraph must run
//! `LookFor`, not `Explore` or idle.

use bevy::math::Vec2;
use worldsim::agent::actions::ActionType;
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::testing::TestWorld;

#[test]
fn hungry_agent_with_no_known_food_runs_look_for() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(64.0, 64.0))
        .metabolism(Metabolism::at_urgency(0.95))
        .done()
        .build();

    let alice = agents["alice"];

    world.tick(120);

    let action = world
        .current_action(alice)
        .expect("alice must have a running action after 120 ticks");

    assert_eq!(
        action,
        ActionType::LookFor,
        "hungry agent with empty MindGraph must run LookFor; got {action:?}"
    );
}
