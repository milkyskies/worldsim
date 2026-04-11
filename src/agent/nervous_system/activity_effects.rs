//! Activity effects: applies per-tick stat changes from the current activity to agent needs and emotions.
//!
//! Reads: CurrentActivity, ActivityConfig, TickCount
//! Writes: PhysicalNeeds, Consciousness, PsychologicalDrives, EmotionalState
//! Upstream: activity system (CurrentActivity, ActivityConfig), core::tick (TickCount)
//! Downstream: nervous_system::urgency (reads updated needs to recalculate urgencies)

use crate::agent::activity::{ActivityConfig, CurrentActivity};
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::psyche::emotions::{Emotion, EmotionalState};
use crate::agent::psyche::personality::Personality;
use crate::core::TickCount;
use bevy::prelude::*;

/// Per-tick metabolism update for **all** agents with `PhysicalNeeds`,
/// regardless of whether they carry a `CurrentActivity` component.
///
/// `apply_activity_effects` below requires `&CurrentActivity`, but real
/// agents in this codebase don't actually carry that component — it's a
/// legacy single-action marker that the multi-action `ActiveActions`
/// system replaced. Without this system, `Metabolism::tick_with_mods`
/// was never called on any spawned agent: stomachs never digested,
/// reserves never refilled from food, and agents starved with full
/// stomachs (#416). Action-level glucose drain in
/// `execution::tick_actions` was running fine, but the missing
/// digestion + reserve mobilization left the metabolism stuck.
///
/// This system runs at the BMR baseline only — activity drain stays in
/// `tick_actions` where the action's per-tick costs already live, and
/// activity-specific stat drift (mood etc.) stays in
/// `apply_activity_effects` for the rare entities that do carry
/// `CurrentActivity`. The mobilization pass at the end of
/// `tick_actions` still runs.
pub fn tick_metabolism(
    activity_config: Res<ActivityConfig>,
    tick: Res<TickCount>,
    mut query: Query<(&mut PhysicalNeeds, Option<&Body>)>,
) {
    let dt = tick.dt();
    let bmr_drain = activity_config.base.effects.glucose_drain;
    for (mut physical, body) in query.iter_mut() {
        let organ_mods = body.map(Body::organ_mods).unwrap_or_default();
        physical
            .metabolism
            .tick_with_mods(dt, bmr_drain, 0.0, organ_mods);

        // Passive stamina regen. The old `Stamina::recover` path is only
        // called from tests, so without this block a Flee/Attack sprint
        // drops anaerobic to 0 and it stays there until Sleep. Mirrors
        // the 0.5/tick anaerobic and slow aerobic refill that the
        // legacy recover API documents.
        physical.stamina.anaerobic =
            (physical.stamina.anaerobic + 0.5).min(physical.stamina.anaerobic_max);
        physical.stamina.aerobic =
            (physical.stamina.aerobic + 0.1).min(physical.stamina.aerobic_max);
    }
}

