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

use crate::agent::mind::conversation::ConversationManager;
use crate::core::{SimRng, TickCount};
use crate::world::spatial_index::SpatialIndex;

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

/// Whether the in-sim pause menu overlay is currently shown. Toggled by ESC.
#[derive(Resource, Debug, Default)]
pub struct PauseMenuOpen(pub bool);

/// Set by menu buttons that want to terminate the app. An Update-schedule
/// system drains this into an actual `AppExit` message so the write happens
/// in a schedule the runner will actually see before it checks at frame end
/// — writing `AppExit` directly from `EguiPrimaryContextPass` was landing
/// too late for the runner to pick up.
#[derive(Resource, Debug, Default)]
pub struct QuitRequested(pub bool);

pub const DEFAULT_WORLD_NAME: &str = "New World";

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .init_resource::<CreateWorldForm>()
            .init_resource::<PauseMenuOpen>()
            .init_resource::<QuitRequested>()
            .add_systems(OnEnter(AppState::MainMenu), (reset_form, pause_ticks))
            .add_systems(OnEnter(AppState::ModePicker), pause_ticks)
            .add_systems(OnEnter(AppState::CreateWorld), pause_ticks)
            .add_systems(OnEnter(AppState::InSim), enter_sim)
            .add_systems(
                OnExit(AppState::InSim),
                (close_pause_menu, reset_sim_resources),
            )
            .add_systems(First, enforce_pause_outside_in_sim)
            .add_systems(Update, handle_pause_key.run_if(in_state(AppState::InSim)))
            .add_systems(Update, drain_quit_requested)
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
            )
            .add_systems(
                EguiPrimaryContextPass,
                pause_menu_screen
                    .run_if(in_state(AppState::InSim))
                    .run_if(pause_menu_is_open),
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

fn close_pause_menu(mut pause: ResMut<PauseMenuOpen>, tick: Option<ResMut<TickCount>>) {
    pause.0 = false;
    // Bringing the player back to the menu also pauses ticks; the next entry
    // into InSim will unpause via `enter_sim`.
    if let Some(mut tick) = tick {
        tick.paused = true;
    }
}

/// Drops entity references held by stateful sim resources so the next run
/// doesn't access stale generations from the previous sim. Without this, a
/// second "New Simulation" crashes because systems like
/// `evaluate_conversation_continuation` try to apply commands against
/// despawned agents whose IDs were still in `ConversationManager`.
fn reset_sim_resources(
    mut conversations: Option<ResMut<ConversationManager>>,
    mut spatial: Option<ResMut<SpatialIndex>>,
) {
    if let Some(conversations) = conversations.as_mut() {
        **conversations = ConversationManager::default();
    }
    if let Some(spatial) = spatial.as_mut() {
        **spatial = SpatialIndex::default();
    }
}

/// Belt-and-suspenders: if we're not in the sim, force the tick paused so
/// any `run_if(not_paused)` system skips even if a transition left the flag
/// in a stale state. Runs in `First` so Update-time systems see the enforced
/// value before they check it.
fn enforce_pause_outside_in_sim(state: Res<State<AppState>>, tick: Option<ResMut<TickCount>>) {
    if *state.get() == AppState::InSim {
        return;
    }
    if let Some(mut tick) = tick {
        tick.paused = true;
    }
}

fn drain_quit_requested(mut quit: ResMut<QuitRequested>, mut exit_writer: MessageWriter<AppExit>) {
    if quit.0 {
        quit.0 = false;
        exit_writer.write(AppExit::Success);
    }
}

fn pause_menu_is_open(pause: Res<PauseMenuOpen>) -> bool {
    pause.0
}

/// Run condition for sim-interaction UI systems: true only while the player
/// is actively in a sim AND not looking at the pause menu overlay. Prevents
/// clicking on the world or toggling debug widgets from leaking through the
/// pause menu.
pub fn sim_interactive(state: Res<State<AppState>>, pause: Res<PauseMenuOpen>) -> bool {
    *state.get() == AppState::InSim && !pause.0
}

fn handle_pause_key(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut pause: ResMut<PauseMenuOpen>,
    tick: Option<ResMut<TickCount>>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }
    pause.0 = !pause.0;
    if let Some(mut tick) = tick {
        // Pause menu open ⇒ force pause; closing returns control to play.
        tick.paused = pause.0;
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
    mut quit: ResMut<QuitRequested>,
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
                quit.0 = true;
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

/// Width of the create-world card. The form is horizontally padded to this
/// width and centered, so widgets like `Grid` (which expand to fill their
/// container) stay aligned instead of drifting to the right edge.
const CREATE_WORLD_CARD_WIDTH: f32 = 440.0;

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
        ui.add_space(80.0);

        let avail_width = ui.available_width();
        let side_pad = ((avail_width - CREATE_WORLD_CARD_WIDTH) / 2.0).max(0.0);

        ui.horizontal(|ui| {
            ui.add_space(side_pad);
            ui.vertical(|ui| {
                ui.set_width(CREATE_WORLD_CARD_WIDTH);

                ui.vertical_centered(|ui| {
                    ui.heading("Create New World");
                    ui.add_space(4.0);
                    ui.label(format!("Mode: {}", form.selected_mode.label()));
                });
                ui.add_space(28.0);

                egui::Grid::new("create_world_form")
                    .num_columns(2)
                    .spacing([16.0, 12.0])
                    .min_col_width(110.0)
                    .show(ui, |ui| {
                        ui.label("World name");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.name)
                                .desired_width(260.0)
                                .hint_text(DEFAULT_WORLD_NAME),
                        );
                        ui.end_row();

                        ui.label("Seed");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut form.seed_input)
                                    .desired_width(170.0)
                                    .hint_text("random"),
                            );
                            if ui.button("Randomize").clicked() {
                                form.seed_input = random_seed().to_string();
                            }
                        });
                        ui.end_row();
                    });

                ui.add_space(36.0);

                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        let button_size = egui::vec2(140.0, 40.0);
                        let row_width = button_size.x * 2.0 + 20.0;
                        let inner_pad = ((CREATE_WORLD_CARD_WIDTH - row_width) / 2.0).max(0.0);
                        ui.add_space(inner_pad);
                        if ui
                            .add_sized(button_size, egui::Button::new("Back"))
                            .clicked()
                        {
                            next_state.set(AppState::ModePicker);
                        }
                        ui.add_space(20.0);
                        if ui
                            .add_sized(button_size, egui::Button::new("Play"))
                            .clicked()
                        {
                            start_simulation(&form, &mut commands, &mut next_state);
                        }
                    });
                });
            });
        });
    });
}

