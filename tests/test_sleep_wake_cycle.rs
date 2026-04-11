//! Regression test for #352: an exhausted agent must enter Sleep and then
//! leave it once rested. Drives the full brain + execution loop so the
//! WakeUp deadlock would cause the second phase to loop forever.

use bevy::math::Vec2;
use worldsim::agent::actions::{ActionType, ActiveActions};
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::testing::TestWorld;

#[test]
fn exhausted_agent_sleeps_and_then_wakes_once_rested() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("sleeper")
        .pos(Vec2::new(50.0, 50.0))
        .stamina(5.0)
        .done()
        .build();
    let sleeper = agents["sleeper"];

    // Phase 1: the agent should choose to sleep within a handful of ticks
    // because their aerobic reserve is critically low.
    let mut slept = false;
    for _ in 0..200 {
        world.tick(1);
        if world
            .get::<ActiveActions>(sleeper)
            .contains(ActionType::Sleep)
        {
            slept = true;
            break;
        }
    }
    assert!(
        slept,
        "exhausted agent should enter Sleep within 200 ticks; current action: {:?}",
        world.current_action(sleeper),
    );

    // Phase 2: the agent must actually wake up once stamina recovers. Before
    // the fix this loop would run forever because WakeUp could never preempt
    // uninterruptible Sleep. Cap the loop well above the expected recovery
    // time (Sleep restores aerobic at +20/s, WAKE_STAMINA_THRESHOLD = 90, so
    // ~5 seconds of sim time at minimum, plus the 30-tick WakeUp transition).
    let mut woke = false;
    for _ in 0..5_000 {
        world.tick(1);
        if !world
            .get::<ActiveActions>(sleeper)
            .contains(ActionType::Sleep)
        {
            woke = true;
            break;
        }
    }

    let aerobic = world.get::<PhysicalNeeds>(sleeper).stamina.aerobic;
    assert!(
        woke,
        "agent should leave Sleep after stamina recovers; final aerobic = {aerobic:.1}",
    );
}
