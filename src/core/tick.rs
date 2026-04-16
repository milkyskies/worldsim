use bevy::prelude::*;

/// Tracks the simulation tick count
#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
pub struct TickCount {
    /// Current tick number (0, 1, 2, ...)
    pub current: u64,
    /// Ticks per second (configurable rate)
    pub ticks_per_second: f32,
    /// Accumulated time since last tick
    accumulated: f32,
    /// Whether simulation is paused
    pub paused: bool,
}

impl TickCount {
    pub fn new(ticks_per_second: f32) -> Self {
        Self {
            current: 0,
            ticks_per_second,
            accumulated: 0.0,
            paused: false,
        }
    }

    /// Per-tick time delta for rate-based effects.
    ///
    /// Fixed at 1/60 game-second per tick. Speed control is handled by
    /// running more FixedUpdate cycles per wall-clock second (via
    /// `Time<Fixed>::set_timestep_hz`), not by scaling dt.
    pub fn dt(&self) -> f32 {
        1.0 / 60.0
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
