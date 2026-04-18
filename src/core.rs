use bevy::prelude::*;

pub mod diagnostics;
pub mod event_log;
pub mod field_logger;
pub mod log;
pub mod perf;
pub mod sim_rng;
pub mod tick;
pub mod time;

pub use diagnostics::DiagnosticsPlugin;
pub use event_log::{
    EventLogBuffer, EventLogConfig, EventLogFilter, EventLogOutput, collect_event_log,
    dump_event_log, parse_log_filter,
};
pub use field_logger::{
    AgentLogState, AgentSelector, FieldLoggerBuffer, FieldLoggerConfig, FieldLoggerFormat,
    FieldLoggerOutput, FieldSpec, OnChangeSpec, collect_field_log, dump_field_log, expand_fields,
    expand_preset, expand_wildcard, parse_agent_selector, parse_field_spec, parse_on_change_spec,
    print_expanded_field_list,
};
pub use log::GameLog;
pub use perf::{
    BucketStats, PerfBucket, PerfOverlayEnabled, PerfPlugin, PerfSnapshot, PerfSubBucket,
    PerfTracker, SubBucketStats,
};
pub use sim_rng::SimRng;
pub use tick::{TickCount, every_n_ticks, not_paused};
pub use time::GameTime;

pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(60.0))
            .register_type::<TickCount>()
            .register_type::<GameTime>()
            .register_type::<GameLog>()
            .insert_resource(TickCount::new(60.0)) // 60 ticks per second
            .insert_resource(GameLog::new(100))
            .init_resource::<GameTime>()
            .init_resource::<SimRng>()
            .add_systems(FixedUpdate, tick::tick_system)
            .add_systems(Update, time_controls);
    }
}

/// Handle time control keyboard input: Space=pause, +/==faster, -=slower
fn time_controls(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut tick: ResMut<TickCount>,
    mut fixed_time: ResMut<Time<Fixed>>,
    mut game_log: ResMut<GameLog>,
    state: Res<State<crate::menu::AppState>>,
    pause_menu: Res<crate::menu::PauseMenuOpen>,
) {
    if *state.get() != crate::menu::AppState::InSim || pause_menu.0 {
        return;
    }

    if keyboard.just_pressed(KeyCode::Space) {
        tick.paused = !tick.paused;
        game_log.event(&format!(
            "Simulation {}",
            if tick.paused { "PAUSED" } else { "RESUMED" }
        ));
    }

    let speeds = [60.0, 120.0, 180.0, 300.0, 600.0]; // ticks per second
    let current_speed_index = speeds
        .iter()
        .position(|&s| (s - tick.ticks_per_second).abs() < 1.0)
        .unwrap_or(0);

    let mut speed_changed = false;

    if (keyboard.just_pressed(KeyCode::Equal) || keyboard.just_pressed(KeyCode::NumpadAdd))
        && current_speed_index < speeds.len() - 1
    {
        tick.ticks_per_second = speeds[current_speed_index + 1];
        speed_changed = true;
        game_log.event(&format!("Speed: {}x", tick.ticks_per_second / 60.0));
    }

    if (keyboard.just_pressed(KeyCode::Minus) || keyboard.just_pressed(KeyCode::NumpadSubtract))
        && current_speed_index > 0
    {
        tick.ticks_per_second = speeds[current_speed_index - 1];
        speed_changed = true;
        game_log.event(&format!("Speed: {}x", tick.ticks_per_second / 60.0));
    }

    if speed_changed {
        fixed_time.set_timestep_hz(tick.ticks_per_second as f64);
    }
}
