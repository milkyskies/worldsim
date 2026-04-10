//! Verifies relationship decay behavior (see src/agent/psyche/relationships.rs).
//!
//! The decay system fires once per game day and uses an exponential half-life
//! that scales with bond strength:
//!   - Strong bonds (near 0.0 or 1.0) decay slowly — full-year half-life.
//!   - Weak bonds (near 0.5) decay quickly — few-day half-life.
//!   - Negative feelings (below 0.5) linger longer (negativity bias).
//!   - Any relationship refreshed within `decay_grace_days` is skipped entirely.

use bevy::prelude::*;
use worldsim::agent::mind::knowledge::{Metadata, MindGraph, Node, Predicate, Triple, Value};
use worldsim::core::time::GameTime;
use worldsim::testing::{AgentConfig, TestWorld};

/// One game day in ticks — the interval at which decay fires.
const TICKS_PER_DAY: u64 = GameTime::TICKS_PER_DAY;

fn set_trust(world: &mut TestWorld, agent: Entity, other: Entity, trust: f32, timestamp: u64) {
    world
        .app_mut()
        .world_mut()
        .get_mut::<MindGraph>(agent)
        .expect("agent should have MindGraph")
        .assert(Triple::with_meta(
            Node::Entity(other),
            Predicate::Trust,
            Value::Float(trust),
            Metadata::semantic(timestamp),
        ));
}

fn get_trust(world: &TestWorld, agent: Entity, other: Entity) -> Option<f32> {
    world
        .app()
        .world()
        .get::<MindGraph>(agent)
        .and_then(|mind| mind.get(&Node::Entity(other), Predicate::Trust))
        .and_then(|v| {
            if let Value::Float(f) = v {
                Some(*f)
            } else {
                None
            }
        })
}

/// Strong bonds should lose only a small fraction of their distance from
/// neutral after a single game day. At a ~60-day half-life, one day removes
/// less than 2% of the gap between the current value and neutral.
#[test]
fn strong_trust_barely_decays_in_one_game_day() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());
    // Bare entity so no social system can refresh the timestamp.
    let other = world.app_mut().world_mut().spawn_empty().id();

    set_trust(&mut world, agent, other, 0.95, 0);

    // Tick one full day + a margin so the day-boundary system definitely fires.
    world.tick(TICKS_PER_DAY + 1);

    let trust = get_trust(&world, agent, other).expect("trust should still exist");
    // 0.95 with a ~60-day half-life: one day ≈ 0.94.7 — nearly unchanged.
    assert!(
        trust > 0.93,
        "strong trust (0.95) should barely fade after 1 game day, got {trust}"
    );
    assert!(trust < 0.95, "some decay should have occurred, got {trust}");
}

/// Weak bonds (close to neutral) fade quickly — a few game days should move a
/// 0.6 trust noticeably back toward 0.5.
#[test]
fn weak_trust_fades_quickly_over_several_days() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());
    let other = world.app_mut().world_mut().spawn_empty().id();

    set_trust(&mut world, agent, other, 0.6, 0);

    // 5 game days.
    world.tick(TICKS_PER_DAY * 5 + 1);

    let trust = get_trust(&world, agent, other).expect("trust should still exist");
    // Weak bond has a half-life of ~4 days; after 5 days trust should be noticeably
    // below 0.6 but still above neutral.
    assert!(
        trust < 0.58,
        "weak trust (0.6) should fade within 5 game days, got {trust}"
    );
    assert!(
        trust > 0.5,
        "trust should asymptote toward neutral, not overshoot, got {trust}"
    );
}

/// Negativity bias: below-neutral trust decays more slowly than equivalent
/// above-neutral trust. Starting from symmetric distances (0.2 vs 0.8), after
/// the same elapsed time, the negative value should be closer to its start.
#[test]
fn negative_trust_lingers_longer_than_positive() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());
    let positive_other = world.app_mut().world_mut().spawn_empty().id();
    let negative_other = world.app_mut().world_mut().spawn_empty().id();

    set_trust(&mut world, agent, positive_other, 0.8, 0);
    set_trust(&mut world, agent, negative_other, 0.2, 0);

    // Advance 10 game days.
    world.tick(TICKS_PER_DAY * 10 + 1);

    let positive = get_trust(&world, agent, positive_other).expect("positive trust exists");
    let negative = get_trust(&world, agent, negative_other).expect("negative trust exists");

    // Distance remaining from neutral.
    let positive_remaining = (positive - 0.5).abs();
    let negative_remaining = (negative - 0.5).abs();

    assert!(
        negative_remaining > positive_remaining,
        "negativity bias: below-neutral trust ({negative}) should decay slower than \
         above-neutral ({positive}). remaining: neg={negative_remaining} pos={positive_remaining}"
    );
}

/// Grace period: a relationship refreshed within `decay_grace_days` of the
/// next decay tick must NOT decay. Starting the clock one day short of the
/// grace boundary means the first decay fire still sees it as "recent."
#[test]
fn recent_interaction_skips_decay() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());
    let other = world.app_mut().world_mut().spawn_empty().id();

    // Advance to just before the first decay fire.
    world.tick(TICKS_PER_DAY - 5);

    // Refresh the trust timestamp at this tick.
    let timestamp = TICKS_PER_DAY - 5;
    set_trust(&mut world, agent, other, 0.8, timestamp);

    // Cross the first day boundary — decay fires at tick TICKS_PER_DAY.
    // Time since last interaction = 5 ticks, well within the 1-day grace period.
    world.tick(6);

    let trust = get_trust(&world, agent, other).expect("trust should still exist");
    assert_eq!(
        trust, 0.8,
        "trust refreshed within the grace period should not decay"
    );
}
