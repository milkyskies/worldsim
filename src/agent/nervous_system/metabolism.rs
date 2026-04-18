//! BMR drain + stomach digestion + reserve mobilization — the per-tick
//! metabolism update that runs for every living agent independent of what
//! they are doing.
//!
//! Reads: PhysicalNeeds, Consciousness, Body, Phenotype, TickCount
//! Writes: PhysicalNeeds (metabolism pools, aerobic/anaerobic recovery)
//! Upstream: core::tick (TickCount)
//! Downstream: nervous_system::urgency (reads updated needs to recalculate urgencies)

use crate::agent::Alive;
use crate::agent::biology::body::Body;
use crate::agent::body::genetics::phenotype::Phenotype;
use crate::agent::body::metabolism::{BMR_GLUCOSE_DRAIN_PER_SEC, BMR_HYDRATION_DRAIN_PER_SEC};
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::core::TickCount;
use bevy::prelude::*;

/// Per-tick metabolism update for **every** agent with `PhysicalNeeds`.
///
/// Runs the basal drain (BMR scaled by consciousness and phenotype),
/// digests the stomach, and does a slow anaerobic passive refill so a
/// Flee sprint doesn't leave the pool stuck at 0.
pub fn tick_metabolism(
    tick: Res<TickCount>,
    mut query: Query<
        (
            &mut PhysicalNeeds,
            &Consciousness,
            Option<&Body>,
            Option<&Phenotype>,
        ),
        With<Alive>,
    >,
) {
    let dt = tick.dt();
    for (mut physical, consciousness, body, phenotype) in query.iter_mut() {
        let mut organ_mods = body.map(Body::organ_mods).unwrap_or_default();
        let digestion_mult = phenotype.map(|p| p.digestion).unwrap_or(1.0);
        organ_mods.stomach *= digestion_mult;
        organ_mods.gut *= digestion_mult;
        let bmr_mult = phenotype.map(|p| p.bmr).unwrap_or(1.0);
        // BMR splits into somatic floor (60% — organs, thermoregulation)
        // and consciousness cost (40% — brain processing). During sleep
        // alertness drops to ~0, reducing effective BMR to ~60% of awake
        // rate. Matches the real ~15-20% metabolic reduction during sleep.
        let consciousness_factor = 0.6 + 0.4 * consciousness.alertness;
        physical.metabolism.tick_with_mods(
            dt,
            BMR_GLUCOSE_DRAIN_PER_SEC * bmr_mult * consciousness_factor,
            0.0,
            organ_mods,
        );

        // Sleeping agents lose less water: no sweat, slower breathing, and no
        // renal activity driven by movement. Real biology drops hydration loss
        // by ~40-60%, but BMR_HYDRATION_DRAIN_PER_SEC is tuned aggressively
        // for awake gameplay (drink-every-1.5h) so the 60% awake rate would
        // still empty a full pool within a normal 6-8h bout. A dedicated
        // hydration-sleep multiplier lets the agent finish the night without
        // the emergency-wake pathway firing.
        let hydration_sleep_factor = 0.3 + 0.7 * consciousness.alertness;
        physical
            .hydration
            .drain(BMR_HYDRATION_DRAIN_PER_SEC * hydration_sleep_factor * dt);

        // Slow passive anaerobic refill so a Flee sprint doesn't leave
        // the pool stuck at 0 forever. The rate is low enough that the
        // Survival brain still sees a Stamina urgency window and can
        // propose Rest/Sleep — removing the signal entirely made agents
        // skip every fatigue cycle and burn surplus glucose into
        // early starvation.
        physical.stamina.anaerobic =
            (physical.stamina.anaerobic + 0.02).min(physical.stamina.anaerobic_max);
    }
}
