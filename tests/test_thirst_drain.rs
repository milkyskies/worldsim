//! #475: Hydration drain — agents get thirsty over time without drinking.

use bevy::math::Vec2;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::events::SimEvent;
use worldsim::testing::{AgentConfig, TestWorld};
use worldsim::world::map::TileType;

/// Hydration must decrease each tick. After 10 000 ticks with no water
/// reachable, the agent should have lost at least 5 hydration units.
#[test]
fn hydration_decreases_over_time_without_drinking() {
    let mut world = TestWorld::with_seed(42);

    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(200.0, 200.0),
        hydration: 100.0,
        ..Default::default()
    });

    let before = world.get::<PhysicalNeeds>(agent).hydration;

    // 10 000 ticks × (1/60) s × 0.035 /s ≈ 5.8 hydration units drained.
    world.tick(10_000);

    let after = world.get::<PhysicalNeeds>(agent).hydration;
    assert!(
        after < before - 5.0,
        "hydration should drain over time, got {before:.1} → {after:.1}"
    );
}

/// A thirsty agent placed adjacent to a water tile must plan and execute
/// Drink, reducing their thirst before the test window expires.
#[test]
fn thirsty_agent_near_water_plans_to_drink() {
    let mut world = TestWorld::with_seed(42);

    // Water tile adjacent to the agent's starting tile.
    world.set_tile(3, 2, TileType::ShallowWater);

    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(40.0, 40.0),
        hydration: 20.0,
        ..Default::default()
    });

    world.tick(400);

    let events = world.sim_events();
    let drank = events.all().iter().any(|e| {
        matches!(
            e,
            SimEvent::ActionCompleted {
                agent: a,
                action: worldsim::agent::actions::ActionType::Drink,
                ..
            } if *a == agent
        )
    });

    assert!(
        drank,
        "thirsty agent near water should have completed a Drink action; \
         final thirst = {:.1}",
        world.agent_thirst(agent)
    );
}
