//! Urgency generation: maps physical/emotional state to drive urgencies.
//!
//! Reads: PhysicalNeeds, Consciousness, PsychologicalDrives, EmotionalState, Body, ActiveActions
//! Writes: CentralNervousSystem.urgencies
//! Upstream: body (needs), psyche (emotions), nervous_system::config
//! Downstream: nervous_system::cns (urgency ranking)

use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default, serde::Serialize)]
pub enum UrgencySource {
    #[default]
    Hunger,
    Thirst,
    Stamina, // Fatigue
    Social,
    Fun,
    Fear,
    Pain,
    /// Desire for novel stimulation. Reads from `drives.curiosity` which
    /// rises slowly during unstimulating activity and drains via
    /// `RuntimeEffects::curiosity_per_sec` when the agent does something
    /// that scratches the itch (Observe, Explore, Wander, Converse).
    /// Replaces the old `Boredom` source, which was a half-implemented
    /// constant-baseline drive with no backing state.
    Curiosity,
    Territoriality,
    /// Homeostatic sleep pressure from wakefulness decay. Independent of
    /// Stamina — a desk worker gets sleepy without physical exertion.
    /// Reads `PhysicalNeeds::wakefulness` (high = rested, inverted to
    /// urgency in the generation loop).
    Sleepiness,
    /// A promise made to another agent. Emitted by `generate_urgency`
    /// for each `PlanSource::VerbalCommitment` in the agent's
    /// `PlanMemory`. Value scales with conscientiousness per the
    /// original VERBAL_COMMITMENT_PRIORITY_* constants.
    Commitment,
    /// Thermal comfort deficit from `PhysicalNeeds::warmth`. Rises as the
    /// agent cools — above 0.6 warmth the urgency is near zero, below 0.3
    /// it rivals Hunger, below 0.1 it is life-threatening. Satisfied by
    /// the WarmUp action (which requires proximity to a HeatSource).
    Warmth,
    /// Sleep quality deficit from `PhysicalNeeds::rest_quality`. Rises as
    /// the agent accumulates nights of bad sleep without shelter. Satisfied
    /// by the RestInShelter action (which requires proximity to a
    /// ShelterProvider — currently a LeanTo).
    RestQuality,
    /// Stockpile-access deficit from `PhysicalNeeds::food_security`. Rises
    /// when the agent has no surplus food and no nearby stocked chest.
    /// Satisfied by `StockChest` (which requires proximity to a
    /// `StorageChest`); the planner backward-chains through
    /// `BuildStorageChest` when no chest exists yet.
    FoodSecurity,
}

impl UrgencySource {
    /// Whether this drive is handled by the survival brain. Every variant
    /// must be listed explicitly so adding a new one causes a compile error
    /// until someone classifies it.
    pub fn is_survival(self) -> bool {
        self.survival_weight() > 0.0
    }

    /// How much this urgency source contributes to survival brain power.
    /// Reads the drive registry — unregistered sources return 0.0.
    ///
    /// Higher weight = stronger survival brain takeover when this drive
    /// is active. Zero = not a survival drive (emotional/rational handles it).
    pub fn survival_weight(self) -> f32 {
        crate::agent::drive_registry::by_urgency(self)
            .map(|e| e.survival_weight)
            .unwrap_or(0.0)
    }

