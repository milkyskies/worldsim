use bevy::prelude::*;

/// Tracks the simulation tick count
#[derive(Resource, Reflect)]
#[reflect(Resource)]
pub struct TickCount {
    /// Current tick number, denominated in game-seconds (1 tick = 1 game-second
    /// per `GameTime::TICKS_PER_SECOND`). Incremented by `game_seconds_per_cycle`
    /// per FixedMain cycle — which is 1 by default (windowed game) and can be
    /// set larger by test harnesses to compress many game-seconds into one
    /// FixedMain cycle and cut wall-clock time proportionally.
    pub current: u64,
    /// Wall-clock speed in FixedUpdate cycles per second. Read by `time_controls`
    /// and the UI speed buttons; written to `Time<Fixed>::set_timestep_hz`
    /// to control how many FixedUpdate cycles Bevy runs per frame.
    pub ticks_per_second: f32,
    /// How many game-seconds elapse per FixedMain cycle. 1 (default) means one
    /// cycle simulates one game-second. Test harnesses set this to 60 to run
    /// 60 game-seconds of physics per cycle — same total effect over the same
    /// `current` span, 60× fewer cycles of work. Must be ≥ 1.
    pub game_seconds_per_cycle: u64,
    /// Whether simulation is paused
    pub paused: bool,
}

impl Default for TickCount {
    fn default() -> Self {
        Self {
            current: 0,
            ticks_per_second: 60.0,
            game_seconds_per_cycle: 1,
            paused: false,
        }
    }
}

impl TickCount {
    pub fn new(ticks_per_second: f32) -> Self {
        Self {
            ticks_per_second,
            ..Self::default()
        }
    }

    /// Sets how many game-seconds elapse per FixedMain cycle. See field docs.
    pub fn with_game_seconds_per_cycle(mut self, gspc: u64) -> Self {
        self.game_seconds_per_cycle = gspc.max(1);
        self
    }

    /// Per-tick game-time delta for rate-based effects.
    ///
    /// At windowed defaults (60 tps, gspc=1): `dt = 1/60` — each FixedMain
    /// cycle advances physics by 1/60 wall-second. At test fast-mode defaults
    /// (60 tps, gspc=60): `dt = 1.0` — each cycle advances 60× more physics,
    /// matching the 60× coarser tick step so per-game-second behavior is
    /// preserved.
    pub fn dt(&self) -> f32 {
        (self.ticks_per_second / 3600.0) * self.game_seconds_per_cycle as f32
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
    let step = tick.game_seconds_per_cycle;
    tick.current += step;
    game_time.update_from_tick(tick.current);
}
