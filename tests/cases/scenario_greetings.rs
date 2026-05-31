//! Scenario tests for the social acknowledgment system (#467).
//!
//! Verifies that agents who know each other exchange passing greetings
//! when in visual range, without entering a full conversation.

use bevy::math::Vec2;
use worldsim::agent::body::needs::PsychologicalDrives;
use worldsim::agent::events::{SimEvent, SimEventKind};
use worldsim::testing::TestWorld;

#[test]
fn greeting_cooldown_prevents_spam() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(0.3)
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(0.3)
        .done()
        .relationship("alice", "bob", |r| r.trust(0.5).affection(0.6))
        .build();

    world.enable_fast_brains();
    // Run for 200 ticks — cooldown is 300, so at most one greeting per agent.
    world.tick(200);

    let alice = agents["alice"];
    let bob = agents["bob"];

    let greeting_count = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent { kind: SimEventKind::SocialAcknowledgment { actor, target, .. }, .. }
                    if (*actor == alice && *target == bob) || (*actor == bob && *target == alice)
            )
        })
        .count();

    assert!(
        greeting_count <= 2,
        "with 300-tick cooldown, at most 2 greetings (one per direction) in 200 ticks, got {greeting_count}"
    );
}

#[test]
fn strangers_do_not_greet() {
    let (mut world, _agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(0.3)
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(0.3)
        .done()
        // No .relationship() — they're strangers
        .build();

    world.enable_fast_brains();
    world.tick(120);

    let greetings = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent {
                    kind: SimEventKind::SocialAcknowledgment { .. },
                    ..
                }
            )
        })
        .count();

    assert_eq!(
        greetings, 0,
        "strangers should not exchange greetings, got {greetings}"
    );
}

#[test]
fn greeting_bumps_companionship() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(0.5)
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(0.5)
        .done()
        .relationship("alice", "bob", |r| r.trust(0.5).affection(0.6))
        .build();

    world.enable_fast_brains();
    // Wait for phenotype to develop, then record baseline.
    world.tick(5);

    let alice = agents["alice"];
    let before = world
        .app()
        .world()
        .get::<PsychologicalDrives>(alice)
        .expect("alice should have PsychologicalDrives")
        .companionship
        .value;

    // Run enough ticks for a greeting to fire (check interval = 60).
    world.tick(120);

    let after = world
        .app()
        .world()
        .get::<PsychologicalDrives>(alice)
        .expect("alice should have PsychologicalDrives")
        .companionship
        .value;

    // Companionship should have increased (from greeting + flocking proximity).
    assert!(
        after > before,
        "companionship should increase after greeting (before={before:.4}, after={after:.4})"
    );
}
