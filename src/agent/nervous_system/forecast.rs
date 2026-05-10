//! Forward-projected urgency: drives can fire on predicted future state.
//!
//! Reads: PhysicalNeeds, PersonalityTraits, WorldForecast
//! Writes: nothing
//! Upstream: world::forecast
//! Downstream: agent::nervous_system::urgency

use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::warmth::project_warmth_value;
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::agent::psyche::personality::PersonalityTraits;
use crate::core::GameTime;
use crate::world::forecast::WorldForecast;

/// Lookahead floor in game-minutes. Agents at conscientiousness 0 still
/// get half-an-hour of foresight — short enough that the live signal
/// dominates, long enough that imminent shortfall (e.g. ambient about to
/// drop in the next dusk window) registers.
pub const FORECAST_HORIZON_MIN_MINUTES: f32 = 30.0;
/// Lookahead ceiling in game-minutes. Four game-hours covers
/// "before nightfall from late afternoon" — long enough to plan ahead,
/// short enough that the closed-form approximations stay accurate.
pub const FORECAST_HORIZON_MAX_MINUTES: f32 = 240.0;

/// Lookahead horizon in game-minutes, scaled linearly by conscientiousness.
pub fn forecast_horizon_minutes(traits: &PersonalityTraits) -> f32 {
    let c = traits.conscientiousness().clamp(0.0, 1.0);
    FORECAST_HORIZON_MIN_MINUTES + c * (FORECAST_HORIZON_MAX_MINUTES - FORECAST_HORIZON_MIN_MINUTES)
}

/// Predicted normalized input for a drive. Returns `None` for drives
/// without a predictor — callers should fall back to the live signal.
/// Polarity matches `urgency::generate_urgency`'s `normalized_input`:
/// 1.0 = full urgency, 0.0 = fully satisfied.
pub fn predicted_normalized_input(
    source: UrgencySource,
    physical: &PhysicalNeeds,
    horizon_minutes: f32,
    current_tick: u64,
) -> Option<f32> {
    match source {
        UrgencySource::Warmth => Some(predicted_warmth_input(
            physical.warmth.value,
            current_tick,
            horizon_minutes,
        )),
        _ => None,
    }
}

/// Project warmth using the day-night ambient curve, assuming the agent
/// is exposed at the horizon (no nearby fire, no shelter delta). The
/// worst-case framing is intentional: an agent comfortable now by a fire
/// still anticipates the fire might be out at dusk and plans accordingly.
fn predicted_warmth_input(current_value: f32, current_tick: u64, horizon_minutes: f32) -> f32 {
    let horizon_ticks = (horizon_minutes * GameTime::TICKS_PER_MINUTE as f32) as u64;
    let future_ambient = WorldForecast::ambient_temperature_at(current_tick + horizon_ticks);
    let predicted = project_warmth_value(current_value, future_ambient, horizon_minutes);
    (1.0 - predicted).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::psyche::personality::{ConscientiousnessFacets, PersonalityTraits};

    #[test]
    fn horizon_scales_with_conscientiousness() {
        let lazy = PersonalityTraits {
            conscientiousness: ConscientiousnessFacets::uniform(0.0),
            ..Default::default()
        };
        let diligent = PersonalityTraits {
            conscientiousness: ConscientiousnessFacets::uniform(1.0),
            ..Default::default()
        };
        assert!((forecast_horizon_minutes(&lazy) - FORECAST_HORIZON_MIN_MINUTES).abs() < 1e-3);
        assert!((forecast_horizon_minutes(&diligent) - FORECAST_HORIZON_MAX_MINUTES).abs() < 1e-3);
        assert!(forecast_horizon_minutes(&diligent) > forecast_horizon_minutes(&lazy));
    }

    #[test]
    fn warmth_predictor_quiet_at_noon() {
        // 06:00 start + 6h = 12:00.
        let noon_tick = 6 * GameTime::TICKS_PER_HOUR;
        assert!(predicted_warmth_input(1.0, noon_tick, 60.0) < 0.1);
    }

    #[test]
    fn warmth_predictor_fires_before_dusk() {
        // 18:00, 4h horizon → projects across dusk into deep cold.
        let dusk_tick = 12 * GameTime::TICKS_PER_HOUR;
        assert!(predicted_warmth_input(1.0, dusk_tick, 240.0) > 0.5);
    }

    #[test]
    fn longer_horizon_predicts_more_warmth_urgency_at_dusk() {
        let dusk_tick = 12 * GameTime::TICKS_PER_HOUR;
        let short = predicted_warmth_input(1.0, dusk_tick, 30.0);
        let long = predicted_warmth_input(1.0, dusk_tick, 240.0);
        assert!(long > short);
    }

    #[test]
    fn predicted_warmth_input_clamps_to_unit_range() {
        let dusk_tick = 12 * GameTime::TICKS_PER_HOUR;
        let value = predicted_warmth_input(0.0, dusk_tick, 240.0);
        assert!((0.0..=1.0).contains(&value));
    }
}
