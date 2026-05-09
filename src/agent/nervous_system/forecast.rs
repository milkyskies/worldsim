//! Forward-projected urgency: drives can fire on predicted future state.
//!
//! Reads: PhysicalNeeds, PersonalityTraits, WorldForecast (closed-form world predictors)
//! Writes: nothing (returns predicted normalized inputs to the urgency loop)
//! Upstream: world::forecast (WorldForecast resource)
//! Downstream: agent::nervous_system::urgency (max(current, predicted) per drive)
//!
//! See issue #735. Each drive opts in by adding a branch to
//! `predicted_normalized_input`; the urgency loop takes the elementwise
//! max with the current normalized input so a foreseeable shortfall
//! generates urgency before a deficit appears. Conscientiousness scales
//! the lookahead horizon (more conscientious = looks farther ahead =
//! acts earlier).

use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::warmth::cell_temp_to_warmth_rate;
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::agent::psyche::personality::PersonalityTraits;
use crate::core::GameTime;
use crate::world::forecast::WorldForecast;

/// Lookahead horizon in game-minutes for an agent at conscientiousness 0.
/// Even the most reactive agent gets some short-term anticipation so the
/// primitive isn't all-or-nothing.
pub const FORECAST_HORIZON_MIN_MINUTES: f32 = 30.0;
/// Lookahead horizon in game-minutes for an agent at conscientiousness 1.
/// Four game-hours covers "before nightfall from late afternoon" — long
/// enough to plan ahead, short enough that the closed-form approximations
/// stay accurate (no compounded drift, no multi-action chains).
pub const FORECAST_HORIZON_MAX_MINUTES: f32 = 240.0;

/// Lookahead horizon in game-minutes, scaled linearly by conscientiousness.
pub fn forecast_horizon_minutes(traits: &PersonalityTraits) -> f32 {
    let c = traits.conscientiousness.clamp(0.0, 1.0);
    FORECAST_HORIZON_MIN_MINUTES + c * (FORECAST_HORIZON_MAX_MINUTES - FORECAST_HORIZON_MIN_MINUTES)
}

/// Per-drive forward projection. Returns the **predicted normalized
/// input** at the agent's lookahead horizon for any drive that opts in,
/// or `None` if the drive has no predictor. The urgency loop combines
/// this with the live normalized input via `max`, so predicted urgency
/// can lift a drive's score above the current-state value but never
/// suppress it.
///
/// Polarity matches the urgency loop's `normalized_input`: 1.0 = full
/// urgency, 0.0 = fully satisfied.
///
/// New opt-ins: add a branch to the match below. The predictor is a
/// pure function of (current state, lookahead horizon, world forecast)
/// — keep it closed-form, no forward-simulation.
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

/// Project warmth forward using the day-night ambient curve. Assumes the
/// agent is exposed at the horizon (no nearby fire, no shelter delta) —
/// the worst-case ambient. This is the canonical "it's going to be cold
/// tonight, do something now" semantics: an agent comfortable today by
/// a campfire still anticipates that the fire might not be there at
/// dusk and starts acting on warmth before the deficit lands.
fn predicted_warmth_input(current_value: f32, current_tick: u64, horizon_minutes: f32) -> f32 {
    let horizon_ticks = (horizon_minutes * GameTime::TICKS_PER_MINUTE as f32) as u64;
    let future_ambient = WorldForecast::ambient_temperature_at(current_tick + horizon_ticks);
    let rate_per_minute = cell_temp_to_warmth_rate(future_ambient);
    let predicted = (current_value + rate_per_minute * horizon_minutes).clamp(0.0, 1.0);
    (1.0 - predicted).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::psyche::personality::PersonalityTraits;

    #[test]
    fn horizon_scales_with_conscientiousness() {
        let lazy = PersonalityTraits {
            conscientiousness: 0.0,
            ..Default::default()
        };
        let diligent = PersonalityTraits {
            conscientiousness: 1.0,
            ..Default::default()
        };
        assert!((forecast_horizon_minutes(&lazy) - FORECAST_HORIZON_MIN_MINUTES).abs() < 1e-3);
        assert!((forecast_horizon_minutes(&diligent) - FORECAST_HORIZON_MAX_MINUTES).abs() < 1e-3);
        assert!(forecast_horizon_minutes(&diligent) > forecast_horizon_minutes(&lazy));
    }

    /// At noon (warm ambient), projecting full warmth forward stays
    /// satisfied — no anticipatory urgency.
    #[test]
    fn warmth_predictor_quiet_at_noon() {
        // Tick 6 hours after start (06:00 + 6h = 12:00 noon).
        let noon_tick = 6 * GameTime::TICKS_PER_HOUR;
        let predicted = predicted_warmth_input(1.0, noon_tick, 60.0);
        assert!(
            predicted < 0.1,
            "noon warmth forecast should be quiet, got {predicted}"
        );
    }

    /// At 18:00 with a 4-hour horizon (= 22:00, deep night, ambient
    /// well below freezing), an agent at full warmth predicts a serious
    /// shortfall — the predicted normalized input should be high.
    #[test]
    fn warmth_predictor_fires_before_dusk() {
        // 18:00 = 12 game-hours after start.
        let dusk_tick = 12 * GameTime::TICKS_PER_HOUR;
        let predicted = predicted_warmth_input(1.0, dusk_tick, 240.0);
        assert!(
            predicted > 0.5,
            "agent at full warmth before dusk should predict significant shortfall, got {predicted}"
        );
    }

    /// Predicted urgency rises monotonically as the horizon lengthens
    /// when ambient is dropping — longer lookahead = more urgent.
    #[test]
    fn longer_horizon_predicts_more_warmth_urgency_at_dusk() {
        let dusk_tick = 12 * GameTime::TICKS_PER_HOUR;
        let short = predicted_warmth_input(1.0, dusk_tick, 30.0);
        let long = predicted_warmth_input(1.0, dusk_tick, 240.0);
        assert!(
            long > short,
            "longer horizon should predict more shortfall at dusk (short={short}, long={long})"
        );
    }

    /// The forecast's normalized input is in [0, 1] — same polarity as
    /// the urgency loop's other normalized inputs.
    #[test]
    fn predicted_warmth_input_clamps_to_unit_range() {
        let dusk_tick = 12 * GameTime::TICKS_PER_HOUR;
        let value = predicted_warmth_input(0.0, dusk_tick, 240.0);
        assert!((0.0..=1.0).contains(&value), "out of range: {value}");
    }
}
