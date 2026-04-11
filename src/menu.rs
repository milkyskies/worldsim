//! Main menu and simulation startup flow.
//!
//! Reads: TickCount (pauses ticks while on a menu screen)
//! Writes: AppState, SimConfig, CreateWorldForm, SimRng (reseeded on entering InSim)
//! Upstream: none
//! Downstream: world::map (`setup_map` reads `SimConfig.seed`), world::spawner (`OnEnter(InSim)` dispatches on `SimConfig.mode`)

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy_egui::{EguiContext, EguiPrimaryContextPass, PrimaryEguiContext, egui};
use rand::Rng;

use crate::core::{SimRng, TickCount};

/// Top-level app screen. Drives whether the simulation plugins are running.
#[derive(States, Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum AppState {
    #[default]
    MainMenu,
    ModePicker,
    CreateWorld,
    InSim,
}

/// Which simulation variant the player chose. Future factory-game systems will
/// `run_if` on `SimMode::Game` to gate themselves to the game-mode build.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub enum SimMode {
    #[default]
    Debug,
    Game,
}

impl SimMode {
    pub fn label(self) -> &'static str {
        match self {
            SimMode::Debug => "Debug",
            SimMode::Game => "Game",
        }
    }
}

/// Marker resource present only in game-mode runs. Lets tests and future
/// factory-game systems detect "we are in game mode" without inspecting
/// `SimConfig` directly.
#[derive(Resource, Debug, Default)]
pub struct GameModeMarker;

/// Configuration for the active simulation. Inserted on entry to `AppState::InSim`
/// and read by `setup_map`, `spawn_initial_population`, and any future system
/// that needs to know which mode is running or what seed to use.
#[derive(Resource, Debug, Clone)]
pub struct SimConfig {
    pub mode: SimMode,
    pub seed: u32,
    pub world_name: String,
}

/// In-progress form state for the menu screens. Persists across state transitions
/// so the player can step Back without losing their input.
#[derive(Resource, Debug, Default)]
pub struct CreateWorldForm {
    pub name: String,
    pub seed_input: String,
    pub selected_mode: SimMode,
}

pub const DEFAULT_WORLD_NAME: &str = "New World";

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .init_resource::<CreateWorldForm>()
            .add_systems(OnEnter(AppState::MainMenu), (reset_form, pause_ticks))
            .add_systems(OnEnter(AppState::ModePicker), pause_ticks)
            .add_systems(OnEnter(AppState::CreateWorld), pause_ticks)
            .add_systems(OnEnter(AppState::InSim), enter_sim)
            .add_systems(
                EguiPrimaryContextPass,
                main_menu_screen.run_if(in_state(AppState::MainMenu)),
            )
            .add_systems(
                EguiPrimaryContextPass,
                mode_picker_screen.run_if(in_state(AppState::ModePicker)),
            )
            .add_systems(
                EguiPrimaryContextPass,
                create_world_screen.run_if(in_state(AppState::CreateWorld)),
            );
    }
}

fn reset_form(mut form: ResMut<CreateWorldForm>) {
    *form = CreateWorldForm::default();
}

fn pause_ticks(tick: Option<ResMut<TickCount>>) {
    if let Some(mut tick) = tick {
        tick.paused = true;
    }
}

fn enter_sim(
    config: Option<Res<SimConfig>>,
    tick: Option<ResMut<TickCount>>,
    mut commands: Commands,
) {
    if let Some(mut tick) = tick {
        tick.paused = false;
    }
    if let Some(config) = config {
        commands.insert_resource(SimRng::from_seed(config.seed as u64));
    }
}

/// Parses a seed string. Empty or unparseable input falls back to a fresh
/// random seed so the player never silently runs with seed 0.
pub fn parse_seed_or_random(input: &str) -> u32 {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return random_seed();
    }
    trimmed.parse::<u32>().unwrap_or_else(|_| random_seed())
}

