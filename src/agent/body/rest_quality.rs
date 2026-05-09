//! Rest-quality tick. Drains baseline always; while sleeping, recovers
//! proportional to the best nearby `ShelterProvider.protection` or pays
//! an exposure penalty when no shelter is in range.

use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::actions::{ActionType, ActiveActions};
use crate::agent::body::need::crossed_threshold;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::species::{Species, SpeciesProfile};
use crate::agent::events::{SimEvent, SimEventKind};
use crate::constants::brains::rest_quality::{
    BASELINE_DRAIN_PER_SEC, COMFORT_THRESHOLD, CRITICAL_THRESHOLD, EXPOSURE_SLEEP_DRAIN_PER_SEC,
    SHELTER_RECOVERY_PER_SEC, URGENT_THRESHOLD,
};
use crate::core::tick::TickCount;
use crate::world::map::TILE_SIZE;
use crate::world::property::ShelterProvider;

/// Matches `shelter_system`'s aerobic-bonus radius so both systems
/// agree on what "inside a shelter" means.
const SHELTER_RANGE: f32 = TILE_SIZE * 3.0;
pub fn tick_rest_quality(
    tick: Res<TickCount>,
    shelter_providers: Query<(&Transform, &ShelterProvider)>,
    mut agents: Query<
        (
            Entity,
            &Transform,
            &mut PhysicalNeeds,
            &ActiveActions,
            Option<&SpeciesProfile>,
        ),
        With<Agent>,
    >,
    mut sim_events: MessageWriter<SimEvent>,
) {
    let dt = tick.dt();
    let current_tick = tick.current;

    for (agent_entity, agent_transform, mut physical, active, species) in agents.iter_mut() {
        if !matches!(species.map(|s| s.species), Some(Species::Human)) {
            continue;
        }

        let agent_pos = agent_transform.translation.truncate();
        let sleeping = active.contains(ActionType::Sleep);
        let best_shelter = best_shelter_protection(agent_pos, &shelter_providers);

        let rate_per_sec = compute_rest_quality_rate(sleeping, best_shelter);

        let old = physical.rest_quality.value;
        physical.rest_quality.apply_delta(rate_per_sec * dt);
        let new = physical.rest_quality.value;

        if crossed_threshold(old, new, NAMED_THRESHOLDS) {
            sim_events.write(SimEvent::single(
                current_tick,
                agent_entity,
                SimEventKind::RestQualityChanged {
                    agent: agent_entity,
                    old_value: old,
                    new_value: new,
                },
            ));
        }
    }
}

fn best_shelter_protection(
    pos: Vec2,
    shelter_providers: &Query<(&Transform, &ShelterProvider)>,
) -> f32 {
    shelter_providers
        .iter()
        .filter(|(transform, _)| transform.translation.truncate().distance(pos) <= SHELTER_RANGE)
        .map(|(_, provider)| provider.protection)
        .fold(0.0_f32, f32::max)
}

fn compute_rest_quality_rate(sleeping: bool, best_shelter: f32) -> f32 {
    let baseline = -BASELINE_DRAIN_PER_SEC;
    if !sleeping {
        return baseline;
    }
    if best_shelter > 0.0 {
        baseline + SHELTER_RECOVERY_PER_SEC * best_shelter
    } else {
        baseline - EXPOSURE_SLEEP_DRAIN_PER_SEC
    }
}

const NAMED_THRESHOLDS: &[f32] = &[COMFORT_THRESHOLD, URGENT_THRESHOLD, CRITICAL_THRESHOLD];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awake_agent_drains_baseline_only() {
        let rate = compute_rest_quality_rate(false, 0.0);
        assert!((rate - (-BASELINE_DRAIN_PER_SEC)).abs() < 1e-6);
    }

    #[test]
    fn awake_agent_near_shelter_still_drains_baseline() {
        // Recovery is gated on actually sleeping — standing under a roof
        // doesn't count as rest.
        let rate = compute_rest_quality_rate(false, 1.5);
        assert!((rate - (-BASELINE_DRAIN_PER_SEC)).abs() < 1e-6);
    }

    #[test]
    fn sleeping_unsheltered_drains_baseline_plus_exposure() {
        let rate = compute_rest_quality_rate(true, 0.0);
        let expected = -BASELINE_DRAIN_PER_SEC - EXPOSURE_SLEEP_DRAIN_PER_SEC;
        assert!((rate - expected).abs() < 1e-6);
    }

    #[test]
    fn sleeping_in_shelter_recovers_proportional_to_protection() {
        let rate = compute_rest_quality_rate(true, 1.5);
        let expected = -BASELINE_DRAIN_PER_SEC + SHELTER_RECOVERY_PER_SEC * 1.5;
        assert!((rate - expected).abs() < 1e-6);
        // Recovery dominates — net positive even after baseline drain.
        assert!(rate > 0.0);
    }

    #[test]
    fn sleeping_in_higher_quality_shelter_recovers_faster() {
        let lean_to = compute_rest_quality_rate(true, 1.5);
        let house = compute_rest_quality_rate(true, 2.5);
        assert!(house > lean_to);
    }

    #[test]
    fn crossing_comfort_threshold_fires() {
        assert!(crossed_threshold(0.65, 0.55, NAMED_THRESHOLDS));
        assert!(crossed_threshold(0.55, 0.65, NAMED_THRESHOLDS));
    }

    #[test]
    fn not_crossing_any_threshold_is_quiet() {
        assert!(!crossed_threshold(0.8, 0.75, NAMED_THRESHOLDS));
        assert!(!crossed_threshold(0.5, 0.45, NAMED_THRESHOLDS));
    }
}
