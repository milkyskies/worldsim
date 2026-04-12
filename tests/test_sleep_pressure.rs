//! Tests for #462: dedicated sleep pressure system (wakefulness).
//!
//! Covers:
//! - Wakefulness decays while awake even when idle (adenosine accumulation)
//! - Circadian modifier: decay is faster at night
//! - Sleep restores wakefulness
//! - Low wakefulness triggers Sleepiness urgency → Sleep proposal
//! - Stamina and wakefulness are independent
//! - Fear suppresses sleep at moderate wakefulness

use bevy::math::Vec2;
use worldsim::agent::actions::{ActionType, ActiveActions};
use worldsim::testing::TestWorld;
use worldsim::world::environment::LightLevel;

// ── Decay ────────────────────────────────────────────────────────────────

#[test]
fn idle_agent_gets_sleepy_after_extended_wakefulness() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(50.0, 50.0))
        .wakefulness(1.0)
        .done()
        .build();
    let alice = agents["alice"];

    let before = world.agent_wakefulness(alice);
    assert!(
        (before - 1.0).abs() < 1e-4,
        "should start fully rested, got {before}"
    );

    // Tick for a while — not sleeping, just existing.
    world.tick(5000);

    let after = world.agent_wakefulness(alice);
    assert!(
        after < before,
        "wakefulness should decay while awake; before={before}, after={after}"
    );
    assert!(
        after > 0.0,
        "should not have fully decayed in 5000 ticks, got {after}"
    );
}

// ── Circadian ────────────────────────────────────────────────────────────

#[test]
fn wakefulness_decays_faster_at_night() {
    // Two agents start identical. One ticks under daylight, the other under
    // darkness. After the same number of ticks, the night agent should have
    // lower wakefulness.
    let (mut world_day, agents_day) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("day")
        .pos(Vec2::new(50.0, 50.0))
        .wakefulness(0.8)
        .done()
        .build();
    let day_agent = agents_day["day"];

    let (mut world_night, agents_night) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("night")
        .pos(Vec2::new(50.0, 50.0))
        .wakefulness(0.8)
        .done()
        .build();
    let night_agent = agents_night["night"];

    // Force light levels: day = 1.0 (full sun), night = 0.3 (full dark).
    world_day
        .app_mut()
        .world_mut()
        .resource_mut::<LightLevel>()
        .0 = 1.0;
    world_night
        .app_mut()
        .world_mut()
        .resource_mut::<LightLevel>()
        .0 = 0.3;

    world_day.tick(3000);
    world_night.tick(3000);

    let day_wake = world_day.agent_wakefulness(day_agent);
    let night_wake = world_night.agent_wakefulness(night_agent);

    assert!(
        night_wake < day_wake,
        "night agent should decay faster; day={day_wake:.4}, night={night_wake:.4}"
    );
}

// ── Sleep restores wakefulness ───────────────────────────────────────────

#[test]
fn sleep_restores_wakefulness() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("sleeper")
        .pos(Vec2::new(50.0, 50.0))
        .wakefulness(0.1)
        .stamina(5.0) // low stamina to help enter sleep
        .done()
        .build();
    let sleeper = agents["sleeper"];

    // Wait for Sleep to start.
    let mut entered_sleep = false;
    for _ in 0..300 {
        world.tick(1);
        if world
            .get::<ActiveActions>(sleeper)
            .contains(ActionType::Sleep)
        {
            entered_sleep = true;
            break;
        }
    }
    assert!(
        entered_sleep,
        "agent with low wakefulness should enter Sleep"
    );

    let before = world.agent_wakefulness(sleeper);

    // Let it sleep for a while.
    world.tick(2000);

    let after = world.agent_wakefulness(sleeper);
    assert!(
        after > before,
        "wakefulness should restore during sleep; before={before:.3}, after={after:.3}"
    );
}

// ── Sleepiness proposal ──────────────────────────────────────────────────

#[test]
fn low_wakefulness_triggers_sleep_proposal() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("drowsy")
        .pos(Vec2::new(50.0, 50.0))
        .wakefulness(0.15) // well below the 0.3 sleep threshold
        .stamina(5.0)
        .done()
        .build();
    let drowsy = agents["drowsy"];

    // Agent should enter Sleep within a reasonable number of ticks.
    // Urgency updates are staggered every 60 ticks, so allow enough
    // cycles for the sleepiness urgency to propagate and win arbitration.
    for _ in 0..600 {
        world.tick(1);
        if world
            .get::<ActiveActions>(drowsy)
            .contains(ActionType::Sleep)
        {
            return; // pass
        }
    }
    panic!(
        "agent with wakefulness 0.15 should enter Sleep within 600 ticks; \
         current action = {:?}, wakefulness = {:.3}",
        world.current_action(drowsy),
        world.agent_wakefulness(drowsy),
    );
}

// ── Independence from stamina ────────────────────────────────────────────

