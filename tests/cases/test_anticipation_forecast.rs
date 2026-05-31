//! End-to-end tests: forecast lifts urgency before a deficit lands.

use bevy::math::Vec2;
use worldsim::agent::body::genetics::builder::personality;
use worldsim::agent::nervous_system::cns::CentralNervousSystem;
use worldsim::agent::nervous_system::urgency::UrgencySource;
use worldsim::core::GameTime;
use worldsim::core::tick::TickCount;
use worldsim::testing::{AgentConfig, TestWorld};

fn warmth_urgency(world: &TestWorld, agent: bevy::prelude::Entity) -> f32 {
    let cns = world.get::<CentralNervousSystem>(agent);
    cns.urgencies
        .iter()
        .find(|u| u.source == UrgencySource::Warmth)
        .map(|u| u.value)
        .unwrap_or(0.0)
}

fn measure_warmth_urgency_at(start_offset_hours: u64, conscientiousness: f32) -> f32 {
    let mut world = TestWorld::with_seed(0);
    world.enable_fast_brains();

    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(1000.0, 1000.0),
        warmth: 1.0,
        genome: personality().conscientiousness(conscientiousness).into(),
        ..Default::default()
    });

    world
        .app_mut()
        .world_mut()
        .resource_mut::<TickCount>()
        .current = start_offset_hours * GameTime::TICKS_PER_HOUR;

    // Pin transform so wandering AI can't drift the agent next to a heat source.
    world.get_mut::<bevy::prelude::Transform>(agent).translation =
        bevy::prelude::Vec3::new(1000.0, 1000.0, 0.0);
    world.tick(1);

    warmth_urgency(&world, agent)
}

#[test]
fn warmth_urgency_quiet_at_noon_with_full_warmth() {
    let urgency = measure_warmth_urgency_at(6, 0.5);
    assert!(
        urgency < 0.1,
        "noon with full warmth should be quiet, got {urgency:.3}"
    );
}

#[test]
fn warmth_urgency_fires_at_dusk_for_comfortable_agent() {
    let dusk = measure_warmth_urgency_at(12, 0.5);
    let noon = measure_warmth_urgency_at(6, 0.5);
    assert!(
        dusk > noon,
        "anticipated Warmth urgency at dusk should exceed noon \
         (dusk={dusk:.3}, noon={noon:.3})"
    );
    assert!(
        dusk > 0.05,
        "dusk forecast should lift Warmth urgency past min_threshold, got {dusk:.3}"
    );
}

#[test]
fn high_conscientiousness_anticipates_more_than_low() {
    let low_c = measure_warmth_urgency_at(11, 0.1);
    let high_c = measure_warmth_urgency_at(11, 0.9);
    assert!(
        high_c > low_c,
        "high-conscientiousness agent should predict more shortfall \
         (low={low_c:.3}, high={high_c:.3})"
    );
}