/// System to apply activity effects to agent state each tick
/// Effects are scaled to per-second rates but applied per-tick
pub fn apply_activity_effects(
    activity_config: Res<ActivityConfig>,
    tick: Res<TickCount>,
    mut query: Query<(
        &CurrentActivity,
        &mut PhysicalNeeds,
        &mut Consciousness,
        Option<&mut PsychologicalDrives>,
        &mut EmotionalState,
        &Personality,
        Option<&Body>,
    )>,
) {
    // Pause is handled by run_if(not_paused) at the plugin level

    let dt = tick.dt();

    // Limits
    let max_stat = 100.0;
    let max_drive = 1.0;

    for (activity, mut physical, mut consciousness, drives, mut emotions, personality, body) in
        query.iter_mut()
    {
        let base_config = &activity_config.base.effects;
        let config = &activity_config.get(activity).effects;

        // --- PHYSICAL NEEDS (0-100) ---
        // Sum base + specific

        // Stamina — routes through the aerobic sub-pool. Anaerobic is managed
        // by the intensity-driven drain/recover functions that the locomotion
        // system will call; activity_effects only moves aerobic. Sleep is the
        // exception: it refills both pools via restore_full below.
        //
        // Two modulators stack here:
        // - conscientiousness reduces *drain* (disciplined agents grind through)
        // - lung condition reduces *recovery* (damaged lungs deliver less O2,
        //   so aerobic reserves refill more slowly — #351 respiration bridge)
        let raw_stamina_change = base_config.stamina_change + config.stamina_change;
        let lung_condition = body.map(Body::lung_condition).unwrap_or(1.0);
        let stamina_change = compute_stamina_change(
            raw_stamina_change,
            personality.traits.conscientiousness,
            lung_condition,
        );
        physical.stamina.adjust_aerobic(stamina_change * dt);

        // Sleep specifically refills both pools fast and boosts alertness
        // restoration. The activity's stamina_change contributes to aerobic;
        // anaerobic is refilled here at the same per-second rate. Lung
        // condition gates the anaerobic refill the same way (oxygen debt
        // recovery is a respiratory process too).
        if matches!(activity, CurrentActivity::Sleeping) && raw_stamina_change > 0.0 {
            let anaerobic_refill = raw_stamina_change * lung_condition * dt;
            physical.stamina.anaerobic =
                (physical.stamina.anaerobic + anaerobic_refill).min(physical.stamina.anaerobic_max);
        }

        // Metabolism: burn glucose at BMR (base) + activity cost, digest the
        // stomach, and spill between glucose and reserves as appropriate.
        // Organ condition modulates digestion rate (stomach), nutrient
        // absorption (gut), and glucose/reserves conversion (liver); agents
        // without a Body (the rare case) get a fully-intact default.
        let bmr_drain = base_config.glucose_drain;
        let activity_drain = config.glucose_drain;
        let organ_mods = body.map(Body::organ_mods).unwrap_or_default();
        physical
            .metabolism
            .tick_with_mods(dt, bmr_drain, activity_drain, organ_mods);

        // Thirst
        let d_thirst = (base_config.thirst_change + config.thirst_change) * dt;
        physical.thirst = (physical.thirst + d_thirst).clamp(0.0, max_stat);

        // Health
        let d_health = (base_config.health_change + config.health_change) * dt;
        physical.health = (physical.health + d_health).clamp(0.0, max_stat);
        if d_health < 0.0 {
            physical.last_health_damage =
                Some(crate::agent::body::needs::HealthDamageSource::Exertion);
        }

        // --- CONSCIOUSNESS (0-1) ---
        let d_alertness = (base_config.alertness_change + config.alertness_change) * (dt * 0.01); // 0-100 rate to 0-1
        consciousness.alertness = (consciousness.alertness + d_alertness).clamp(0.0, 1.0);

        // --- PSYCHOLOGICAL DRIVES (0-1) ---
        if let Some(mut drives) = drives {
            // Social
            if config.social_change != 0.0 {
                let d_social = config.social_change * dt;
                drives.social = (drives.social + d_social).clamp(0.0, max_drive);
            }

            // Fun
            if config.fun_change != 0.0 {
                let d_fun = config.fun_change * dt;
                drives.fun = (drives.fun + d_fun).clamp(0.0, max_drive);
            }

            // Curiosity
            if config.curiosity_change != 0.0 {
                let d_curiosity = config.curiosity_change * dt;
                drives.curiosity = (drives.curiosity + d_curiosity).clamp(0.0, max_drive);
            }
        }

        // --- EMOTIONS ---
        // Only active can trigger emotions
        for (etype, intensity) in &config.emotion_changes {
            // Intensity needs to be scaled by dt?
            // If it's "Joy +5.0", does that mean +5 intensity per second?
            // Yes.
            let d_intensity = intensity * dt;

            // Just add it as a tiny burst or accumulate?
            // Since Emotion has `intensity` and `fuel`, we can just add fuel/intensity directly.
            if d_intensity > 0.0 {
                emotions.add_emotion(Emotion::new(*etype, d_intensity));
            }
        }
    }
}

