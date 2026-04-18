use bevy::prelude::*;

/// Tracks the simulation tick count
#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
pub struct TickCount {
    /// Current tick number (0, 1, 2, ...)
    pub current: u64,
    /// Current speed in ticks per wall-clock second. Read by `time_controls`
    /// and the UI speed buttons; written to `Time<Fixed>::set_timestep_hz`
    /// to control how many FixedUpdate cycles Bevy runs per frame.
    pub ticks_per_second: f32,
    /// Whether simulation is paused
    pub paused: bool,
}

impl TickCount {
    pub fn new(ticks_per_second: f32) -> Self {
        Self {
            current: 0,
            ticks_per_second,
            paused: false,
        }
    }

    /// Per-tick time delta for rate-based effects.
    ///
    /// Derived from `ticks_per_second` against a 3600-tick/game-hour baseline:
    /// at the default 60 tps, `dt = 60/3600 = 1/60` (same as the hardcoded
    /// value the FixedUpdate migration used). Tests that want each tick to
    /// represent one full game-second of physics set `ticks_per_second = 3600`
    /// which yields `dt = 1.0` — matching `GameTime`'s "1 tick = 1 game-second"
    /// convention so a single tick realizes a full `*PerSec` effect.
    pub fn dt(&self) -> f32 {
        self.ticks_per_second / 3600.0
    }

    /// Check if this entity should run on this tick (for staggered updates)
    /// Usage: `if !tick.should_run(entity, 10) { continue; }`
    pub fn should_run(&self, entity: Entity, interval: u64) -> bool {
        let entity_id = entity.index_u32() as u64;
        (self.current + entity_id).is_multiple_of(interval.max(1))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// RUN CONDITIONS - Use these with `.run_if()` on systems
// ═══════════════════════════════════════════════════════════════════════════

/// Run condition: Only run when simulation is NOT paused
/// Usage: `.run_if(not_paused)`
pub fn not_paused(tick: Res<TickCount>) -> bool {
    !tick.paused
}

/// Run condition: Only run every N ticks (not staggered by entity)
/// Usage: `.run_if(every_n_ticks(10))`
pub fn every_n_ticks(n: u64) -> impl Fn(Res<TickCount>) -> bool {
    move |tick: Res<TickCount>| tick.current.is_multiple_of(n.max(1))
}

pub fn tick_system(mut tick: ResMut<TickCount>, mut game_time: ResMut<super::GameTime>) {
    if tick.paused {
        return;
    }
    tick.current += 1;
    game_time.update_from_tick(tick.current);
}
