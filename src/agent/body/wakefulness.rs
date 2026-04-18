//! Wakefulness tick: adenosine-like sleep pressure that decays while awake and restores during sleep.
//!
//! Reads: ActiveActions, LightLevel, Phenotype, TickCount
//! Writes: PhysicalNeeds.wakefulness, Consciousness.alertness (drag)
//! Upstream: actions::registry (ActiveActions), world::environment (LightLevel)
//! Downstream: nervous_system::urgency (Sleepiness source), brains::survival (sleep/wake gate)

use bevy::prelude::*;

use crate::agent::actions::{ActionType, ActiveActions};
use crate::agent::body::genetics::phenotype::Phenotype;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::constants::brains::wakefulness::{
    ADENOSINE_RATE, ALERTNESS_DRAG_PER_DEFICIT, CIRCADIAN_LIGHT_CEILING, CIRCADIAN_NIGHT_BOOST,
    SLEEP_RESTORE_RATE,
};
use crate::core::tick::TickCount;
use crate::world::environment::LightLevel;

/// Ticks wakefulness for every agent every tick. Decay while awake,
/// restore during Sleep, and passively drag alertness when drowsy.
pub fn tick_wakefulness(
    tick: Res<TickCount>,
    light: Res<LightLevel>,
    mut query: Query<(
        &ActiveActions,
        &mut PhysicalNeeds,
        &mut Consciousness,
        Option<&Phenotype>,
    )>,
) {
    let dt = tick.dt();

    for (active, mut physical, mut consciousness, phenotype) in query.iter_mut() {
        let is_sleeping = active.contains(ActionType::Sleep);

        if is_sleeping {
            let efficiency = phenotype.map(|p| p.sleep_efficiency).unwrap_or(1.0);
            physical
                .wakefulness
                .top_up(SLEEP_RESTORE_RATE * efficiency * dt);
        } else {
            let circadian_multiplier =
                1.0 + CIRCADIAN_NIGHT_BOOST * (CIRCADIAN_LIGHT_CEILING - light.0).max(0.0);
            physical
                .wakefulness
                .drain(ADENOSINE_RATE * circadian_multiplier * dt);
        }

        // Low wakefulness passively drags alertness — a drowsy agent is less
        // perceptive and slower to plan, even before committing to Sleep.
        let deficit = physical.wakefulness.deficit();
        let alertness_cap = 1.0 - deficit * ALERTNESS_DRAG_PER_DEFICIT;
        if consciousness.alertness > alertness_cap && !is_sleeping {
            consciousness.alertness = alertness_cap;
        }
    }
}
