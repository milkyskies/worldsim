//! Integration tests for the anticipation primitive (#735).
//!
//! Pins the wiring between the urgency loop and the per-drive forecast:
//! at dusk a comfortable agent's Warmth urgency lifts ahead of any
//! actual deficit, and the lift scales with conscientiousness so
//! careful planners act earlier than reactive ones.

use bevy::math::Vec2;
use worldsim::agent::body::genetics::builder::personality;
use worldsim::agent::nervous_system::cns::CentralNervousSystem;
use worldsim::agent::nervous_system::urgency::UrgencySource;
use worldsim::core::GameTime;
use worldsim::core::tick::TickCount;
use worldsim::testing::{AgentConfig, TestWorld};

/// Returns the agent's Warmth urgency value, or 0.0 if no urgency was emitted
/// (drives below `min_threshold` aren't published).
fn warmth_urgency(world: &TestWorld, agent: bevy::prelude::Entity) -> f32 {
    let cns = world.get::<CentralNervousSystem>(agent);
    cns.urgencies
        .iter()
        .find(|u| u.source == UrgencySource::Warmth)
        .map(|u| u.value)
        .unwrap_or(0.0)
}

/// Drives the world to `start_offset_hours` past the simulation start
/// and reads the Warmth urgency for an agent spawned with full warmth.
/// Conscientiousness on the genome controls the lookahead horizon.
fn measure_warmth_urgency_at(start_offset_hours: u64, conscientiousness: f32) -> f32 {
    let mut world = TestWorld::with_seed(0);
    world.enable_fast_brains();

    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(1000.0, 1000.0),
        warmth: 1.0,
        genome: personality().conscientiousness(conscientiousness).into(),
        ..Default::default()
    });

    // Jump to the requested wall-clock hour. TestWorld's deterministic
    // tick advances both TickCount and GameTime, so a direct write here
    // is enough — the next tick reads through to the new value.
    world
        .app_mut()
        .world_mut()
        .resource_mut::<TickCount>()
        .current = start_offset_hours * GameTime::TICKS_PER_HOUR;

    // Pin the agent so wandering AI doesn't drift them next to a heat
    // source, and run one tick to refresh `cns.urgencies`.
    world.get_mut::<bevy::prelude::Transform>(agent).translation =
        bevy::prelude::Vec3::new(1000.0, 1000.0, 0.0);
    world.tick(1);

    warmth_urgency(&world, agent)
}

/// Comfortable agent at noon — no deficit now, no deficit predicted at
/// horizon (still warm at the lookahead) — Warmth urgency stays under
/// the `min_threshold` and is suppressed.
#[test]
fn warmth_urgency_quiet_at_noon_with_full_warmth() {
    // 06:00 start + 6h = 12:00 noon.
    let urgency = measure_warmth_urgency_at(6, 0.5);
    assert!(
        urgency < 0.1,
        "noon with full warmth should produce no Warmth urgency, got {urgency:.3}"
    );
}

/// Comfortable agent at dusk — current warmth is full, but the forecast
/// projects deep cold across the lookahead. The urgency loop must lift
/// the score above the live (zero) signal.
#[test]
fn warmth_urgency_fires_at_dusk_for_comfortable_agent() {
    // 06:00 start + 12h = 18:00 dusk.
    let dusk = measure_warmth_urgency_at(12, 0.5);
    let noon = measure_warmth_urgency_at(6, 0.5);
    assert!(
        dusk > noon,
        "anticipated Warmth urgency at dusk should exceed noon \
         (dusk={dusk:.3}, noon={noon:.3})"
    );
    assert!(
        dusk > 0.05,
        "dusk forecast should lift Warmth urgency past the min_threshold, got {dusk:.3}"
    );
}

/// Higher conscientiousness lengthens the lookahead horizon, so two
/// agents with identical body state at the same dusk tick produce
/// different anticipated urgency. This is the statistical
/// "high-Conscientiousness agents act earlier" property in
/// deterministic form: we read the urgency directly instead of
/// inferring it from action timing across many seeds.
#[test]
fn high_conscientiousness_anticipates_more_than_low() {
    // 17:00 — early dusk, ambient just starting to drop.
    let low_c = measure_warmth_urgency_at(11, 0.1);
    let high_c = measure_warmth_urgency_at(11, 0.9);
    assert!(
        high_c > low_c,
        "high-conscientiousness agent should predict more shortfall at dusk \
         (low={low_c:.3}, high={high_c:.3})"
    );
}