    /// Whether this urgency source contributes to the deprivation penalty
    /// that impairs rational thought. Only physical deficits (hunger,
    /// thirst, pain, warmth) count — fatigue and sleepiness don't cloud
    /// thinking the same way starvation does.
    pub fn is_deprivation(self) -> bool {
        crate::agent::drive_registry::by_urgency(self)
            .map(|e| e.is_deprivation)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Reflect, serde::Serialize)]
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
    light: Res<crate::world::environment::LightLevel>,
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
            &crate::agent::actions::ActiveActions,
            Option<&crate::agent::brains::plan_memory::PlanMemory>,
        ),
        With<crate::agent::Agent>,
    >,
) {
    for (
        entity,
        mut cns,
        physical,
        consciousness,
        drives,
        emotions,
        body,
        personality,
        active_actions,
        plan_memory,
    ) in query.iter_mut()
    {
        // Staggered: heavy thinking runs every N ticks, offset by entity ID
        if !tick.should_run(entity, ns_config.thinking_interval) {
            continue;
        }

        cns.urgencies.clear();
        cns.sleep_wake_trigger = None;

        // Per-agent forecast horizon — same for every drive iteration.
        let forecast_horizon_minutes =
            crate::agent::nervous_system::forecast::forecast_horizon_minutes(&personality.traits);

        // Helper: Return the "intensity of want" (0..1) for an urgency
        // source. All physical/psychological fields now store satisfaction
        // (high = good), so this is where polarity inversion happens —
        // every satisfaction-type field gets `1.0 - x`, every already-
        // deficit-type input (Pain, Fear) is passed through directly.
        let get_source_value = |source: UrgencySource| -> f32 {
            match source {
                // Already a deficit (high = hungry). Stays as-is.
                UrgencySource::Hunger => physical.hunger_urgency(),
                // Satisfaction → urgency via inversion.
                UrgencySource::Thirst => physical.hydration.deficit(),
                // Stamina is satisfaction (aerobic_fraction() is high=rested);
                // the loop below does the inversion via the Stamina-specific
                // branch so no inversion here.
                UrgencySource::Stamina => physical.stamina.aerobic_fraction(),

                // Pain is already a deficit (high = hurts). Stays as-is.
                UrgencySource::Pain => {
                    let pain = body.map(|b| b.total_pain()).unwrap_or(0.0);
                    (pain / 100.0).clamp(0.0, 1.0)
                }

                // Psychological drives are all satisfaction now — invert
                // each to get "how much the agent wants more of X."
                UrgencySource::Social => drives.map(|d| d.companionship.deficit()).unwrap_or(0.0),
                UrgencySource::Fun => drives.map(|d| d.enjoyment.deficit()).unwrap_or(0.0),
                UrgencySource::Curiosity => drives.map(|d| d.stimulation.deficit()).unwrap_or(0.0),
                UrgencySource::Territoriality => {
                    drives.map(|d| d.dominion.deficit()).unwrap_or(0.0)
                }
                UrgencySource::Fear => emotions
                    .get_emotion_intensity(crate::agent::psyche::emotions::EmotionType::Fear),
                // Wakefulness is satisfaction (high = rested); the loop
                // below handles the Sleepiness-specific inversion just
                // like Stamina, so return the raw satisfaction value here.
                UrgencySource::Sleepiness => physical.wakefulness.value,
                // Warmth is satisfaction (high = comfortable); the loop
                // below inverts it like Stamina/Sleepiness so cold (low
                // value) maps to high urgency.
                UrgencySource::Warmth => physical.warmth.value,
                // RestQuality is satisfaction (high = well-rested); the
                // loop below inverts it like Warmth so poor sleep (low
                // value) maps to high urgency.
                UrgencySource::RestQuality => physical.rest_quality.value,
                UrgencySource::FoodSecurity => physical.food_security.value,
                // Commitment urgency is emitted directly below the drive
                // loop, not through the source-value map, because its
                // magnitude comes from PlanMemory not body/drive state.
                UrgencySource::Commitment => 0.0,
            }
        };

        // --- GENERIC LOOP OVER ALL DRIVE CONFIGS ---
        for drive_config in &ns_config.drives {
            // 1. Get Base Input (Hardcoded Mapping)
            let base_input = get_source_value(drive_config.source);

            // For Stamina, "High Fatigue" means "Low Stamina"
            // We handle inversion here specifically for Stamina if needed, or rely on config curve.
            // Actually, config response curve handles the mapping from Input -> Urgency.
            // e.g. If Input is Stamina (High = 1.0), and we want Urgency when Low,
            // we probably need an explicit Invert flag in config or handle it here?
            // The previous code had `invert` in config. Let's rely on standard logic:
            // Urgency = f(Needs). High Need = High Urgency.
            // Hunger: High Value = High Need.
            // Stamina: Low Value = High Need.

            let current_normalized_input = match drive_config.source {
                // Satisfaction-polarity fields: high value = low urgency.
                UrgencySource::Stamina => 1.0 - base_input,
                UrgencySource::Sleepiness => 1.0 - base_input,
                UrgencySource::Warmth => 1.0 - base_input,
                UrgencySource::RestQuality => 1.0 - base_input,
                UrgencySource::FoodSecurity => 1.0 - base_input,
                _ => base_input,
            };

            // Forward-projected urgency: opted-in drives lift their input
            // before a deficit lands. Max keeps the live signal as a floor.
            let normalized_input =
                match crate::agent::nervous_system::forecast::predicted_normalized_input(
                    drive_config.source,
                    physical,
                    forecast_horizon_minutes,
                    tick.current,
                ) {
                    Some(predicted) => current_normalized_input.max(predicted),
                    None => current_normalized_input,
                };

            // Sleep wake pathway: compare the pre-gated raw input against the
            // drive's wake threshold. This runs before gating/curves/sensitivity
            // because real-life wake pathways (nociception, amygdala alarm,
            // starvation cortisol) respond to raw stimulus strength, not to
            // the attenuated conscious-perception signal. The brain reads
            // `cns.sleep_wake_trigger` to decide whether to rouse a sleeper.
            if let Some(threshold) = drive_config.sleep_wake_threshold
                && normalized_input >= threshold
                && cns.sleep_wake_trigger.is_none()
            {
                cns.sleep_wake_trigger = Some(drive_config.source);
            }

            // If base constant is non-zero, it might override or add to input.
            // Kept as a general mechanism for drives that want a hard floor.
            // Curiosity used to be constant-baseline (#338 follow-up);
            // it now uses the real drives.curiosity state.
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

            // 5. Circadian dampening for Sleepiness. At full daylight
            //    (light = 1.0) the score is multiplied by `1 -
            //    SLEEPINESS_DAYLIGHT_DAMPEN` (= 0.5 by default), so a
            //    rested-but-tiring agent doesn't fall asleep mid-harvest
            //    at noon. At full darkness (light = 0.3) the dampening
            //    vanishes and the full sigmoid score fires. Anchors
            //    sleep to the late-night window in concert with the
            //    sigmoid midpoint at 0.85 and the stronger
            //    `ADENOSINE_RATE` / slower `SLEEP_RESTORE_RATE`.
            if drive_config.source == UrgencySource::Sleepiness {
                let normalized_light = ((light.0 - 0.3) / 0.7).clamp(0.0, 1.0);
                let dampen = normalized_light
                    * crate::constants::brains::wakefulness::SLEEPINESS_DAYLIGHT_DAMPEN;
                score *= 1.0 - dampen;
            }

            // 6. Clamp and threshold
            score = score.max(0.0);
            if score > drive_config.min_threshold {
                cns.urgencies.push(Urgency::new(drive_config.source, score));
            }
        }

        // --- COMMITMENT URGENCY ---
        //
        // Emit one `UrgencySource::Commitment` urgency for the highest-
        // commitment verbal-commitment plan held in memory. Priority
        // scales with conscientiousness using the same formula the old
        // `formulate_goals` path used for committed-goal promotion.
        if let Some(memory) = plan_memory {
            use crate::agent::brains::plan_memory::PlanSource;
            let strongest = memory
                .plans
                .iter()
                .filter(|p| matches!(p.source, PlanSource::VerbalCommitment { .. }))
                .map(|p| p.commitment)
                .fold(f32::NEG_INFINITY, f32::max);
            if strongest.is_finite() {
                let priority = crate::agent::nervous_system::cns::VERBAL_COMMITMENT_PRIORITY_BASE
                    + personality.traits.conscientiousness
                        * crate::agent::nervous_system::cns::VERBAL_COMMITMENT_PRIORITY_BONUS;
                cns.urgencies
                    .push(Urgency::new(UrgencySource::Commitment, priority));
            }
        }

        // --- MOMENTUM & CONSCIOUSNESS ---

        // Multiple actions may run in parallel - any of them can grant momentum
        // to its corresponding drive.
        let current_sources: std::collections::HashSet<UrgencySource> = active_actions
            .iter()
            .filter_map(|action| match action.action_type {
                crate::agent::actions::ActionType::Eat => Some(UrgencySource::Hunger),
                crate::agent::actions::ActionType::Drink => Some(UrgencySource::Thirst),
                crate::agent::actions::ActionType::Sleep => Some(UrgencySource::Sleepiness),
                crate::agent::actions::ActionType::WarmUp => Some(UrgencySource::Warmth),
                crate::agent::actions::ActionType::RestInShelter => {
                    Some(UrgencySource::RestQuality)
                }
                crate::agent::actions::ActionType::StockChest => Some(UrgencySource::FoodSecurity),
                crate::agent::actions::ActionType::Wander => Some(UrgencySource::Curiosity),
                crate::agent::actions::ActionType::Explore => Some(UrgencySource::Curiosity),
                crate::agent::actions::ActionType::Observe => Some(UrgencySource::Curiosity),
                _ => None,
            })
            .collect();

        apply_momentum_and_gating(
            &mut cns.urgencies,
            &current_sources,
            consciousness.alertness,
            &ns_config,
        );

        // Sort Highest Urgency First
        cns.urgencies.sort_by(|a, b| {
            b.value
                .partial_cmp(&a.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

/// Applies momentum bonus to the currently-active drives and consciousness gating to all others.
fn apply_momentum_and_gating(
    urgencies: &mut [Urgency],
    current_sources: &std::collections::HashSet<UrgencySource>,
    alertness: f32,
    ns_config: &NervousSystemConfig,
) {
    for urgency in urgencies.iter_mut() {
        let is_current_drive = current_sources.contains(&urgency.source);
        if is_current_drive {
            urgency.value *= ns_config.momentum_bonus;
        }

        if !is_current_drive {
            let bypass = ns_config
                .get_drive(urgency.source)
                .map(|d| d.bypasses_gating)
                .unwrap_or(false);

            if !bypass {
                // Channel dampening: how much consciousness reduces non-active drives
                let channel_dampening = if ns_config.interoception.sources.contains(&urgency.source)
                {
                    0.6 + (alertness * 0.4) // Interoception (Hunger/Pain): hard to ignore
                } else if ns_config.exteroception.sources.contains(&urgency.source) {
                    alertness // Exteroception (Social/Fear): requires being awake
                } else if ns_config.proprioception.sources.contains(&urgency.source) {
                    0.2 + (alertness * 0.8) // Proprioception (Stamina): moderate gating
                } else {
                    0.1 + (alertness * 0.9) // Default
                };

                urgency.value *= channel_dampening;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Low hydration must produce a high raw thirst input; full hydration must
    /// produce zero. This pins the inversion polarity: hydration is stored as
    /// satisfaction (high = good) and urgency generation inverts it.
    #[test]
    fn low_hydration_gives_high_thirst_input() {
        use crate::agent::body::need::Need;
        assert!(
            Need::new(0.1).deficit() > 0.85,
            "hydration 0.1 → thirst input {:.2}, expected > 0.85",
            Need::new(0.1).deficit()
        );
        assert_eq!(
            Need::full().deficit(),
            0.0,
            "fully hydrated agent must have zero thirst input"
        );
    }
}
