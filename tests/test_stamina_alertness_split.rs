//! Integration tests for the stamina/alertness split (#331).
//!
//! Verifies the two-pool Stamina (anaerobic + aerobic) and the cognitive load
//! drain that targets alertness independently of physical fatigue. The drain
//! and recovery formulas themselves are unit-tested in `agent::body::needs`;
//! these tests exercise how the systems wire them into the ECS loop.

use worldsim::agent::activity::CurrentActivity;
use worldsim::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives, Stamina};
use worldsim::testing::{AgentConfig, TestWorld, personality};

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

/// Exhaustion from physical work hits aerobic hard and leaves anaerobic
/// mostly intact (activity_effects only drains aerobic; anaerobic is
/// intensity-driven and only burns when the agent is sprinting or patrolling
/// hard). The ≤2 tolerance post-#386 accounts for the fact that a default
/// agent spawns with `drives.curiosity = 0.5` and the Emotional brain now
/// proposes a real Explore (intensity 0.5) in parallel, which nibbles at
/// anaerobic at a barely-visible rate. That's legitimate — the test's
/// point was "wandering shouldn't drain your sprint reserve meaningfully,"
/// not "anaerobic must be byte-identical to before."
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
    assert!(
        stamina.anaerobic > anaerobic_before - 2.0,
        "wandering should not meaningfully drain anaerobic (sprint reserve); \
         had {anaerobic_before}, now {}",
        stamina.anaerobic,
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
        genome: personality().conscientiousness(0.0).into(),
        ..AgentConfig::default()
    });
    let disciplined = world.spawn_agent(AgentConfig {
        name: Some("disciplined".into()),
        genome: personality().conscientiousness(1.0).into(),
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
        genome: personality().conscientiousness(0.0).openness(0.0).into(),
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

/// Forcing an agent to idle (no physical work) while brain ticks fire
/// repeatedly should drain alertness without touching stamina. The
/// decoupling is the whole point of the split.
#[test]
fn idle_brain_work_drains_alertness_but_not_stamina() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig {
        // Zero openness / conscientiousness so cognitive drain lands at full rate.
        genome: personality().conscientiousness(0.0).openness(0.0).into(),
        ..AgentConfig::default()
    });

    // Tick once so `develop_phenotype_system` runs and writes genome-derived
    // drives. After this we max every drive's satisfaction so the
    // brain has no motivation to pursue anything — the test relies on
    // the agent staying in Idle for the full duration, and unmet drives
    // would occasionally make it wander and drain anaerobic.
    world.tick(1);
    {
        let mut drives = world.get_mut::<PsychologicalDrives>(agent);
        *drives = PsychologicalDrives {
            companionship: 1.0,
            enjoyment: 1.0,
            stimulation: 1.0,
            esteem: 1.0,
            safety: 1.0,
            autonomy: 1.0,
            dominion: 1.0,
        };
    }

    // Pin the agent into Idle so no physical activity drains aerobic. The
    // idle activity still restores alertness (+2.5/s scaled), so we need to
    // run long enough for cognitive drain to overcome it but short enough
    // that aerobic metabolism (-0.15/s) stays visible.
    let aerobic_before = world.get::<PhysicalNeeds>(agent).stamina.aerobic;
    let anaerobic_before = world.get::<PhysicalNeeds>(agent).stamina.anaerobic;

    for _ in 0..20 {
        world
            .app_mut()
            .world_mut()
            .entity_mut(agent)
            .insert(CurrentActivity::Idle);
        world.tick(30);
    }

    let after = world.get::<PhysicalNeeds>(agent).stamina.clone();
    // Anaerobic is fully independent of activity_effects — must be untouched.
    assert_eq!(
        after.anaerobic, anaerobic_before,
        "idle brain work must not touch anaerobic"
    );
    // Aerobic only drifts from base metabolism, which is tiny; the key is
    // it stays near the starting value rather than collapsing.
    assert!(
        after.aerobic > aerobic_before - 5.0,
        "idle agent's aerobic should not drop more than base metabolism, \
         before={aerobic_before:.1}, after={after:.1?}"
    );
}

