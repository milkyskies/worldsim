//! Activity effects: applies per-tick stat changes from the current activity to agent needs and emotions.
//!
//! Reads: CurrentActivity, ActivityConfig, TickCount
//! Writes: PhysicalNeeds, Consciousness, PsychologicalDrives, EmotionalState
//! Upstream: activity system (CurrentActivity, ActivityConfig), core::tick (TickCount)
//! Downstream: nervous_system::urgency (reads updated needs to recalculate urgencies)

use crate::agent::activity::{ActivityConfig, CurrentActivity};
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::psyche::emotions::{Emotion, EmotionalState};
use crate::agent::psyche::personality::Personality;
use crate::core::TickCount;
use bevy::prelude::*;

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
    )>,
) {
    // Pause is handled by run_if(not_paused) at the plugin level

    let dt = tick.dt();

    // Limits
    let max_stat = 100.0;
    let max_drive = 1.0;

    for (activity, mut physical, mut consciousness, drives, mut emotions, personality) in
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
        let raw_stamina_change = base_config.stamina_change + config.stamina_change;
        // Low-conscientiousness agents tire faster from physical work. Only
        // applies to drain; positive (recovery) is not penalized by personality.
        let stamina_change = if raw_stamina_change < 0.0 {
            let conscientiousness_relief = personality.traits.conscientiousness
                * crate::constants::brains::cognition::CONSCIENTIOUSNESS_STAMINA_RELIEF;
            raw_stamina_change * (1.0 - conscientiousness_relief)
        } else {
            raw_stamina_change
        };
        physical.stamina.adjust_aerobic(stamina_change * dt);

        // Sleep specifically refills both pools fast and boosts alertness
        // restoration. The activity's stamina_change contributes to aerobic;
        // anaerobic is refilled here at the same per-second rate.
        if matches!(activity, CurrentActivity::Sleeping) && raw_stamina_change > 0.0 {
            physical.stamina.anaerobic = (physical.stamina.anaerobic + raw_stamina_change * dt)
                .min(physical.stamina.anaerobic_max);
        }

        // Metabolism: burn glucose at BMR (base) + activity cost, digest the
        // stomach, and spill between glucose and reserves as appropriate.
        let bmr_drain = base_config.glucose_drain;
        let activity_drain = config.glucose_drain;
        physical.metabolism.tick(dt, bmr_drain, activity_drain);

        // Thirst
        let d_thirst = (base_config.thirst_change + config.thirst_change) * dt;
        physical.thirst = (physical.thirst + d_thirst).clamp(0.0, max_stat);

        // Health
        let d_health = (base_config.health_change + config.health_change) * dt;
        physical.health = (physical.health + d_health).clamp(0.0, max_stat);

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