fn pause_menu_screen(
    mut contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>,
    mut next_state: ResMut<NextState<AppState>>,
    mut pause: ResMut<PauseMenuOpen>,
    tick: Option<ResMut<TickCount>>,
    mut quit: ResMut<QuitRequested>,
) {
    let Ok(mut egui_ctx) = contexts.single_mut() else {
        return;
    };
    let ctx = egui_ctx.get_mut();

    egui::Window::new("Paused")
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .fixed_size(egui::vec2(280.0, 220.0))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(16.0);
                ui.heading("Paused");
                ui.add_space(20.0);

                let button_size = egui::vec2(200.0, 40.0);

                if ui
                    .add_sized(button_size, egui::Button::new("Resume"))
                    .clicked()
                {
                    pause.0 = false;
                    if let Some(mut tick) = tick {
                        tick.paused = false;
                    }
                    return;
                }
                ui.add_space(8.0);
                if ui
                    .add_sized(button_size, egui::Button::new("Main Menu"))
                    .clicked()
                {
                    // OnExit(InSim) closes the pause menu and DespawnOnExit
                    // tears down the sim entities.
                    next_state.set(AppState::MainMenu);
                    return;
                }
                ui.add_space(8.0);
                if ui
                    .add_sized(button_size, egui::Button::new("Quit"))
                    .clicked()
                {
                    quit.0 = true;
                }
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

    #[test]
    fn close_pause_menu_resets_resource_and_pauses_tick() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<AppState>();
        app.insert_resource(PauseMenuOpen(true));
        app.insert_resource(TickCount::new(60.0));
        {
            let mut tick = app.world_mut().resource_mut::<TickCount>();
            tick.paused = false;
        }

        let sys_id = app.register_system(close_pause_menu);
        app.world_mut().run_system(sys_id).unwrap();

        assert!(!app.world().resource::<PauseMenuOpen>().0);
        assert!(app.world().resource::<TickCount>().paused);
    }

    #[test]
    fn drain_quit_requested_writes_app_exit_and_clears_flag() {
        use bevy::app::AppExit;
        use bevy::ecs::message::Messages;

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.add_message::<AppExit>();
        app.init_resource::<QuitRequested>();
        app.world_mut().resource_mut::<QuitRequested>().0 = true;

        let sys_id = app.register_system(drain_quit_requested);
        app.world_mut().run_system(sys_id).unwrap();

        assert!(
            !app.world().resource::<QuitRequested>().0,
            "flag should be cleared after drain"
        );
        let messages = app.world().resource::<Messages<AppExit>>();
        assert!(
            messages.len() >= 1,
            "drain_quit_requested should have written at least one AppExit message"
        );
    }

    #[test]
    fn reset_sim_resources_clears_stale_conversation_entities() {
        let mut app = App::new();
        app.init_resource::<ConversationManager>();
        app.init_resource::<SpatialIndex>();

        // Seed the conversation manager with a fake participant — after
        // DespawnOnExit runs, its Entity id would be stale.
        let fake_entity = app.world_mut().spawn_empty().id();
        {
            let mut conv = app.world_mut().resource_mut::<ConversationManager>();
            conv.start_conversation(vec![fake_entity], 0);
        }
        {
            let mut spatial = app.world_mut().resource_mut::<SpatialIndex>();
            spatial.update_entity(fake_entity, IVec2::new(0, 0));
        }

        let sys_id = app.register_system(reset_sim_resources);
        app.world_mut().run_system(sys_id).unwrap();

        assert!(
            app.world()
                .resource::<ConversationManager>()
                .conversations
                .is_empty(),
            "ConversationManager should be empty after reset"
        );
    }
}