#[test]
fn stamina_and_wakefulness_are_independent() {
    // Full stamina agent with low wakefulness still gets sleepy.
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("desk_worker")
        .pos(Vec2::new(50.0, 50.0))
        .wakefulness(0.05) // very drowsy
        .stamina(100.0) // physically fresh
        .done()
        .build();
    let agent = agents["desk_worker"];

    for _ in 0..600 {
        world.tick(1);
        if world
            .get::<ActiveActions>(agent)
            .contains(ActionType::Sleep)
        {
            // The whole point: full stamina but still sleepy.
            let stamina = world.agent_aerobic(agent);
            assert!(
                stamina > 50.0,
                "stamina should still be high when sleepiness drives Sleep; got {stamina:.1}"
            );
            return; // pass
        }
    }
    panic!(
        "full-stamina agent with low wakefulness should still enter Sleep; \
         current action = {:?}, wakefulness = {:.3}",
        world.current_action(agent),
        world.agent_wakefulness(agent),
    );
}

// ── Sleep/wake cycle ─────────────────────────────────────────────────────

#[test]
fn drowsy_agent_sleeps_and_wakes_after_recovery() {
    // An agent with low wakefulness enters Sleep, recovers, and wakes up.
    // This is the core sleep/wake cycle test.
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("sleeper")
        .pos(Vec2::new(50.0, 50.0))
        .wakefulness(0.05)
        .stamina(5.0)
        .done()
        .build();
    let sleeper = agents["sleeper"];

    // Phase 1: enter Sleep.
    let mut entered = false;
    for _ in 0..600 {
        world.tick(1);
        if world
            .get::<ActiveActions>(sleeper)
            .contains(ActionType::Sleep)
        {
            entered = true;
            break;
        }
    }
    assert!(
        entered,
        "agent should enter Sleep; action={:?}, wakefulness={:.3}",
        world.current_action(sleeper),
        world.agent_wakefulness(sleeper),
    );

    // Phase 2: wakefulness recovers and agent wakes.
    // SLEEP_RESTORE_RATE is 0.00278/rate-sec. From 0.05 to 0.9 threshold
    // takes ~305 rate-seconds = ~305 game minutes = ~18300 ticks.
    // Allow generous headroom.
    let mut woke = false;
    for _ in 0..25000 {
        world.tick(1);
        if !world
            .get::<ActiveActions>(sleeper)
            .contains(ActionType::Sleep)
        {
            woke = true;
            break;
        }
    }
    let final_wake = world.agent_wakefulness(sleeper);
    assert!(
        woke,
        "agent should wake after wakefulness recovers; wakefulness={final_wake:.3}, \
         aerobic={:.1}, action={:?}",
        world.agent_aerobic(sleeper),
        world.current_action(sleeper),
    );
    assert!(
        final_wake > 0.8,
        "wakefulness should be high after waking; got {final_wake:.3}"
    );
}

// ── Natural day/night cycle ───────────────────────────────────────────────

#[test]
fn agent_sleeps_at_night_and_wakes_during_day() {
    // Full day/night cycle: agent starts at noon (tick 0) with full
    // wakefulness. By nightfall (~8 game hours later) wakefulness should
    // have decayed enough to trigger Sleep. Then wakefulness restores
    // during sleep and the agent wakes before the next noon.
    //
    // Game timing: tick 0 = 12:00 noon. Night starts ~20:00 (tick 28800).
    // One full day = 86400 ticks. We simulate 1.5 days.
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(50.0, 50.0))
        .wakefulness(1.0)
        .done()
        .build();
    let alice = agents["alice"];

    // Phase 1: tick through the afternoon and evening until Sleep starts.
    // Should happen somewhere around nightfall (tick ~28800-40000).
    let mut slept_at = None;
    for tick in 0..60000 {
        world.tick(1);
        if slept_at.is_none()
            && world
                .get::<ActiveActions>(alice)
                .contains(ActionType::Sleep)
        {
            slept_at = Some(tick);
        }
    }
    assert!(
        slept_at.is_some(),
        "agent should enter Sleep during the first night; \
         wakefulness={:.3}, action={:?}",
        world.agent_wakefulness(alice),
        world.current_action(alice),
    );
    let slept_at = slept_at.unwrap();
    // Should sleep sometime in the evening/night, not at noon.
    assert!(
        slept_at > 15000,
        "agent slept too early (tick {slept_at}); wakefulness should last most of a day"
    );

    // Phase 2: continue ticking — agent should wake up during the morning.
    let mut woke = false;
    for _ in 0..40000 {
        world.tick(1);
        if !world
            .get::<ActiveActions>(alice)
            .contains(ActionType::Sleep)
        {
            woke = true;
            break;
        }
    }
    assert!(
        woke,
        "agent should wake up after sleeping; wakefulness={:.3}, aerobic={:.1}",
        world.agent_wakefulness(alice),
        world.agent_aerobic(alice),
    );
}

// ── Fear suppression ─────────────────────────────────────────────────────

#[test]
fn safety_intent_suppresses_sleep_at_moderate_wakefulness() {
    // Agent with moderate wakefulness (0.4) should stay awake when a
    // predator is nearby — fear dampens sleepiness urgency.
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(32, 32)
        .noise_biomes(false)
        .agent("prey")
        .pos(Vec2::new(50.0, 50.0))
        .wakefulness(0.4)
        .done()
        .build();
    let prey = agents["prey"];

    // Spawn wolf within vision range.
    let _wolf = world.spawn_wolf(Vec2::new(70.0, 50.0));

    // Let the agent perceive the wolf and react.
    for _ in 0..300 {
        world.tick(1);
        assert!(
            !world.get::<ActiveActions>(prey).contains(ActionType::Sleep),
            "scared agent at wakefulness 0.4 should not sleep; \
             current action = {:?}",
            world.current_action(prey),
        );
    }
}
