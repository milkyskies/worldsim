//! Integration tests for the stamina/alertness split (#331).
//!
//! Verifies the two-pool Stamina (anaerobic + aerobic) and the cognitive
//! load drain that targets alertness independently of physical fatigue.
//! Per-activity tests were removed when the legacy `apply_activity_effects`
//! system was deleted in #495 — those concerns are covered by
//! `test_effort_recovery.rs` and the effort-model unit tests.

use worldsim::agent::body::needs::{Consciousness, Stamina};

#[test]
fn stamina_default_starts_at_full_capacity() {
    let s = Stamina::default();
    assert_eq!(s.aerobic, s.aerobic_max);
    assert_eq!(s.anaerobic, s.anaerobic_max);
    assert_eq!(s.aerobic_fraction(), 1.0);
    assert_eq!(s.anaerobic_fraction(), 1.0);
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
    let high = Consciousness::default();
    let emotions = EmotionalState::default();
    let personality = Personality::default();

    let low_powers = calculate_brain_powers(&cns, &low, &emotions, &personality);
    let high_powers = calculate_brain_powers(&cns, &high, &emotions, &personality);

    assert!(
        low_powers.rational < high_powers.rational * 0.5,
        "exhausted agent's rational power should be <50% of alert baseline \
         (low={:.2}, high={:.2})",
        low_powers.rational,
        high_powers.rational
    );
}

/// Introverts pay more alertness per conversation turn than extraverts.
/// Mechanical analogue of "conversation is energising for extraverts and
/// draining for introverts."
#[test]
fn conversation_drains_alertness_more_for_introverts() {
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

    let mut introvert = Consciousness::default();
    let mut extravert = Consciousness::default();
    introvert.alertness -= introvert_drain;
    extravert.alertness -= extravert_drain;
    assert!(
        introvert.alertness < extravert.alertness,
        "introvert should end with less alertness than extravert after one turn"
    );
}
