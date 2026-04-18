use bevy::prelude::*;

/// Game time derived from tick count
/// 60 ticks = 1 game second
#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
pub struct GameTime {
    /// Game seconds (1 second = 60 ticks)
    pub seconds: u64,
    /// Game minutes
    pub minutes: u32,
    /// Game hours (0-23)
    pub hours: u32,
    /// Game days
    pub days: u32,
}

impl GameTime {
    /// Ticks per game second (1 tick = 1 game second, so 60 ticks = 1 game minute)
    /// At 60 ticks/sec real-time, this means 1 real second = 1 game minute (RimWorld-like)
    pub const TICKS_PER_SECOND: u64 = 1;
    pub const SECONDS_PER_MINUTE: u64 = 60;
    pub const MINUTES_PER_HOUR: u64 = 60;
    pub const HOURS_PER_DAY: u64 = 24;

    /// 60 ticks per game minute.
    pub const TICKS_PER_MINUTE: u64 = Self::TICKS_PER_SECOND * Self::SECONDS_PER_MINUTE;
    /// 3,600 ticks per game hour.
    pub const TICKS_PER_HOUR: u64 = Self::TICKS_PER_MINUTE * Self::MINUTES_PER_HOUR;
    /// 86,400 ticks per game day. At the default 60 real ticks/sec, 1 game day = 24 real minutes.
    pub const TICKS_PER_DAY: u64 = Self::TICKS_PER_HOUR * Self::HOURS_PER_DAY;

    /// Wall-clock hour the simulation begins at (both `cargo run` and
    /// `--headless`). 8am sits agents down to their first breakfast and
    /// leaves a full day before the first natural bedtime.
    pub const START_HOUR: u64 = 8;
    /// `START_HOUR * MINUTES_PER_HOUR * SECONDS_PER_MINUTE * TICKS_PER_SECOND`
    /// — the offset applied to raw tick counts when computing wall-clock
    /// `hours/minutes/seconds` / `days` fields.
    pub const INITIAL_TICK_OFFSET: u64 = Self::START_HOUR
        * Self::MINUTES_PER_HOUR
        * Self::SECONDS_PER_MINUTE
        * Self::TICKS_PER_SECOND;

    /// Update game time from tick count
    pub fn update_from_tick(&mut self, tick: u64) {
        let total_ticks = tick + Self::INITIAL_TICK_OFFSET;
        self.seconds = total_ticks / Self::TICKS_PER_SECOND;

        // ... (rest of calcs use self.seconds which is now offset)
        let total_minutes = self.seconds / Self::SECONDS_PER_MINUTE;
        let total_hours = total_minutes / Self::MINUTES_PER_HOUR;

        self.minutes = (total_minutes % Self::MINUTES_PER_HOUR) as u32;
        self.hours = (total_hours % Self::HOURS_PER_DAY) as u32;
        self.days = (total_hours / Self::HOURS_PER_DAY) as u32;
    }

    /// Format as HH:MM:SS
    pub fn format(&self) -> String {
        let secs = self.seconds % Self::SECONDS_PER_MINUTE;
        format!(
            "Day {} {:02}:{:02}:{:02}",
            self.days + 1,
            self.hours,
            self.minutes,
            secs
        )
    }

    /// Static helper to format raw ticks into [HH:MM] string
    pub fn format_tick(tick: u64) -> String {
        let total_ticks = tick + Self::INITIAL_TICK_OFFSET;
        let seconds = total_ticks / Self::TICKS_PER_SECOND;
        let total_minutes = seconds / Self::SECONDS_PER_MINUTE;
        let total_hours = total_minutes / Self::MINUTES_PER_HOUR;

        let minutes = total_minutes % Self::MINUTES_PER_HOUR;
        let hours = total_hours % Self::HOURS_PER_DAY;

        format!("[{:02}:{:02}]", hours, minutes)
    }
}
