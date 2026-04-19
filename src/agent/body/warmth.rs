//! Warmth tick: thermal comfort drains with exposure and recovers near heat.
//!
//! Reads: Transform, HeatSource (world), ShelterProvider (world), TickCount,
//!        SpatialIndex
//! Writes: PhysicalNeeds.warmth, SimEvent::WarmthChanged
//! Upstream: world::property (HeatSource, ShelterProvider), core::tick
//! Downstream: nervous_system::urgency (UrgencySource::Warmth reads the value),
//!             invariants (`warmth ∈ [0.0, 1.0]`)

use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::events::{SimEvent, SimEventKind};
use crate::constants::brains::warmth::{
    BASELINE_DRAIN_PER_SEC, COMFORT_THRESHOLD, CRITICAL_THRESHOLD, EXPOSURE_DRAIN_PER_SEC,
    HEAT_RECOVERY_PER_SEC, SHELTER_RECOVERY_PER_SEC, URGENT_THRESHOLD,
};
use crate::core::tick::TickCount;
use crate::world::property::{HeatSource, ShelterProvider};
use crate::world::spatial_index::SpatialIndex;

/// Maximum scan radius for warmth sources — both heat and shelter. Set
/// generously so the spatial-index narrow-phase isn't the gate; exact
/// protection is decided per-source by its declared `radius` / `protection`
/// field (for heat) or overlap check (for shelter).
const WARMTH_SCAN_RADIUS: f32 = 128.0;

/// Ticks warmth for every agent every tick. Passive recovery when within
/// a lit `HeatSource` radius or inside a `ShelterProvider`; baseline drain
/// always applies; exposure drain adds on when neither protection is
/// present. Emits `SimEvent::WarmthChanged` when the value crosses a
/// named threshold so tooling can see the drive pipeline fire.
pub fn tick_warmth(
    tick: Res<TickCount>,
    spatial_index: Res<SpatialIndex>,
    heat_sources: Query<(&Transform, &HeatSource)>,
    shelters: Query<(&Transform, &ShelterProvider)>,
    mut agents: Query<(Entity, &Transform, &mut PhysicalNeeds), With<Agent>>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    let dt = tick.dt();
    let current_tick = tick.current;

    for (agent_entity, agent_transform, mut physical) in agents.iter_mut() {
        let agent_pos = agent_transform.translation.truncate();
        let old = physical.warmth.value;

        // Scan once for protection factors.
        let mut heat_recovery_rate = 0.0f32;
        let mut in_shelter = false;

        for candidate in spatial_index.entities_near(agent_pos, WARMTH_SCAN_RADIUS) {
            if let Ok((source_transform, heat)) = heat_sources.get(candidate) {
                let source_pos = source_transform.translation.truncate();
                let distance = agent_pos.distance(source_pos);
                if distance > heat.radius {
                    continue;
                }
                // Intensity-weighted recovery, tapered with distance. A cold
                // agent standing right on top of a roaring fire recovers fastest.
                let proximity = 1.0 - (distance / heat.radius).clamp(0.0, 1.0);
                let rate = HEAT_RECOVERY_PER_SEC * heat.intensity * proximity;
                if rate > heat_recovery_rate {
                    heat_recovery_rate = rate;
                }
            }
            if let Ok((shelter_transform, shelter)) = shelters.get(candidate) {
                let shelter_pos = shelter_transform.translation.truncate();
                // Shelter protects anyone within its protection-scaled range.
                if agent_pos.distance(shelter_pos) <= shelter.protection {
                    in_shelter = true;
                }
            }
        }

        // Net delta for this tick.
        let mut delta = -BASELINE_DRAIN_PER_SEC * dt;
        if heat_recovery_rate > 0.0 {
            delta += heat_recovery_rate * dt;
        } else if in_shelter {
            delta += SHELTER_RECOVERY_PER_SEC * dt;
        } else {
            // Exposed: no heat, no shelter. Extra drain on top of baseline.
            delta -= EXPOSURE_DRAIN_PER_SEC * dt;
        }

        physical.warmth.apply_delta(delta);
        let new = physical.warmth.value;

        if crossed_named_threshold(old, new) {
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

/// Returns `true` when `old` and `new` fall on opposite sides of any of the
/// three named thresholds (comfort / urgent / critical). Keeps the SimEvent
/// stream sparse — agents that slowly cool produce one event per band-crossing
/// rather than one per tick.
fn crossed_named_threshold(old: f32, new: f32) -> bool {
    const THRESHOLDS: &[f32] = &[COMFORT_THRESHOLD, URGENT_THRESHOLD, CRITICAL_THRESHOLD];
    THRESHOLDS
        .iter()
        .any(|t| (old >= *t && new < *t) || (old < *t && new >= *t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_comfort_threshold_fires() {
        assert!(crossed_named_threshold(0.65, 0.55));
        assert!(crossed_named_threshold(0.55, 0.65));
    }

    #[test]
    fn crossing_urgent_threshold_fires() {
        assert!(crossed_named_threshold(0.35, 0.25));
    }

    #[test]
    fn crossing_critical_threshold_fires() {
        assert!(crossed_named_threshold(0.15, 0.05));
    }

    #[test]
    fn not_crossing_any_threshold_is_quiet() {
        assert!(!crossed_named_threshold(0.8, 0.75));
        assert!(!crossed_named_threshold(0.5, 0.45));
        assert!(!crossed_named_threshold(0.2, 0.15));
    }
}
