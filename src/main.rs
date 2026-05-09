// See lib.rs for the rationale - Bevy systems trip these lints by design.
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

use bevy::diagnostic::{
    DiagnosticPath, DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
    SystemInformationDiagnosticsPlugin,
};
use bevy::prelude::*;
use clap::Parser;
use worldsim::agent::AgentPlugin;
use worldsim::cli::CliArgs;
use worldsim::core::CorePlugin;
use worldsim::eyes::EyesPlugin;
use worldsim::headless;
use worldsim::injuries::InjuriesPlugin;
use worldsim::menu::MenuPlugin;
use worldsim::palette::PalettePlugin;
use worldsim::silhouette::SilhouettePlugin;
use worldsim::ui::UiPlugin;
use worldsim::ui::camera::CameraPlugin;
use worldsim::world::WorldPlugin;

fn main() {
    // Bevy caches each system's `info_span!("system", name = ...)` at
    // SystemMeta::new(), and `info_span!` checks the global tracing dispatcher
    // at construction. Without a subscriber installed first, every system span
    // is permanently disabled and never reaches Tracy.
    #[cfg(feature = "profile-tracy")]
    {
        use tracing_subscriber::layer::SubscriberExt;
        let subscriber = tracing_subscriber::registry().with(tracing_tracy::TracyLayer::default());
        tracing::subscriber::set_global_default(subscriber)
            .expect("failed to install global tracing-tracy subscriber");
    }

    let args = CliArgs::parse();

    if args.dump_map {
        print_terrain_matrix();
        return;
    }

    if args.log_list_fields {
        match worldsim::core::expand_fields(&args.log_field, &args.log_preset) {
            Ok(fields) => worldsim::core::print_expanded_field_list(&fields),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }

    if let Some(run_dir) = &args.debug_dir {
        match worldsim::core::event_log::generate_duckdb_setup_script(run_dir) {
            Ok(script) => println!("{script}"),
            Err(e) => {
                eprintln!("--debug failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.headless {
        let report = headless::run_headless(args.to_headless_config());
        if args.report {
            let json = serde_json::to_string_pretty(&report)
                .expect("HeadlessReport serialization should never fail");
            println!("{json}");
        } else {
            println!(
                "Headless run complete: {} ticks in {:.3}s ({:.0} ticks/s)",
                report.ticks, report.elapsed_secs, report.ticks_per_second
            );
        }
        return;
    }

    run_windowed();
}

/// Generates the default terrain and prints it to stdout as ASCII art.
///
/// Legend: `.` grass, `f` forest, `#` rock, `s` sand, `~` shallow water, `W` deep water.
fn print_terrain_matrix() {
    use worldsim::world::map::{
        DEFAULT_TERRAIN_SEED, TileType, WORLD_HEIGHT, WORLD_WIDTH, generate_terrain,
    };

    let tiles = generate_terrain(WORLD_WIDTH, WORLD_HEIGHT, DEFAULT_TERRAIN_SEED);
    // Print with y inverted so north is at the top (matches the rendered view).
    for y in (0..WORLD_HEIGHT).rev() {
        let row: String = (0..WORLD_WIDTH)
            .map(|x| match tiles[(y * WORLD_WIDTH + x) as usize] {
                TileType::Grass => '.',
                TileType::Dirt => 'd',
                TileType::Gravel => ',',
                TileType::Rock => '#',
                TileType::Sand => 's',
                TileType::ShallowWater => '~',
                TileType::Water => 'W',
            })
            .collect();
        println!("{row}");
    }
}

/// Builds and runs the full Bevy app with rendering and UI.
fn run_windowed() {
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
        // Per-system tick timer feeding the F3 overlay.
        .add_plugins(worldsim::core::PerfPlugin)
        // Menu first so AppState is registered before any other plugin
        // schedules systems on OnEnter(AppState::InSim).
        .add_plugins(MenuPlugin)
        // Palette resource - loaded before any spawn system reads colors.
        .add_plugins(PalettePlugin)
        // Reads CreatureSilhouette and renders the child sprite hierarchy.
        .add_plugins(SilhouettePlugin)
        // Emotion-driven eye state (Open/Wide/Squint/Closed) per silhouette eye.
        .add_plugins(EyesPlugin)
        // Body injury overlays - tints sprite parts toward blood reds.
        .add_plugins(InjuriesPlugin)
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
        && let Some(avg) = fps.average()
    {
        game_log.performance(format!("FPS: {:.1}", avg));
    }

    if let Some(frame_time) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        && let Some(avg) = frame_time.average()
    {
        game_log.performance(format!("Frame Time: {:.2}ms", avg));
    }

    if let Some(entities) = diagnostics.get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        && let Some(count) = entities.value()
    {
        game_log.performance(format!("Entities: {:.0}", count));
    }

    if let Some(mem) = diagnostics.get(&DiagnosticPath::const_new("system/mem_used"))
        && let Some(val) = mem.value()
    {
        // value is in bytes ?? No, SystemInfo usually returns bytes. Let's assume bytes -> MB
        // Actually bevy_diagnostic documentation says some are in bytes.
        // Let's just print raw value for now or try to format
        game_log.performance(format!(
            "Mem Used: {:.1} GB",
            val / 1024.0 / 1024.0 / 1024.0
        ));
    }

    if let Some(cpu) = diagnostics.get(&DiagnosticPath::const_new("system/cpu_usage"))
        && let Some(val) = cpu.value()
    {
        game_log.performance(format!("CPU Usage: {:.1}%", val));
    }
}
