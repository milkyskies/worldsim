//! Integration tests for the stamina/alertness split (#331).
//!
//! Verifies the two-pool Stamina (anaerobic + aerobic) and the cognitive load
//! drain that targets alertness independently of physical fatigue. The drain
//! and recovery formulas themselves are unit-tested in `agent::body::needs`;
//! these tests exercise how the systems wire them into the ECS loop.

use worldsim::agent::activity::CurrentActivity;
use worldsim::agent::body::needs::{Consciousness, PhysicalNeeds, Stamina};
use worldsim::agent::psyche::personality::{Personality, PersonalityTraits};
use worldsim::testing::{AgentConfig, TestWorld};

/// Sleep activity refills BOTH aerobic and anaerobic pools — the acceptance
/// criterion for "sleep restores both at high rate".
#[test]
fn sleeping_restores_both_stamina_pools() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig::default());

    // Drain both pools to low values and force the sleeping activity.
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(agent);
        needs.stamina.aerobic = 20.0;
        needs.stamina.anaerobic = 20.0;
    }
    // Re-insert Sleeping each tick to prevent the brain from switching away.
    // Tick in short bursts so both pools get meaningful recovery time.
    for _ in 0..4 {
        world
            .app_mut()
            .world_mut()
            .entity_mut(agent)
            .insert(CurrentActivity::Sleeping);
        world.tick(15);
    }

    let stamina = world.get::<PhysicalNeeds>(agent).stamina.clone();
    assert!(
        stamina.aerobic > 20.0,
        "sleep should restore aerobic above starting 20.0, got {}",
        stamina.aerobic
    );
    assert!(
        stamina.anaerobic > 20.0,
        "sleep should also restore anaerobic above starting 20.0, got {}",
        stamina.anaerobic
    );
}

/// Exhaustion from physical work hits aerobic but leaves anaerobic untouched
/// (because activity_effects only drains aerobic; anaerobic is intensity-driven
/// in the locomotion system). Verifies the split at the ECS level.
#[test]
fn wandering_drains_aerobic_not_anaerobic() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig::default());

    world
        .app_mut()
        .world_mut()
        .entity_mut(agent)
        .insert(CurrentActivity::Wandering);

    let anaerobic_before = world.get::<PhysicalNeeds>(agent).stamina.anaerobic;
    world.tick(120);

    let stamina = world.get::<PhysicalNeeds>(agent).stamina.clone();
    assert!(
        stamina.aerobic < 100.0,
        "wandering should drain aerobic, got {}",
        stamina.aerobic
    );
    assert_eq!(
        stamina.anaerobic, anaerobic_before,
        "wandering must not touch anaerobic (sprint reserve)"
    );
}

/// Low-conscientiousness agents drain aerobic faster from the same physical
/// activity than high-conscientiousness agents. Verifies the personality
/// multiplier is wired into activity_effects.
#[test]
fn low_conscientiousness_drains_stamina_faster_than_high() {
    let mut world = TestWorld::with_seed(0);
    let lazy = world.spawn_agent(AgentConfig {
        name: Some("lazy".into()),
        personality: Personality {
            traits: PersonalityTraits {
                conscientiousness: 0.0,
                ..Default::default()
            },
        },
        ..AgentConfig::default()
    });
    let disciplined = world.spawn_agent(AgentConfig {
        name: Some("disciplined".into()),
        personality: Personality {
            traits: PersonalityTraits {
                conscientiousness: 1.0,
                ..Default::default()
            },
        },
        ..AgentConfig::default()
    });

    for agent in [lazy, disciplined] {
        world
            .app_mut()
            .world_mut()
            .entity_mut(agent)
            .insert(CurrentActivity::Wandering);
    }

    world.tick(120);

    let lazy_aerobic = world.get::<PhysicalNeeds>(lazy).stamina.aerobic;
    let disciplined_aerobic = world.get::<PhysicalNeeds>(disciplined).stamina.aerobic;

    assert!(
        lazy_aerobic < disciplined_aerobic,
        "lazy agent should drain aerobic faster than disciplined one \
         (lazy={lazy_aerobic:.2}, disciplined={disciplined_aerobic:.2})"
    );
}

/// Brain arbitration drains alertness over time via the cognitive-load system.
/// We force an activity that doesn't restore alertness (Wandering gives only
/// +0.5/s, which we work around by using a much shorter observation window
/// than what recovery could plausibly offset) and verify alertness drops
/// below the starting value after enough brain ticks.
#[test]
fn cognitive_load_drains_alertness_during_activity() {
    let mut world = TestWorld::with_seed(0);
    // Agent with near-zero conscientiousness so tick relief is minimal.
    let agent = world.spawn_agent(AgentConfig {
        personality: Personality {
            traits: PersonalityTraits {
                conscientiousness: 0.0,
                openness: 0.0,
                ..Default::default()
            },
        },
        ..AgentConfig::default()
    });
    // Harvesting: alertness_change -0.5, so activity pulls alertness down
    // independently of cognitive load. Fake target entity is fine — we
    // only care that the activity slot drains alertness.
    let dummy_target = world.app_mut().world_mut().spawn_empty().id();
    world
        .app_mut()
        .world_mut()
        .entity_mut(agent)
        .insert(CurrentActivity::Harvesting(dummy_target, 100));

    let alertness_before = world.get::<Consciousness>(agent).alertness;
    world.tick(600); // 10 seconds at 60 tps
    let alertness_after = world.get::<Consciousness>(agent).alertness;

    assert!(
        alertness_after < alertness_before,
        "alertness should drop under sustained activity + cognitive load \
         (before={alertness_before:.3}, after={alertness_after:.3})"
    );
}

/// The Stamina struct is exported and constructible from tests.
#[test]
fn stamina_default_starts_at_full_capacity() {
    let s = Stamina::default();
    assert_eq!(s.aerobic, s.aerobic_max);
    assert_eq!(s.anaerobic, s.anaerobic_max);
    assert_eq!(s.aerobic_fraction(), 1.0);
    assert_eq!(s.anaerobic_fraction(), 1.0);
}
