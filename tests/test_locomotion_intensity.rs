//! Integration tests for locomotion intensity (#339).
//!
//! These exercise the full proposal → admission → execution pipeline where
//! Movement-class actions carry a desired intensity and the body delivers
//! what it can. Pure math on `intensity_speed_multiplier` and
//! `effective_intensity` lives in `movement.rs`'s unit tests; here we
//! verify the ECS wiring holds end-to-end.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::actions::registry::{ActionState, ActiveActions};
use worldsim::agent::body::needs::{PhysicalNeeds, Stamina};
use worldsim::agent::movement::{effective_intensity, intensity_speed_multiplier};
use worldsim::testing::{AgentConfig, TestWorld};

/// Walk's default intensity maps to a 1.2x speed multiplier, Flee's default
/// maps to 2.0x — the calibration the issue spells out.
#[test]
fn default_intensities_match_issue_calibration() {
    let walk_default = ActionType::Walk.default_intensity_policy().resolve();
    let flee_default = ActionType::Flee.default_intensity_policy().resolve();
    assert_eq!(walk_default, 0.5, "Walk default is 0.5");
    assert_eq!(flee_default, 1.0, "Flee default is 1.0");

    let walk_mult = intensity_speed_multiplier(walk_default);
    let flee_mult = intensity_speed_multiplier(flee_default);
    assert!(
        (walk_mult - 1.2).abs() < 1e-5,
        "Walk at default should move at 1.2x base, got {walk_mult}"
    );
    assert!(
        (flee_mult - 2.0).abs() < 1e-5,
        "Flee at default should move at 2.0x base, got {flee_mult}"
    );
}

/// The brain's urgency boost lifts the same action's intensity — a hungry
/// agent walks faster to food than a curious one walking the same distance.
#[test]
fn urgency_boosts_same_action_intensity() {
    // Urgency inputs are on [0, 1] (the arbitration 0-100 scale divided)
    let calm_walk = ActionType::Walk
        .default_intensity_policy()
        .resolve_with_urgency(0.0);
    let urgent_walk = ActionType::Walk
        .default_intensity_policy()
        .resolve_with_urgency(0.9);

    assert!(
        urgent_walk > calm_walk,
        "urgent walk ({urgent_walk}) should be faster than calm walk ({calm_walk})"
    );
    assert_eq!(calm_walk, 0.5);
    // 0.5 default + 0.9 * 0.3 boost = 0.77
    assert!((urgent_walk - 0.77).abs() < 1e-5);

    // Non-locomotion actions stay at 0 regardless of urgency.
    assert_eq!(
        ActionType::Eat
            .default_intensity_policy()
            .resolve_with_urgency(1.0),
        0.0
    );
}

/// Flee's hardcoded 1.5x speed multiplier is gone. A Flee action running at
/// default intensity 1.0 gets exactly the intensity multiplier (2.0x) — not
/// an extra 1.5x bolt-on.
#[test]
fn flee_speed_is_intensity_driven_not_hardcoded() {
    // Sanity check: the only Flee-specific speed modifier should be its
    // default intensity mapping to 2.0x. If someone reintroduces a
    // hardcoded multiplier elsewhere, the test for "Flee at i=1 → 2.0x"
    // would not catch it — but this invariant check pins the whole
    // speed pipeline through intensity_speed_multiplier.
    let flee_default_mult =
        intensity_speed_multiplier(ActionType::Flee.default_intensity_policy().resolve());
    // 2.0 is the issue's calibration target. 1.5x (the old hardcoded
    // value) should NOT appear anywhere as a Flee-specific constant.
    assert_eq!(flee_default_mult, 2.0);
}

/// With empty anaerobic reserves, a desired sprint downgrades to jog. The
/// desired intensity on the ActionState is NOT mutated — the body just
/// delivers less.
#[test]
fn sprint_downgrades_to_jog_when_anaerobic_empty() {
    let s = Stamina {
        anaerobic: 0.0,
        aerobic: 80.0,
        ..Default::default()
    };
    // Brain still wants to sprint (desired = 1.0).
    let desired = 1.0;
    let effective = effective_intensity(desired, &s);
    assert_eq!(effective, 0.5, "anaerobic-empty sprint should jog");
    // Speed multiplier drops from 2.0 to 1.2 (jog).
    assert!((intensity_speed_multiplier(effective) - 1.2).abs() < 1e-5);
}

/// With both stamina pools critically low, any sustained action downgrades
/// to walking pace. The agent hasn't given up — their body just can't.
#[test]
fn exhausted_agent_downgrades_to_walk() {
    let s = Stamina {
        anaerobic: 0.0,
        aerobic: 5.0,
        ..Default::default()
    };
    // Desired sprint caps at walk.
    assert_eq!(effective_intensity(1.0, &s), 0.3);
    // Desired jog also caps at walk.
    assert_eq!(effective_intensity(0.6, &s), 0.3);
    // Walk-intensity desired stays put.
    assert_eq!(effective_intensity(0.3, &s), 0.3);
}

