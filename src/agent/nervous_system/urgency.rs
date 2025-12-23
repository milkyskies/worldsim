use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default)]
pub enum UrgencySource {
    #[default]
    Hunger,
    Thirst,
    Energy, // Fatigue
    Social,
    Fun,
    Fear,
    Pain,
    Boredom,
}

#[derive(Debug, Clone, Reflect)]
pub struct Urgency {
    pub source: UrgencySource,
    pub value: f32, // 0.0 to 1.0 (or higher if extreme)
}

impl Urgency {
    pub fn new(source: UrgencySource, value: f32) -> Self {
        Self { source, value }
    }
}

use super::config::{ModifierOp, NervousSystemConfig};
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::psyche::emotions::EmotionalState;

/// Fully data-driven urgency generation
/// Iterates over DriveConfigs and reads from Components directly
pub fn generate_urgency(
    ns_config: Res<NervousSystemConfig>,
    tick: Res<crate::core::tick::TickCount>,
    mut query: Query<
        (
            Entity,
            &mut CentralNervousSystem,
            &PhysicalNeeds,
            &Consciousness,
            Option<&PsychologicalDrives>,
            &EmotionalState,
            Option<&Body>,
            &crate::agent::psyche::personality::Personality,
            &crate::agent::actions::ActionState,
        ),
        With<crate::agent::Agent>,
    >,
) {
    for (entity, mut cns, physical, consciousness, drives, emotions, body, personality, activity) in
        query.iter_mut()
    {
        // Staggered: heavy thinking runs every N ticks, offset by entity ID
        if !tick.should_run(entity, ns_config.thinking_interval) {
            continue;
        }

        cns.urgencies.clear();

        // Helper: Get normalized value (0-1) for a specific urgency source
        // This maps the Source (Hunger, Energy) to the underlying Component Field.
        let get_source_value = |source: UrgencySource| -> f32 {
            match source {
                UrgencySource::Hunger => (physical.hunger / 100.0).clamp(0.0, 1.0),
                UrgencySource::Thirst => (physical.thirst / 100.0).clamp(0.0, 1.0),
                UrgencySource::Energy => (physical.energy / 100.0).clamp(0.0, 1.0),

                // Pain is complex, sum of injuries
                UrgencySource::Pain => {
                    let pain = body.map(|b| b.total_pain()).unwrap_or(0.0);
                    (pain / 100.0).clamp(0.0, 1.0)
                }

                // Social Drive (0-1)
                UrgencySource::Social => drives.map(|d| d.social).unwrap_or(0.0),
                UrgencySource::Fun => drives.map(|d| d.fun).unwrap_or(0.0),
                UrgencySource::Fear => emotions
                    .get_emotion_intensity(crate::agent::psyche::emotions::EmotionType::Fear),

                // Boredom is usually a constant base + modifiers
                UrgencySource::Boredom => 0.0,
            }
        };

        // --- GENERIC LOOP OVER ALL DRIVE CONFIGS ---
        for drive_config in &ns_config.drives {
            // 1. Get Base Input (Hardcoded Mapping)
            let base_input = get_source_value(drive_config.source);

            // For Energy, "High Fatigue" means "Low Energy"
            // We handle inversion here specifically for Energy if needed, or rely on config curve.
            // Actually, config response curve handles the mapping from Input -> Urgency.
            // e.g. If Input is Energy (High = 1.0), and we want Urgency when Low,
            // we probably need an explicit Invert flag in config or handle it here?
            // The previous code had `invert` in config. Let's rely on standard logic:
            // Urgency = f(Needs). High Need = High Urgency.
            // Hunger: High Value = High Need.
            // Energy: Low Value = High Need.

            let normalized_input = if drive_config.source == UrgencySource::Energy {
                1.0 - base_input // 1.0 energy = 0.0 fatigue
            } else {
                base_input
            };

            // If base constant is non-zero, it might override or add to input (e.g. Boredom)
            // If input is 0 (e.g. Boredom source returns 0), we use base_constant?
            // Let's just Max them.
            let effective_base = normalized_input.max(drive_config.base_constant);

            // 2. Apply response curve
            let curved = drive_config.curve.apply(effective_base);

            // 3. Apply personality sensitivity
            let sensitivity = drive_config.sensitivity.compute(personality);
            let mut score = curved * sensitivity;

            // 4. Apply context modifiers
            for modifier in &drive_config.modifiers {
                // Modifiers also read from Sources now
                let mod_input = get_source_value(modifier.input_source);

                match modifier.operation {
                    ModifierOp::DampenByHigh => {
                        score *= 1.0 - (mod_input * modifier.factor);
                    }
                    ModifierOp::DampenByLow => {
                        score *= 1.0 - ((1.0 - mod_input) * modifier.factor);
                    }
                    ModifierOp::BoostBy => {
                        score *= 1.0 + (mod_input * modifier.factor);
                    }
                    ModifierOp::Add => {
                        score += mod_input * modifier.factor;
                    }
                    ModifierOp::Subtract => {
                        score -= mod_input * modifier.factor;
                    }
                }
            }

            // 5. Clamp and threshold
            score = score.max(0.0);
            if score > drive_config.min_threshold {
                cns.urgencies.push(Urgency::new(drive_config.source, score));
            }
        }

        // --- MOMENTUM & CONSCIOUSNESS ---

        // Map Activity to UrgencySource
        let current_source = match activity.action_type {
            crate::agent::actions::ActionType::Eat => Some(UrgencySource::Hunger),
            crate::agent::actions::ActionType::Sleep => Some(UrgencySource::Energy),
            crate::agent::actions::ActionType::Wander => Some(UrgencySource::Boredom),
            _ => None,
        };

        let alertness = consciousness.alertness;

        for urgency in cns.urgencies.iter_mut() {
            // Apply Momentum (from config)
            if Some(urgency.source) == current_source {
                urgency.value *= ns_config.momentum_bonus;
            }

            // Apply Consciousness / Sensory Gating (emergent from alertness)
            let is_current_drive = Some(urgency.source) == current_source;

            if !is_current_drive {
                // Check if this drive bypasses gating (e.g. Pain)
                let bypass = ns_config
                    .get_drive(urgency.source)
                    .map(|d| d.bypasses_gating)
                    .unwrap_or(false);

                if !bypass {
                    // Determine Channel Factor
                    let mut channel_dampening = 0.1 + (alertness * 0.9); // Default fallback

                    if ns_config.interoception.sources.contains(&urgency.source) {
                        // Interoception (Hunger/Pain): Hard to ignore.
                        channel_dampening = 0.6 + (alertness * 0.4);
                    } else if ns_config.exteroception.sources.contains(&urgency.source) {
                        // Exteroception (Social/Fear): Highly dependent on being awake.
                        channel_dampening = 0.0 + (alertness * 1.0);
                    } else if ns_config.proprioception.sources.contains(&urgency.source) {
                        // Proprioception (Movement/Energy):
                        channel_dampening = 0.2 + (alertness * 0.8);
                    }

                    urgency.value *= channel_dampening;
                }
            }
        }

        // Sort Highest Urgency First
        cns.urgencies.sort_by(|a, b| {
            b.value
                .partial_cmp(&a.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}