/// Compute the effective stamina change for one tick, stacking the two
/// modulators that live here:
///
/// - **Drain path** (`raw_change < 0`): low-conscientiousness agents tire
///   faster. Lung condition is intentionally ignored on drain — weak lungs
///   don't make effort cheaper, just recovery slower. Weak-lungs penalty
///   applies through the *recovery* path.
/// - **Recovery path** (`raw_change >= 0`): lung condition scales the
///   refill. Fully intact lungs pass the raw change through; fully
///   destroyed lungs zero out recovery. Conscientiousness does not
///   accelerate recovery (rest isn't a willpower contest).
///
/// Pulled out as a pure function so the drain and recovery branches are
/// testable in isolation without spinning up a Bevy world.
fn compute_stamina_change(raw_change: f32, conscientiousness: f32, lung_condition: f32) -> f32 {
    if raw_change < 0.0 {
        let relief = conscientiousness
            * crate::constants::brains::cognition::CONSCIENTIOUSNESS_STAMINA_RELIEF;
        raw_change * (1.0 - relief)
    } else {
        raw_change * lung_condition.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod respiration_tests {
    use super::compute_stamina_change;

    /// Fully-intact lungs and neutral conscientiousness leave recovery
    /// unchanged — a baseline that protects the helper from accidental
    /// "always apply a multiplier" refactors.
    #[test]
    fn healthy_lungs_pass_recovery_through_unchanged() {
        let change = compute_stamina_change(20.0, 0.5, 1.0);
        assert!(
            (change - 20.0).abs() < 1e-6,
            "healthy lungs must not alter recovery, got {change}"
        );
    }

    /// Destroyed lungs zero out recovery. Downstream effect: an agent with
    /// no lungs sleeping on soft grass does not recover aerobic stamina,
    /// matching the issue-spec acceptance criterion.
    #[test]
    fn destroyed_lungs_zero_out_recovery() {
        let change = compute_stamina_change(20.0, 0.5, 0.0);
        assert_eq!(change, 0.0, "dead lungs must halt recovery");
    }

    /// Half-damaged lungs deliver half recovery — verifies the modulator
    /// is proportional, not a binary gate.
    #[test]
    fn half_damaged_lungs_halve_recovery() {
        let change = compute_stamina_change(20.0, 0.5, 0.5);
        assert!((change - 10.0).abs() < 1e-6, "expected 10.0, got {change}");
    }

    /// Drain is unaffected by lung condition. Weak lungs slow recovery but
    /// do not make running cheaper — you still burn effort.
    #[test]
    fn drain_is_independent_of_lung_condition() {
        let healthy = compute_stamina_change(-10.0, 0.0, 1.0);
        let dying = compute_stamina_change(-10.0, 0.0, 0.0);
        assert!(
            (healthy - dying).abs() < 1e-6,
            "drain must not differ by lungs, got healthy={healthy} dying={dying}"
        );
    }

    /// Conscientiousness still relieves drain as before. Protects the
    /// existing behaviour from the refactor.
    #[test]
    fn conscientiousness_relieves_drain() {
        let lazy = compute_stamina_change(-10.0, 0.0, 1.0);
        let disciplined = compute_stamina_change(-10.0, 1.0, 1.0);
        assert!(
            disciplined.abs() < lazy.abs(),
            "disciplined agents drain less stamina, got lazy={lazy} disciplined={disciplined}"
        );
    }

    /// Conscientiousness does NOT accelerate recovery — rest is not a
    /// willpower contest.
    #[test]
    fn conscientiousness_does_not_accelerate_recovery() {
        let lazy = compute_stamina_change(20.0, 0.0, 1.0);
        let disciplined = compute_stamina_change(20.0, 1.0, 1.0);
        assert!(
            (lazy - disciplined).abs() < 1e-6,
            "recovery must ignore conscientiousness, got lazy={lazy} disciplined={disciplined}"
        );
    }
}