pub fn random_seed() -> u32 {
    rand::rng().random::<u32>()
}

/// Pure helper: turns the form state into a `SimConfig`, inserts it, and
/// queues a transition to `AppState::InSim`. Extracted from the egui screen so
/// tests can drive it without simulating button clicks.
pub fn start_simulation(
    form: &CreateWorldForm,
    commands: &mut Commands,
    next_state: &mut NextState<AppState>,
) -> SimConfig {
    let seed = parse_seed_or_random(&form.seed_input);
    let world_name = if form.name.trim().is_empty() {
        DEFAULT_WORLD_NAME.to_string()
    } else {
        form.name.clone()
    };
    let config = SimConfig {
        mode: form.selected_mode,
        seed,
        world_name,
    };
    commands.insert_resource(config.clone());
    if matches!(form.selected_mode, SimMode::Game) {
        commands.init_resource::<GameModeMarker>();
    }
    next_state.set(AppState::InSim);
    config
}

fn main_menu_screen(
    mut contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit_writer: MessageWriter<AppExit>,
) {
    let Ok(mut egui_ctx) = contexts.single_mut() else {
        return;
    };
    let ctx = egui_ctx.get_mut();

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(140.0);
            ui.heading(egui::RichText::new("Worldsim").size(56.0));
            ui.add_space(40.0);

            let button_size = egui::vec2(220.0, 48.0);
            if ui
                .add_sized(button_size, egui::Button::new("New Simulation"))
                .clicked()
            {
                next_state.set(AppState::ModePicker);
            }
            ui.add_space(8.0);
            if ui
                .add_sized(button_size, egui::Button::new("Quit"))
                .clicked()
            {
                exit_writer.write(AppExit::Success);
            }
        });
    });
}

fn mode_picker_screen(
    mut contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>,
    mut next_state: ResMut<NextState<AppState>>,
    mut form: ResMut<CreateWorldForm>,
) {
    let Ok(mut egui_ctx) = contexts.single_mut() else {
        return;
    };
    let ctx = egui_ctx.get_mut();

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(120.0);
            ui.heading("Choose a mode");
            ui.add_space(8.0);
            ui.label("Debug is the sandbox sim. Game is the factory-game scaffold.");
            ui.add_space(40.0);

            let button_size = egui::vec2(220.0, 48.0);
            if ui
                .add_sized(button_size, egui::Button::new("Debug"))
                .clicked()
            {
                form.selected_mode = SimMode::Debug;
                next_state.set(AppState::CreateWorld);
            }
            ui.add_space(8.0);
            if ui
                .add_sized(button_size, egui::Button::new("Game"))
                .clicked()
            {
                form.selected_mode = SimMode::Game;
                next_state.set(AppState::CreateWorld);
            }
            ui.add_space(20.0);
            if ui
                .add_sized(egui::vec2(140.0, 32.0), egui::Button::new("Back"))
                .clicked()
            {
                next_state.set(AppState::MainMenu);
            }
        });
    });
}