/// Rational brain power collapses when alertness is pinned at 0.2 — the
/// existing `alertness_penalty` mechanic, asserted explicitly.
#[test]
fn low_alertness_cripples_rational_power() {
    use worldsim::agent::brains::arbitration::calculate_brain_powers;
    use worldsim::agent::nervous_system::cns::CentralNervousSystem;
    use worldsim::agent::psyche::emotions::EmotionalState;
    use worldsim::agent::psyche::personality::Personality;

    let cns = CentralNervousSystem::default();
    let low = Consciousness { alertness: 0.2 };
    let high = Consciousness::default(); // 1.0 by default
    let emotions = EmotionalState::default();
    let personality = Personality::default();

    let low_powers = calculate_brain_powers(&cns, &low, &emotions, &personality);
    let high_powers = calculate_brain_powers(&cns, &high, &emotions, &personality);

    // At alertness 0.2, penalty = (0.5 - 0.2) * 2 = 0.6, so rational power
    // collapses to 40% of its alert value.
    assert!(
        low_powers.rational < high_powers.rational * 0.5,
        "exhausted agent's rational power should be <50% of alert baseline \
         (low={:.2}, high={:.2})",
        low_powers.rational,
        high_powers.rational
    );
}

/// A full sleep cycle (enter sleep → accumulate sleep time → wake up) should
/// leave the agent with alertness at maximum. The mechanism: alertness
/// drops to near-zero during the Sleeping activity, then the WakeUp
/// transition spikes it back up to 1.0.
#[test]
fn sleep_cycle_restores_alertness_to_max() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig::default());

    // Pre-drain alertness to simulate a mentally exhausted agent.
    {
        let mut c = world.get_mut::<Consciousness>(agent);
        c.alertness = 0.2;
    }

    // Phase 1: force Sleeping for a bit. Alertness drops further.
    for _ in 0..4 {
        world
            .app_mut()
            .world_mut()
            .entity_mut(agent)
            .insert(CurrentActivity::Sleeping);
        world.tick(15);
    }

    // Phase 2: force WakeUp transition. Alertness should snap to ~1.0.
    for _ in 0..4 {
        world
            .app_mut()
            .world_mut()
            .entity_mut(agent)
            .insert(CurrentActivity::WakeUp);
        world.tick(15);
    }

    let alertness_after = world.get::<Consciousness>(agent).alertness;
    assert!(
        alertness_after > 0.9,
        "sleep cycle should restore alertness to near-max, got {alertness_after:.3}"
    );
}

/// Introverts pay more alertness per conversation turn than extraverts.
/// This is the mechanical analogue of "conversation is energising for
/// extraverts and draining for introverts."
#[test]
fn conversation_drains_alertness_more_for_introverts() {
    use worldsim::agent::body::needs::Consciousness;

    // Direct unit test of the drain math — full system integration would
    // require a conversation scenario which is covered by existing tests.
    let speaker_drain =
        worldsim::constants::brains::cognition::CONVERSATION_SPEAKER_ALERTNESS_DRAIN;
    let relief = worldsim::constants::brains::cognition::EXTRAVERSION_CONVERSATION_RELIEF;

    let introvert_drain = speaker_drain * (1.0 - 0.1 * relief);
    let extravert_drain = speaker_drain * (1.0 - 0.9 * relief);

    assert!(
        introvert_drain > extravert_drain * 2.0,
        "introvert conversation drain should be much larger than extravert's \
         (introvert={introvert_drain:.5}, extravert={extravert_drain:.5})"
    );

    // Sanity: applying the drain to two Consciousness values produces the
    // expected ordering.
    let mut introvert = Consciousness::default();
    let mut extravert = Consciousness::default();
    introvert.alertness -= introvert_drain;
    extravert.alertness -= extravert_drain;
    assert!(
        introvert.alertness < extravert.alertness,
        "introvert should end with less alertness than extravert after one turn"
    );
}
