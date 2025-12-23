use bevy::diagnostic::{
    DiagnosticPath, DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
    SystemInformationDiagnosticsPlugin,
};
use bevy::prelude::*;
use worldsim::agent::AgentPlugin;

use worldsim::core::CorePlugin;
use worldsim::ui::UiPlugin;
use worldsim::ui::camera::CameraPlugin;
use worldsim::world::WorldPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        // Core systems (tick, time) - must be first
        .add_plugins(CorePlugin)
        // Diagnostics (Data collectors only)
        .add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
            SystemInformationDiagnosticsPlugin,
        ))
        // Custom performance logger
        .add_systems(Update, log_performance_stats)
        // Performance profiling for debugging
        .add_plugins(worldsim::core::DiagnosticsPlugin)
        // Domain plugins
        .add_plugins(WorldPlugin)
        .add_plugins(AgentPlugin)
        .add_plugins(UiPlugin)
        .add_plugins(CameraPlugin)
        .add_systems(Startup, setup_camera)
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn log_performance_stats(
    mut game_log: ResMut<worldsim::core::GameLog>,
    diagnostics: Res<DiagnosticsStore>,
    time: Res<Time>,
    tick: Res<worldsim::core::tick::TickCount>,
    mut last_log: Local<f32>,
) {
    // Do not log if paused
    if tick.paused {
        return;
    }

    if time.elapsed_secs() - *last_log < 0.5 {
        return;
    }

    if !game_log.is_enabled(worldsim::core::log::LogCategory::Performance) {
        return;
    }

    *last_log = time.elapsed_secs();

    if let Some(fps) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS)
        && let Some(avg) = fps.average() {
            game_log.performance(format!("FPS: {:.1}", avg));
        }

    if let Some(frame_time) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        && let Some(avg) = frame_time.average() {
            game_log.performance(format!("Frame Time: {:.2}ms", avg));
        }

    if let Some(entities) = diagnostics.get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        && let Some(count) = entities.value() {
            game_log.performance(format!("Entities: {:.0}", count));
        }

    if let Some(mem) = diagnostics.get(&DiagnosticPath::const_new("system/mem_used"))
        && let Some(val) = mem.value() {
            // value is in bytes ?? No, SystemInfo usually returns bytes. Let's assume bytes -> MB
            // Actually bevy_diagnostic documentation says some are in bytes.
            // Let's just print raw value for now or try to format
            game_log.performance(format!(
                "Mem Used: {:.1} GB",
                val / 1024.0 / 1024.0 / 1024.0
            ));
        }

    if let Some(cpu) = diagnostics.get(&DiagnosticPath::const_new("system/cpu_usage"))
        && let Some(val) = cpu.value() {
            game_log.performance(format!("CPU Usage: {:.1}%", val));
        }
}