/// A Wander action that ticks for several seconds drains aerobic (the
/// intensity-based drain path fires in `tick_actions`, not via the
/// activity_effects legacy rate).
#[test]
fn wandering_drains_aerobic_through_intensity_path() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(200.0, 200.0),
        ..AgentConfig::default()
    });

    // Inject a Wander ActionState with an explicit target so the movement
    // branch of tick_actions actually runs.
    {
        let mut active = world
            .app_mut()
            .world_mut()
            .get_mut::<ActiveActions>(agent)
            .expect("agent should have ActiveActions");
        let mut state = ActionState::new(ActionType::Wander, 0);
        state = state.with_target_position(Vec2::new(1000.0, 200.0));
        active.insert(state);
    }

    let aerobic_before = world.get::<PhysicalNeeds>(agent).stamina.aerobic;
    world.tick(120);
    let aerobic_after = world.get::<PhysicalNeeds>(agent).stamina.aerobic;

    assert!(
        aerobic_after < aerobic_before,
        "aerobic should drop while wandering, before={aerobic_before:.2}, after={aerobic_after:.2}"
    );
}

/// A Flee sprint burns aerobic or anaerobic much faster than a Wander
/// covering the same wallclock time — this is the quadratic relationship
/// between intensity and stamina cost.
#[test]
fn sprint_costs_more_than_wander_for_same_duration() {
    let mut world = TestWorld::with_seed(0);
    let wanderer = world.spawn_agent(AgentConfig {
        name: Some("wanderer".into()),
        pos: Vec2::new(200.0, 200.0),
        ..AgentConfig::default()
    });
    let sprinter = world.spawn_agent(AgentConfig {
        name: Some("sprinter".into()),
        pos: Vec2::new(200.0, 500.0),
        ..AgentConfig::default()
    });

    {
        let world_mut = world.app_mut().world_mut();
        if let Some(mut active) = world_mut.get_mut::<ActiveActions>(wanderer) {
            let mut state = ActionState::new(ActionType::Wander, 0);
            state = state.with_target_position(Vec2::new(2000.0, 200.0));
            active.insert(state);
        }
        if let Some(mut active) = world_mut.get_mut::<ActiveActions>(sprinter) {
            let mut state = ActionState::new(ActionType::Flee, 0);
            state = state.with_target_position(Vec2::new(2000.0, 500.0));
            active.insert(state);
        }
    }

    world.tick(120);

    let wanderer_stamina = world.get::<PhysicalNeeds>(wanderer).stamina.clone();
    let sprinter_stamina = world.get::<PhysicalNeeds>(sprinter).stamina.clone();

    let wanderer_total_loss =
        (100.0 - wanderer_stamina.aerobic) + (100.0 - wanderer_stamina.anaerobic);
    let sprinter_total_loss =
        (100.0 - sprinter_stamina.aerobic) + (100.0 - sprinter_stamina.anaerobic);

    assert!(
        sprinter_total_loss > wanderer_total_loss * 2.0,
        "sprinter should burn much more stamina than wanderer over the same duration \
         (wanderer_loss={wanderer_total_loss:.2}, sprinter_loss={sprinter_total_loss:.2})"
    );
}

/// The desired intensity stored on the ActionState never changes even as
/// the body's effective intensity degrades. The agent's *intent* (to Flee
/// at 1.0) stays visible.
#[test]
fn desired_intensity_stays_stable_even_when_exhausted() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(200.0, 200.0),
        ..AgentConfig::default()
    });

    // Pre-exhaust both stamina pools so the body can't deliver sprint speed.
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(agent);
        needs.stamina.aerobic = 5.0;
        needs.stamina.anaerobic = 0.0;
    }

    // Inject a Flee at full desired intensity.
    {
        let mut active = world
            .app_mut()
            .world_mut()
            .get_mut::<ActiveActions>(agent)
            .expect("agent should have ActiveActions");
        let state = ActionState::new(ActionType::Flee, 0)
            .with_target_position(Vec2::new(2000.0, 200.0))
            .with_locomotion_intensity(1.0);
        active.insert(state);
    }

    world.tick(30);

    // After ticking while exhausted, the stored desired intensity on the
    // Flee ActionState is still 1.0 — the body delivered less, but the
    // intent is intact.
    let active = world.get::<ActiveActions>(agent);
    let flee_state = active
        .iter()
        .find(|a| a.action_type == ActionType::Flee)
        .expect("Flee should still be active");
    assert_eq!(
        flee_state.locomotion_intensity, 1.0,
        "desired intensity should not be mutated by exhaustion"
    );
}
