//! Integration tests for action location-preference (#650).
//!
//! Sleep's location_preference scorer combines warmth + social, so at
//! bedtime a cold sleepy agent pre-empts `Sleep` with a `Walk` toward
//! the best-scoring nearby tile (near a fire, near companions, etc.),
//! then fires Sleep once positioned.

use bevy::math::Vec2;
use worldsim::agent::actions::ActionType;
use worldsim::agent::body::need::Need;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::testing::{AgentConfig, TestWorld};

fn agent_pos(world: &TestWorld, agent: bevy::prelude::Entity) -> Vec2 {
    world
        .app()
        .world()
        .get::<bevy::prelude::Transform>(agent)
        .unwrap()
        .translation
        .truncate()
}

fn pin_sleepy_and_cold(
    world: &mut TestWorld,
    agent: bevy::prelude::Entity,
    warmth: f32,
    wakefulness: f32,
) {
    let mut needs = world.get_mut::<PhysicalNeeds>(agent);
    needs.warmth = Need::new(warmth);
    needs.wakefulness.set(wakefulness);
}

fn sleep_fired(world: &TestWorld, agent: bevy::prelude::Entity) -> bool {
    world.sim_events().all().iter().any(|ev| {
        matches!(
            ev,
            SimEvent {
                kind: SimEventKind::ActionStarted {
                    agent: a,
                    action: ActionType::Sleep,
                    ..
                },
                ..
            } if *a == agent
        )
    })
}

/// A cold sleepy agent 80 px from a fire should end up near the fire
/// AND fire Sleep during the test window. Currently (pre-#650) Sleep
/// fires in-place at the agent's origin.
#[test]
fn cold_sleepy_agent_walks_to_fire_before_sleeping() {
    let mut world = TestWorld::with_seed(42);

    // Fire at 50 px — well within vision at the 6am start light level
    // (100 × 0.65 = 65 px). Otherwise the agent can't perceive the fire
    // and the prep scorer has no entity pull to drift toward.
    let fire_pos = Vec2::new(50.0, 0.0);
    world.spawn_campfire(fire_pos);
    world.tick(1); // let ontology derivation fill HasTrait(Campfire, HeatEmitting)

    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.4,
        ..Default::default()
    });
    // Warm up perception + urgency systems before pinning sleep state —
    // the prep swap relies on `visible` being populated, and that takes
    // a couple of ticks after spawn.
    world.tick(5);

    let start_dist = agent_pos(&world, agent).distance(fire_pos);

    // Pin the state each tick: sleepy enough for Sleep urgency to cross
    // its plan threshold, but not emergency-sleepy (so the prep pass
    // still runs). Cold enough that the sleep scorer actively wants the
    // fire tile.
    for _ in 0..400 {
        pin_sleepy_and_cold(&mut world, agent, 0.4, 0.15);
        world.tick(1);
    }

    let end_dist = agent_pos(&world, agent).distance(fire_pos);

    assert!(
        end_dist < start_dist - 20.0,
        "agent should have walked toward fire before sleeping; \
         start_dist={start_dist:.1}, end_dist={end_dist:.1}"
    );
    assert!(
        sleep_fired(&world, agent),
        "Sleep should fire once the agent arrives at a good sleep tile"
    );
}

/// An already-warm sleepy agent on the fire tile should fire Sleep
/// without a prep walk — their current tile already scores well.
#[test]
fn sleepy_agent_on_fire_tile_sleeps_in_place() {
    let mut world = TestWorld::with_seed(42);

    let fire_pos = Vec2::new(0.0, 0.0);
    world.spawn_campfire(fire_pos);
    world.tick(1);

    let agent = world.spawn_agent(AgentConfig {
        pos: fire_pos,
        warmth: 0.4,
        ..Default::default()
    });
    world.tick(1);

    let start_pos = agent_pos(&world, agent);

    for _ in 0..200 {
        pin_sleepy_and_cold(&mut world, agent, 0.4, 0.15);
        world.tick(1);
    }

    let end_pos = agent_pos(&world, agent);
    let travelled = start_pos.distance(end_pos);

    assert!(
        travelled < 15.0,
        "agent already on a good sleep tile should not travel far; got {travelled:.1}"
    );
    assert!(
        sleep_fired(&world, agent),
        "Sleep should fire directly when already positioned"
    );
}

/// Emergency sleepiness (agent about to pass out) bypasses the prep
/// pass — life beats quality. Sleep fires in-place even if a better
/// tile exists.
#[test]
fn emergency_sleepiness_bypasses_prep() {
    let mut world = TestWorld::with_seed(42);

    let fire_pos = Vec2::new(50.0, 0.0);
    world.spawn_campfire(fire_pos);
    world.tick(1);

    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(0.0, 0.0),
        warmth: 0.4,
        ..Default::default()
    });
    world.tick(1);

    // Pin the state: cold AND near-passing-out. Prep-would-prefer-fire,
    // but emergency override should fire Sleep immediately.
    for _ in 0..60 {
        pin_sleepy_and_cold(&mut world, agent, 0.4, 0.01);
        world.tick(1);
    }

    // Sleep should have fired despite the agent being at origin (nowhere
    // near the fire), proving the emergency override bypassed prep.
    assert!(
        sleep_fired(&world, agent),
        "near-passing-out agent must Sleep immediately, skip prep"
    );
    let end_pos = agent_pos(&world, agent);
    assert!(
        end_pos.distance(fire_pos) > 20.0,
        "emergency-sleep agent should NOT have walked to the fire; end pos {end_pos:?}"
    );
}
