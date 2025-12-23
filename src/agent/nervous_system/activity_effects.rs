use crate::agent::activity::ActivityConfig;
use crate::agent::activity::CurrentActivity;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::psyche::emotions::{Emotion, EmotionalState};
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
    )>,
) {
    // Pause is handled by run_if(not_paused) at the plugin level

    // Convert per-second rates to per-tick
    // Base 60Hz execution. Speed multiplier = ticks_per_second / 60.0.
    // dt = (Multiplier) * (1.0 / 60.0) = ticks_per_second / 3600.0.
    let dt = tick.ticks_per_second / 3600.0;

    // Limits
    let max_stat = 100.0;
    let max_drive = 1.0;

    for (activity, mut physical, mut consciousness, drives, mut emotions) in query.iter_mut() {
        let base_config = &activity_config.base.effects;
        let config = &activity_config.get(activity).effects;

        // --- PHYSICAL NEEDS (0-100) ---
        // Sum base + specific

        // Energy
        let d_energy = (base_config.energy_change + config.energy_change) * dt;
        physical.energy = (physical.energy + d_energy).clamp(0.0, max_stat);

        // Hunger
        let d_hunger = (base_config.hunger_change + config.hunger_change) * dt;
        physical.hunger = (physical.hunger + d_hunger).clamp(0.0, max_stat);

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
