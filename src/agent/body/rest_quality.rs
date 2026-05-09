//! Rest-quality tick. Drains baseline always; recovers proportional to
//! the best nearby `ShelterProvider.protection`. Mirrors the proximity-
//! only recovery pattern that `tick_warmth` uses for `HeatSource`.

use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::body::need::crossed_threshold;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::species::{Species, SpeciesProfile};
use crate::agent::events::{SimEvent, SimEventKind};
use crate::constants::brains::rest_quality::{
    BASELINE_DRAIN_PER_SEC, COMFORT_THRESHOLD, CRITICAL_THRESHOLD, SHELTER_RECOVERY_PER_SEC,
    URGENT_THRESHOLD,
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
            Option<&SpeciesProfile>,
        ),
        With<Agent>,
    >,
    mut sim_events: MessageWriter<SimEvent>,
) {
    let dt = tick.dt();
    let current_tick = tick.current;

    for (agent_entity, agent_transform, mut physical, species) in agents.iter_mut() {
        if !matches!(species.map(|s| s.species), Some(Species::Human)) {
            continue;
        }

        let agent_pos = agent_transform.translation.truncate();
        let best_shelter = best_shelter_protection(agent_pos, &shelter_providers);

        let rate_per_sec = compute_rest_quality_rate(best_shelter);

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

fn compute_rest_quality_rate(best_shelter: f32) -> f32 {
    -BASELINE_DRAIN_PER_SEC + SHELTER_RECOVERY_PER_SEC * best_shelter
}

const NAMED_THRESHOLDS: &[f32] = &[COMFORT_THRESHOLD, URGENT_THRESHOLD, CRITICAL_THRESHOLD];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_shelter_drains_baseline() {
        let rate = compute_rest_quality_rate(0.0);
        assert!((rate - (-BASELINE_DRAIN_PER_SEC)).abs() < 1e-6);
    }

    #[test]
    fn near_shelter_recovers_proportional_to_protection() {
        let rate = compute_rest_quality_rate(1.5);
        let expected = -BASELINE_DRAIN_PER_SEC + SHELTER_RECOVERY_PER_SEC * 1.5;
        assert!((rate - expected).abs() < 1e-6);
        // Recovery dominates — net positive.
        assert!(rate > 0.0);
    }

    #[test]
    fn higher_quality_shelter_recovers_faster() {
        let lean_to = compute_rest_quality_rate(1.5);
        let house = compute_rest_quality_rate(2.5);
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
