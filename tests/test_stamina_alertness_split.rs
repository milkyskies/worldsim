//! Integration tests for the stamina/alertness split (#331).
//!
//! Verifies the two-pool Stamina (anaerobic + aerobic) and the cognitive
//! load drain that targets alertness independently of physical fatigue.
//! Per-activity tests were removed when the legacy `apply_activity_effects`
//! system was deleted in #495 — those concerns are covered by
//! `test_effort_recovery.rs` and the effort-model unit tests.

use worldsim::agent::body::needs::Consciousness;

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