fn create_world_screen(
    mut contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>,
    mut next_state: ResMut<NextState<AppState>>,
    mut form: ResMut<CreateWorldForm>,
    mut commands: Commands,
) {
    let Ok(mut egui_ctx) = contexts.single_mut() else {
        return;
    };
    let ctx = egui_ctx.get_mut();

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);
            ui.heading("Create New World");
            ui.add_space(4.0);
            ui.label(format!("Mode: {}", form.selected_mode.label()));
            ui.add_space(28.0);

            egui::Grid::new("create_world_form")
                .num_columns(2)
                .spacing([16.0, 12.0])
                .show(ui, |ui| {
                    ui.label("World name");
                    ui.add(
                        egui::TextEdit::singleline(&mut form.name)
                            .desired_width(220.0)
                            .hint_text(DEFAULT_WORLD_NAME),
                    );
                    ui.end_row();

                    ui.label("Seed");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut form.seed_input)
                                .desired_width(160.0)
                                .hint_text("random"),
                        );
                        if ui.button("Randomize").clicked() {
                            form.seed_input = random_seed().to_string();
                        }
                    });
                    ui.end_row();
                });

            ui.add_space(32.0);

            ui.horizontal(|ui| {
                if ui
                    .add_sized(egui::vec2(140.0, 40.0), egui::Button::new("Back"))
                    .clicked()
                {
                    next_state.set(AppState::ModePicker);
                }
                ui.add_space(20.0);
                if ui
                    .add_sized(egui::vec2(140.0, 40.0), egui::Button::new("Play"))
                    .clicked()
                {
                    start_simulation(&form, &mut commands, &mut next_state);
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_seed_returns_explicit_value() {
        assert_eq!(parse_seed_or_random("1337"), 1337);
        assert_eq!(parse_seed_or_random("  42  "), 42);
    }

    #[test]
    fn parse_seed_empty_input_returns_some_value() {
        // Returning a random u32 — just verify it doesn't panic and returns
        // a valid value. The randomness itself isn't testable.
        let _ = parse_seed_or_random("");
        let _ = parse_seed_or_random("   ");
    }

    #[test]
    fn parse_seed_invalid_input_falls_through_to_random() {
        // Should not panic. Any u32 result is acceptable.
        let _ = parse_seed_or_random("not a number");
        let _ = parse_seed_or_random("3.14");
    }

    fn drive_start_simulation(form: CreateWorldForm) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<AppState>();
        app.init_resource::<CreateWorldForm>();
        let sys_id = app.register_system(
            move |mut commands: Commands, mut next: ResMut<NextState<AppState>>| {
                start_simulation(&form, &mut commands, &mut next);
            },
        );
        app.world_mut().run_system(sys_id).unwrap();
        // Two updates: first commits the inserted resource and applies the
        // queued state transition; second lets transition observers settle.
        app.update();
        app.update();
        app
    }

    #[test]
    fn start_simulation_with_explicit_seed_writes_matching_config() {
        let app = drive_start_simulation(CreateWorldForm {
            name: "My World".into(),
            seed_input: "42".into(),
            selected_mode: SimMode::Debug,
        });

        let config = app.world().resource::<SimConfig>();
        assert_eq!(config.seed, 42);
        assert_eq!(config.mode, SimMode::Debug);
        assert_eq!(config.world_name, "My World");

        let state = app.world().resource::<State<AppState>>();
        assert_eq!(*state.get(), AppState::InSim);
        assert!(app.world().get_resource::<GameModeMarker>().is_none());
    }

    #[test]
    fn start_simulation_game_mode_inserts_marker_resource() {
        let app = drive_start_simulation(CreateWorldForm {
            name: "Factory Town".into(),
            seed_input: "9".into(),
            selected_mode: SimMode::Game,
        });

        let config = app.world().resource::<SimConfig>();
        assert_eq!(config.mode, SimMode::Game);
        assert!(app.world().get_resource::<GameModeMarker>().is_some());
    }

    #[test]
    fn start_simulation_blank_name_uses_default() {
        let app = drive_start_simulation(CreateWorldForm {
            name: "   ".into(),
            seed_input: "7".into(),
            selected_mode: SimMode::Debug,
        });
        let config = app.world().resource::<SimConfig>();
        assert_eq!(config.world_name, DEFAULT_WORLD_NAME);
    }

    #[test]
    fn start_simulation_blank_seed_is_replaced_with_a_value() {
        // Empty seed input must produce *some* SimConfig with a seed — the
        // important property is that the player never silently runs with a
        // missing seed value.
        let app = drive_start_simulation(CreateWorldForm {
            name: "n".into(),
            seed_input: "".into(),
            selected_mode: SimMode::Debug,
        });
        // The seed is whatever the RNG produced; we just need a SimConfig.
        let _ = app.world().resource::<SimConfig>().seed;
    }
}
