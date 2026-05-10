//! Closed-form predictors of world state at a future tick.
//!
//! Reads: nothing (pure functions of tick + thermal/light constants)
//! Writes: nothing
//! Upstream: world::environment (light schedule), world::field_grid_plugin
//!           (ambient blend), core::time (tick layout)
//! Downstream: agent::nervous_system::forecast (forward-projected urgency)

use bevy::prelude::*;

use crate::constants::thermal::LIGHT_AT_NIGHT;
use crate::core::GameTime;
use crate::world::environment::compute_light_level;
use crate::world::field_grid_plugin::ambient_for_light;

/// Closed-form world-state predictors. Empty struct: every method is
/// associated. Lives as a resource so future weather / season state has
/// a natural home and so callers express the dependency as
/// `Res<WorldForecast>`.
#[derive(Resource, Debug, Default, Clone)]
pub struct WorldForecast;

impl WorldForecast {
    /// Wall-clock hour (0.0..24.0) at the given absolute tick.
    pub fn hour_at(tick: u64) -> f32 {
        GameTime::hour_at_tick(tick)
    }

    /// Light level (0.3..=1.0) at the given absolute tick.
    pub fn light_at(tick: u64) -> f32 {
        compute_light_level(Self::hour_at(tick))
    }

    /// Ambient air temperature (Celsius) at the given absolute tick.
    pub fn ambient_temperature_at(tick: u64) -> f32 {
        ambient_for_light(Self::light_at(tick))
    }

    /// True iff the given tick falls in the night band (light pinned to
    /// the floor).
    pub fn is_night_at(tick: u64) -> bool {
        Self::light_at(tick) <= LIGHT_AT_NIGHT + f32::EPSILON
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::thermal::{DAY_AMBIENT_C, NIGHT_AMBIENT_C};

    /// Builds a tick that lands at wall-clock `hour` on day 1.
    fn tick_for_hour(hour: u64) -> u64 {
        let target = hour * GameTime::MINUTES_PER_HOUR;
        let start = GameTime::START_HOUR * GameTime::MINUTES_PER_HOUR;
        let delta = if target >= start {
            target - start
        } else {
            (24 * GameTime::MINUTES_PER_HOUR) - start + target
        };
        delta * GameTime::TICKS_PER_MINUTE
    }

    #[test]
    fn hour_at_tracks_wall_clock_offset() {
        assert!((WorldForecast::hour_at(0) - GameTime::START_HOUR as f32).abs() < 1e-3);
        let twelve_hours = 12 * GameTime::TICKS_PER_HOUR;
        let expected = (GameTime::START_HOUR as f32 + 12.0) % 24.0;
        assert!((WorldForecast::hour_at(twelve_hours) - expected).abs() < 1e-3);
    }

    #[test]
    fn light_at_noon_is_full_brightness() {
        assert!((WorldForecast::light_at(tick_for_hour(12)) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn light_at_midnight_is_floor() {
        assert!((WorldForecast::light_at(tick_for_hour(0)) - LIGHT_AT_NIGHT).abs() < 1e-3);
    }

    #[test]
    fn ambient_at_noon_is_day_ambient() {
        let temp = WorldForecast::ambient_temperature_at(tick_for_hour(12));
        assert!((temp - DAY_AMBIENT_C).abs() < 1e-3);
    }

    #[test]
    fn ambient_at_deep_night_is_night_ambient() {
        let temp = WorldForecast::ambient_temperature_at(tick_for_hour(2));
        assert!((temp - NIGHT_AMBIENT_C).abs() < 1e-3);
    }

    #[test]
    fn ambient_during_dusk_is_between_day_and_night() {
        // 19:00 falls inside the 18:00-20:00 dusk ramp.
        let temp = WorldForecast::ambient_temperature_at(tick_for_hour(19));
        assert!(temp > NIGHT_AMBIENT_C && temp < DAY_AMBIENT_C);
    }

    #[test]
    fn is_night_flips_at_dusk() {
        assert!(!WorldForecast::is_night_at(tick_for_hour(12)));
        assert!(WorldForecast::is_night_at(tick_for_hour(2)));
    }
}
