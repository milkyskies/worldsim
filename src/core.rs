use bevy::prelude::*;

pub mod diagnostics;
pub mod log;
pub mod tick;
pub mod time;

pub use log::GameLog;
pub use tick::{TickCount, not_paused};
pub use time::GameTime;
pub use diagnostics::DiagnosticsPlugin;

pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<TickCount>()
            .register_type::<GameTime>()
            .register_type::<GameLog>()
            .insert_resource(TickCount::new(60.0)) // 60 ticks per second
            .insert_resource(GameLog::new(100))
            .init_resource::<GameTime>()
            .add_systems(Update, (time_controls, tick::tick_system).chain());
    }
}

/// Handle time control keyboard input: Space=pause, +/==faster, -=slower
fn time_controls(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut tick: ResMut<TickCount>,
    mut game_log: ResMut<GameLog>,
) {
    // Space toggles pause
    if keyboard.just_pressed(KeyCode::Space) {
        tick.paused = !tick.paused;
        game_log.event(&format!(
            "Simulation {}",
            if tick.paused { "PAUSED" } else { "RESUMED" }
        ));
    }

    // Speed controls (1x, 2x, 3x, 5x, 10x)
    let speeds = [60.0, 120.0, 180.0, 300.0, 600.0]; // ticks per second
    let current_speed_index = speeds
        .iter()
        .position(|&s| (s - tick.ticks_per_second).abs() < 1.0)
        .unwrap_or(0);

    // + or = to speed up
    if (keyboard.just_pressed(KeyCode::Equal) || keyboard.just_pressed(KeyCode::NumpadAdd))
        && current_speed_index < speeds.len() - 1 {
            tick.ticks_per_second = speeds[current_speed_index + 1];
            game_log.event(&format!("Speed: {}x", tick.ticks_per_second / 60.0));
        }

    // - to slow down
    if (keyboard.just_pressed(KeyCode::Minus) || keyboard.just_pressed(KeyCode::NumpadSubtract))
        && current_speed_index > 0 {
            tick.ticks_per_second = speeds[current_speed_index - 1];
            game_log.event(&format!("Speed: {}x", tick.ticks_per_second / 60.0));
        }
}
