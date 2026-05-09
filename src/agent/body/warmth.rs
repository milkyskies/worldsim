//! Warmth tick: thermal comfort driven by the tile temperature grid.

use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::body::need::crossed_threshold;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::species::{Species, SpeciesProfile};
use crate::agent::events::{SimEvent, SimEventKind};
use crate::constants::brains::warmth::{
    BASELINE_DRAIN_PER_SEC, COMFORT_THRESHOLD, CRITICAL_THRESHOLD, EXPOSURE_DRAIN_PER_SEC,
    HEAT_RECOVERY_PER_SEC, URGENT_THRESHOLD,
};
use crate::constants::thermal::COMFORT_MIN_C;
use crate::core::tick::TickCount;
use crate::world::field_grid_plugin::FieldGrids;
use crate::world::spatial_index::world_pos_to_tile;

const FREEZING_C: f32 = 0.0;
const COLD_THRESHOLD_C: f32 = 10.0;
const FULL_RECOVERY_C: f32 = 40.0;

/// Ticks warmth for every human agent every tick. Reads the temperature
/// at the agent's tile from the Temperature field grid; maps that to a
/// per-tick warmth delta (drain when cold, recovery when warm). Emits
/// `SimEvent::WarmthChanged` when the value crosses a named threshold
/// so tooling can see the drive pipeline fire.
pub fn tick_warmth(
    tick: Res<TickCount>,
    fields: Res<FieldGrids>,
    mut agents: Query<
        (
            Entity,
            &Transform,
            &mut PhysicalNeeds,
            Option<&SpeciesProfile>,
        ),
        With<Agent>,
    >,
    mut sim_events: MessageWriter<SimEvent>,
) {
    let dt = tick.dt();
    let current_tick = tick.current;
    let grid = fields.temperature();

    for (agent_entity, agent_transform, mut physical, species) in agents.iter_mut() {
        // Only humans have thermal-comfort needs for now; animal warmth
        // would need its own tuning (smaller comfort band, fur, etc.).
        if !matches!(species.map(|s| s.species), Some(Species::Human)) {
            continue;
        }
        let tile = world_pos_to_tile(agent_transform.translation.truncate());
        let cell_temp = grid.sample_tile(tile);
        let rate_per_sec = cell_temp_to_warmth_rate(cell_temp);

        let old = physical.warmth.value;
        physical.warmth.apply_delta(rate_per_sec * dt);
        let new = physical.warmth.value;

        if crossed_threshold(old, new, NAMED_THRESHOLDS) {
            sim_events.write(SimEvent::single(
                current_tick,
                agent_entity,
                SimEventKind::WarmthChanged {
                    agent: agent_entity,
                    old_value: old,
                    new_value: new,
                },
            ));
        }
    }
}

/// Translate a tile's Celsius temperature into a warmth delta per rate-
/// second. Always includes the baseline drain; adds recovery above the
/// comfort floor and exposure drain below the cold threshold.
fn cell_temp_to_warmth_rate(temp_c: f32) -> f32 {
    let recovery = if temp_c >= FULL_RECOVERY_C {
        HEAT_RECOVERY_PER_SEC
    } else if temp_c >= COMFORT_MIN_C {
        let t = (temp_c - COMFORT_MIN_C) / (FULL_RECOVERY_C - COMFORT_MIN_C);
        HEAT_RECOVERY_PER_SEC * t
    } else {
        0.0
    };

    let exposure = if temp_c < COLD_THRESHOLD_C {
        let t = ((COLD_THRESHOLD_C - temp_c) / (COLD_THRESHOLD_C - FREEZING_C)).clamp(0.0, 1.0);
        -EXPOSURE_DRAIN_PER_SEC * t
    } else {
        0.0
    };

    -BASELINE_DRAIN_PER_SEC + recovery + exposure
}

const NAMED_THRESHOLDS: &[f32] = &[COMFORT_THRESHOLD, URGENT_THRESHOLD, CRITICAL_THRESHOLD];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_comfort_threshold_fires() {
        assert!(crossed_threshold(0.65, 0.55, NAMED_THRESHOLDS));
        assert!(crossed_threshold(0.55, 0.65, NAMED_THRESHOLDS));
    }

    #[test]
    fn crossing_urgent_threshold_fires() {
        assert!(crossed_threshold(0.35, 0.25, NAMED_THRESHOLDS));
    }

    #[test]
    fn crossing_critical_threshold_fires() {
        assert!(crossed_threshold(0.15, 0.05, NAMED_THRESHOLDS));
    }

    #[test]
    fn not_crossing_any_threshold_is_quiet() {
        assert!(!crossed_threshold(0.8, 0.75, NAMED_THRESHOLDS));
        assert!(!crossed_threshold(0.5, 0.45, NAMED_THRESHOLDS));
        assert!(!crossed_threshold(0.2, 0.15, NAMED_THRESHOLDS));
    }

    /// At freezing, exposure drain is at its floor (full value) and
    /// recovery is zero — matches the legacy "no heat, no shelter" case.
    #[test]
    fn freezing_cell_drains_baseline_plus_full_exposure() {
        let rate = cell_temp_to_warmth_rate(FREEZING_C);
        assert!((rate - (-BASELINE_DRAIN_PER_SEC - EXPOSURE_DRAIN_PER_SEC)).abs() < 1e-6);
    }

    /// In the cool band (between cold threshold and comfort), only the
    /// baseline drain applies — no exposure, no recovery.
    #[test]
    fn cool_band_drains_baseline_only() {
        let rate = cell_temp_to_warmth_rate(15.0);
        assert!((rate - (-BASELINE_DRAIN_PER_SEC)).abs() < 1e-6);
    }

    /// Above comfort, recovery ramps in linearly. At `FULL_RECOVERY_C`,
    /// it offsets the baseline with the full heat-recovery rate.
    #[test]
    fn at_full_recovery_temp_net_rate_is_recovery_minus_baseline() {
        let rate = cell_temp_to_warmth_rate(FULL_RECOVERY_C);
        assert!((rate - (HEAT_RECOVERY_PER_SEC - BASELINE_DRAIN_PER_SEC)).abs() < 1e-6);
    }

    /// Midpoint of the ramp gives half recovery minus baseline.
    #[test]
    fn midway_recovery_is_half_heat_minus_baseline() {
        let mid = (COMFORT_MIN_C + FULL_RECOVERY_C) / 2.0;
        let rate = cell_temp_to_warmth_rate(mid);
        let expected = 0.5 * HEAT_RECOVERY_PER_SEC - BASELINE_DRAIN_PER_SEC;
        assert!((rate - expected).abs() < 1e-6);
    }

    /// Sanity: sub-zero temperature saturates at the same floor as
    /// freezing.
    #[test]
    fn subzero_does_not_overflow_exposure() {
        let rate_freezing = cell_temp_to_warmth_rate(FREEZING_C);
        let rate_subzero = cell_temp_to_warmth_rate(FREEZING_C - 20.0);
        assert!((rate_freezing - rate_subzero).abs() < 1e-6);
    }
}
