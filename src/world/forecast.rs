//! Closed-form predictors for world state at a future tick.
//!
//! Reads: nothing (pure functions of tick + thermal/light constants)
//! Writes: nothing
//! Upstream: world::environment (light schedule), core::time (tick layout),
//!           constants::thermal (ambient blend constants)
//! Downstream: agent::nervous_system::forecast (forward-projected urgency)
//!
//! The day-night cycle is a deterministic function of tick. So is the
//! ambient temperature curve that rides on top of it. We expose tiny
//! arithmetic accessors here so any system that wants to reason about
//! "what the world will look like at tick T" can do so without running
//! the simulation forward. This is the foundation for the anticipation
//! primitive (issue #735).

use bevy::prelude::*;

use crate::constants::thermal::{DAY_AMBIENT_C, LIGHT_AT_NIGHT, NIGHT_AMBIENT_C};
use crate::core::GameTime;
use crate::world::environment::compute_light_level;

/// Closed-form world-state predictors. Empty struct today — every method
/// is associated. Lives as a resource so future weather / season state
/// has a natural home, and so callers express the dependency as
/// `Res<WorldForecast>` rather than reaching into freestanding functions.
#[derive(Resource, Debug, Default, Clone)]
pub struct WorldForecast;

impl WorldForecast {
    /// Game-hour (0.0..24.0) at the given absolute tick, including
    /// fractional minutes. Mirrors `GameTime::update_from_tick` arithmetic.
    pub fn hour_at(tick: u64) -> f32 {
        let total_ticks = tick + GameTime::INITIAL_TICK_OFFSET;
        let total_seconds = total_ticks / GameTime::TICKS_PER_SECOND;
        let total_minutes = total_seconds / GameTime::SECONDS_PER_MINUTE;
        let hour_of_day = (total_minutes / GameTime::MINUTES_PER_HOUR) % GameTime::HOURS_PER_DAY;
        let minute_of_hour = total_minutes % GameTime::MINUTES_PER_HOUR;
        hour_of_day as f32 + (minute_of_hour as f32) / 60.0
    }

    /// Light level (0.3..=1.0) at the given absolute tick. Same curve as
    /// `world::environment::compute_light_level`.
    pub fn light_at(tick: u64) -> f32 {
        compute_light_level(Self::hour_at(tick))
    }

    /// Ambient air temperature (Celsius) at the given absolute tick.
    /// Mirrors `field_grid_plugin::update_thermal_ambient`: the night→day
    /// blend is keyed on the light level above the night floor.
    pub fn ambient_temperature_at(tick: u64) -> f32 {
        let light = Self::light_at(tick);
        let t = ((light - LIGHT_AT_NIGHT) / (1.0 - LIGHT_AT_NIGHT)).clamp(0.0, 1.0);
        NIGHT_AMBIENT_C + (DAY_AMBIENT_C - NIGHT_AMBIENT_C) * t
    }

    /// True iff the given tick falls in the night band (light pinned to
    /// the floor). Useful for "do I have shelter for tonight?" checks.
    pub fn is_night_at(tick: u64) -> bool {
        Self::light_at(tick) <= LIGHT_AT_NIGHT + f32::EPSILON
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick_for_hour(hour: u64) -> u64 {
        // Game starts at START_HOUR (= 6). Tick 0 ≙ 06:00. Build a tick
        // that lands at the requested wall-clock hour the same day.
        let target_minutes_since_midnight = hour * GameTime::MINUTES_PER_HOUR;
        let start_minutes_since_midnight = GameTime::START_HOUR * GameTime::MINUTES_PER_HOUR;
        let delta_minutes = if target_minutes_since_midnight >= start_minutes_since_midnight {
            target_minutes_since_midnight - start_minutes_since_midnight
        } else {
            (24 * GameTime::MINUTES_PER_HOUR) - start_minutes_since_midnight
                + target_minutes_since_midnight
        };
        delta_minutes * GameTime::TICKS_PER_MINUTE
    }

    #[test]
    fn hour_at_tracks_wall_clock_offset() {
        // Tick 0 lands at the simulation start hour.
        assert!((WorldForecast::hour_at(0) - GameTime::START_HOUR as f32).abs() < 1e-3);
        // Twelve game-hours after start lands twelve hours later.
        let twelve_hours = 12 * GameTime::TICKS_PER_HOUR;
        let expected = (GameTime::START_HOUR as f32 + 12.0) % 24.0;
        assert!((WorldForecast::hour_at(twelve_hours) - expected).abs() < 1e-3);
    }

    #[test]
    fn light_at_noon_is_full_brightness() {
        let noon_tick = tick_for_hour(12);
        assert!((WorldForecast::light_at(noon_tick) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn light_at_midnight_is_floor() {
        let midnight_tick = tick_for_hour(0);
        assert!((WorldForecast::light_at(midnight_tick) - LIGHT_AT_NIGHT).abs() < 1e-3);
    }

    #[test]
    fn ambient_at_noon_is_day_ambient() {
        let noon_tick = tick_for_hour(12);
        let temp = WorldForecast::ambient_temperature_at(noon_tick);
        assert!(
            (temp - DAY_AMBIENT_C).abs() < 1e-3,
            "noon ambient should equal DAY_AMBIENT_C, got {temp}"
        );
    }

    #[test]
    fn ambient_at_deep_night_is_night_ambient() {
        let midnight_tick = tick_for_hour(2);
        let temp = WorldForecast::ambient_temperature_at(midnight_tick);
        assert!(
            (temp - NIGHT_AMBIENT_C).abs() < 1e-3,
            "deep-night ambient should equal NIGHT_AMBIENT_C, got {temp}"
        );
    }

    #[test]
    fn ambient_during_dusk_is_between_day_and_night() {
        // 19:00 sits inside the 18:00-20:00 dusk ramp.
        let dusk_tick = tick_for_hour(19);
        let temp = WorldForecast::ambient_temperature_at(dusk_tick);
        assert!(
            temp > NIGHT_AMBIENT_C && temp < DAY_AMBIENT_C,
            "dusk ambient {temp} should fall strictly between night and day"
        );
    }

    #[test]
    fn is_night_flips_at_dusk() {
        let day_tick = tick_for_hour(12);
        let deep_night_tick = tick_for_hour(2);
        assert!(!WorldForecast::is_night_at(day_tick));
        assert!(WorldForecast::is_night_at(deep_night_tick));
    }
}
