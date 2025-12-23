pub mod camera;
pub mod hud;
pub mod overlays;
// pub mod inspector; // Merged for now to simplify migration

use bevy::camera::Viewport;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{
    EguiContext, EguiContextSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui,
};
use bevy_inspector_egui::DefaultInspectorConfigPlugin;
use bevy_inspector_egui::bevy_inspector;
use bevy_inspector_egui::bevy_inspector::hierarchy::{SelectedEntities, hierarchy_ui};
use bevy_inspector_egui::bevy_inspector::{
    ui_for_entities_shared_components, ui_for_entity_with_children,
};
use egui::Color32;
use egui_dock::{DockArea, DockState, NodeIndex, Style};
use hud::GameLog;

pub mod debug_knowledge;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .add_plugins(DefaultInspectorConfigPlugin)
            .add_plugins(overlays::OverlayPlugin)
            .init_resource::<UiState>()
            .init_resource::<debug_knowledge::KnowledgeInspectorState>()
            .add_systems(EguiPrimaryContextPass, ui_system)
            .add_systems(PostUpdate, set_camera_viewport.after(ui_system))
            .add_systems(Update, (handle_game_click, draw_selection_gizmos));
    }
}

#[derive(Resource)]
pub struct UiState {
    dock_state: DockState<Tab>,
    pub selected_entities: SelectedEntities,
    pub viewport_rect: egui::Rect,
    /// Time control commands - applied by apply_time_controls system
    pub toggle_pause: bool,
    pub set_speed: Option<f32>,
}

use overlays::OverlayState;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Tab {
    GameView,
    Hierarchy,
    Inspector,
    Log,
    Resources,
    Time,
    Settings,
    AgentViewer,
    MindInspector,
    Social,
}

impl Default for UiState {
    fn default() -> Self {
        let mut dock_state = DockState::new(vec![Tab::GameView]);
        let tree = dock_state.main_surface_mut();

        // Layout: Game center, Hierarchy left 20%, Inspector/AgentViewer right 25%
        // Then Inspector split: Inspector top 70%, Log/Resources bottom 30% (tabbed)
        // Time panel above game
        let [game, _inspector] = tree.split_right(
            NodeIndex::root(),
            0.75,
            vec![
                Tab::AgentViewer,
                Tab::Inspector,
                Tab::MindInspector,
                Tab::Social,
            ],
        );
        let [_hierarchy, game] = tree.split_left(game, 0.2, vec![Tab::Hierarchy]);
        // let [_inspector, _bottom_tabs] =
        //    tree.split_below(_inspector, 0.7, vec![Tab::Log, Tab::Resources]);
        let [_time, _game] = tree.split_above(game, 0.08, vec![Tab::Time]);

        // Settings below Hierarchy
        tree.split_below(
            _hierarchy,
            0.60,
            vec![Tab::Settings, Tab::Log, Tab::Resources],
        );

        Self {
            dock_state,
            selected_entities: SelectedEntities::default(),
            viewport_rect: egui::Rect::NOTHING,
            toggle_pause: false,
            set_speed: None,
        }
    }
}

fn ui_system(world: &mut World) {
    let Ok(egui_context) = world
        .query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>()
        .single(world)
    else {
        return;
    };
    let mut egui_context = egui_context.clone();

    // Use resource_scope pattern from example
    world.resource_scope::<UiState, _>(|world, mut ui_state| {
        ui_state.ui(world, egui_context.get_mut())
    });
}

impl UiState {
    fn ui(&mut self, world: &mut World, ctx: &mut egui::Context) {
        DockArea::new(&mut self.dock_state)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show(
                ctx,
                &mut UiViewer {
                    world,
                    selected_entities: &mut self.selected_entities,
                    viewport_rect: &mut self.viewport_rect,
                },
            );
    }
}

fn set_camera_viewport(
    ui_state: Res<UiState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, Without<PrimaryEguiContext>>,
    egui_settings_query: Query<&EguiContextSettings>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(egui_settings) = egui_settings_query.single() else {
        return;
    };

    for mut camera in cameras.iter_mut() {
        let scale_factor = window.scale_factor() * egui_settings.scale_factor;

        let viewport_pos = ui_state.viewport_rect.left_top().to_vec2() * scale_factor;
        let viewport_size = ui_state.viewport_rect.size() * scale_factor;

        let physical_position = UVec2::new(viewport_pos.x as u32, viewport_pos.y as u32);
        let physical_size = UVec2::new(viewport_size.x as u32, viewport_size.y as u32);

        let rect = physical_position + physical_size;
        let window_size = window.physical_size();

        if rect.x <= window_size.x
            && rect.y <= window_size.y
            && physical_size.x > 0
            && physical_size.y > 0
        {
            camera.viewport = Some(Viewport {
                physical_position,
                physical_size,
                depth: 0.0..1.0,
            });
        }
    }
}

// Handle clicking in game view to select entities
fn handle_game_click(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut ui_state: ResMut<UiState>,
    entities: Query<(Entity, &Transform, Option<&Sprite>)>,
) {
    if buttons.just_pressed(MouseButton::Left) {
        let Ok(window) = windows.single() else { return };
        let Some(cursor_position) = window.cursor_position() else {
            return;
        };

        // Check if click is inside game viewport
        let viewport = ui_state.viewport_rect;
        let cursor_egui = egui::pos2(cursor_position.x, cursor_position.y);

        if !viewport.contains(cursor_egui) {
            return;
        }

        let Some((camera, camera_transform)) = cameras.iter().next() else {
            return;
        };

        // Convert screen to world coords
        let Ok(world_position) = camera.viewport_to_world_2d(camera_transform, cursor_position)
        else {
            return;
        };

        // Find closest entity within pick radius
        let pick_radius = 16.0;
        let mut candidates: Vec<(Entity, f32, f32)> = Vec::new(); // (entity, z, distance)

        for (entity, transform, sprite) in entities.iter() {
            let entity_pos = transform.translation.truncate();
            let dist = entity_pos.distance(world_position);

            // Use sprite size if available, otherwise default radius
            let entity_radius = sprite
                .and_then(|s| s.custom_size)
                .map(|size| size.x.max(size.y) / 2.0)
                .unwrap_or(8.0);

            if dist < entity_radius + pick_radius {
                candidates.push((entity, transform.translation.z, dist));
            }
        }

        // Sort by z (highest first), then by distance (closest first)
        candidates.sort_by(|a, b| {
            b.1.partial_cmp(&a.1) // Higher z first
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal)) // Closer first
        });

        if let Some((entity, _z, _dist)) = candidates.first() {
            let add = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ShiftLeft]);
            ui_state.selected_entities.select_maybe_add(*entity, add);
        }
    }
}

fn draw_selection_gizmos(
    mut gizmos: Gizmos,
    ui_state: Res<UiState>,
    entities: Query<(&GlobalTransform, Option<&Sprite>)>,
) {
    for &entity in ui_state.selected_entities.as_slice() {
        if let Ok((transform, sprite)) = entities.get(entity) {
            let position = transform.translation().truncate();

            // Determine radius based on sprite size or default
            let radius = sprite
                .and_then(|s| s.custom_size)
                .map(|size| size.x.max(size.y) * 0.7) // Roughly fit circle to bounding box
                .unwrap_or(16.0);

            // Draw static selection circle
            gizmos.circle_2d(position, radius, Color::WHITE);
        }
    }
}

struct UiViewer<'a> {
    world: &'a mut World,
    selected_entities: &'a mut SelectedEntities,
    viewport_rect: &'a mut egui::Rect,
}

// ... (ui_system same) ...

impl<'a> egui_dock::TabViewer for UiViewer<'a> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        format!("{:?}", tab).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::GameView => {
                *self.viewport_rect = ui.clip_rect();
            }
            Tab::Hierarchy => {
                let _selected = hierarchy_ui(self.world, ui, self.selected_entities);
            }
            Tab::Inspector => match self.selected_entities.as_slice() {
                &[] => {
                    ui.label("No entity selected.");
                }
                &[entity] => {
                    ui_for_entity_with_children(self.world, entity, ui);
                }
                entities => {
                    ui.label("Multiple entities selected.");
                    ui_for_entities_shared_components(self.world, entities, ui);
                }
            },
            Tab::Log => {
                // ... (Log implementation same)
                if let Some(mut game_log) = self.world.get_resource_mut::<GameLog>() {
                    // Category filter row
                    ui.horizontal_wrapped(|ui| {
                        let mut categories: Vec<_> =
                            crate::core::log::LogCategory::all().into_iter().collect();
                        // Sort by debug name for stable UI
                        categories.sort_by_key(|c| format!("{:?}", c));

                        for category in categories {
                            let mut enabled = game_log.is_enabled(category);
                            if ui
                                .checkbox(&mut enabled, format!("{:?}", category))
                                .changed()
                            {
                                game_log.toggle(category);
                            }
                        }
                    });

                    // Entity filter row
                    ui.horizontal(|ui| {
                        let has_filter = game_log.has_entity_filter();

                        if has_filter {
                            ui.colored_label(Color32::YELLOW, "🔍 Filtering by entity");
                            if ui.button("Clear Filter").clicked() {
                                game_log.clear_entity_filter();
                            }
                        } else if !self.selected_entities.as_slice().is_empty()
                            && ui.button("🔍 Filter by Selected").clicked()
                        {
                            for &entity in self.selected_entities.as_slice() {
                                game_log.add_entity_to_filter(entity);
                            }
                        }
                    });

                    ui.separator();

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for entry in game_log.visible_entries() {
                                ui.label(format!(
                                    "[{}] {} {} {}",
                                    entry.timestamp,
                                    entry.category.prefix(),
                                    entry.message,
                                    if entry.count > 1 {
                                        format!("(x{})", entry.count)
                                    } else {
                                        "".to_string()
                                    }
                                ));
                            }
                        });
                } else {
                    ui.label("No log available");
                }
            }
            Tab::Resources => {
                bevy_inspector::ui_for_resources(self.world, ui);
            }
            Tab::Settings => {
                ui.heading("Overlays");
                if let Some(mut overlay_state) = self.world.get_resource_mut::<OverlayState>() {
                    ui.checkbox(&mut overlay_state.show_vision, "Show Vision Range");
                    ui.checkbox(&mut overlay_state.show_intent, "Show Agent Intent");
                } else {
                    ui.label("OverlayState not found.");
                }
            }
            Tab::Time => {
                if let Some(game_time) = self.world.get_resource::<crate::core::GameTime>() {
                    ui.heading(game_time.format());
                }

                // Get current values first
                let (paused, speed, tick) =
                    if let Some(tick_res) = self.world.get_resource::<crate::core::TickCount>() {
                        (
                            tick_res.paused,
                            tick_res.ticks_per_second / 60.0,
                            tick_res.current,
                        )
                    } else {
                        return;
                    };

                ui.label(format!("Tick: {}", tick));
                ui.separator();

                // Pause/Resume button
                ui.horizontal(|ui| {
                    let pause_text = if paused { "▶ Resume" } else { "⏸ Pause" };
                    if ui.button(pause_text).clicked()
                        && let Some(mut tick_res) =
                            self.world.get_resource_mut::<crate::core::TickCount>()
                    {
                        tick_res.paused = !tick_res.paused;
                    }
                    if paused {
                        ui.colored_label(egui::Color32::RED, "PAUSED");
                    }
                });

                ui.separator();
                ui.label(format!("Speed: {}x", speed));

                // Speed preset buttons
                ui.horizontal(|ui| {
                    for (label, rate) in [
                        ("1x", 60.0),
                        ("2x", 120.0),
                        ("3x", 180.0),
                        ("5x", 300.0),
                        ("10x", 600.0),
                    ] {
                        let selected = (speed - rate / 60.0).abs() < 0.1;
                        let btn = egui::Button::new(label);
                        let btn = if selected {
                            btn.fill(egui::Color32::from_rgb(60, 100, 60))
                        } else {
                            btn
                        };
                        if ui.add(btn).clicked()
                            && let Some(mut tick_res) =
                                self.world.get_resource_mut::<crate::core::TickCount>()
                        {
                            tick_res.ticks_per_second = rate;
                        }
                    }
                });
            }
            Tab::AgentViewer => match *self.selected_entities.as_slice() {
                [] => {
                    ui.label("Select an agent to view details.");
                }
                [entity] => {
                    agent_viewer_ui_for_agent(self.world, entity, ui);
                }
                _ => {
                    ui.label("Select a single agent.");
                }
            },
            Tab::MindInspector => {
                self.world
                    .resource_scope::<debug_knowledge::KnowledgeInspectorState, _>(
                        |world, mut state| {
                            // Auto-select agent if none selected but one is selected in global context
                            if let Some(entity) = self.selected_entities.as_slice().first()
                                && state.target_agent != Some(*entity)
                            {
                                state.target_agent = Some(*entity);
                                // Optional: clear filters on switch? Maybe not, persistence is good.
                            }

                            debug_knowledge::render_mind_inspector(ui, &mut state, world);
                        },
                    );
            }
            Tab::Social => {
                render_social_ui(self.world, ui, self.selected_entities.as_slice());
            }
        }
    }

    fn clear_background(&self, tab: &Self::Tab) -> bool {
        !matches!(tab, Tab::GameView)
    }
}

fn agent_viewer_ui_for_agent(world: &mut World, entity: Entity, ui: &mut egui::Ui) {
    // --- 1. Header (Quick Stats) ---
    ui.horizontal(|ui| {
        if let Some(name) = world.get::<Name>(entity) {
            ui.heading(name.as_str());
        } else {
            ui.heading("Unknown Agent");
        }
        ui.label(format!("(ID: {:?})", entity));
    });

    if let Some(action_state) = world.get::<crate::agent::actions::ActionState>(entity) {
        ui.horizontal(|ui| {
            ui.strong("Current Action:");
            ui.label(format!("{:?}", action_state.action_type));

            // Show target entity name if available
            if let Some(target_entity) = action_state.target_entity {
                if let Some(target_name) = world.get::<Name>(target_entity) {
                    ui.label(format!("→ {}", target_name.as_str()));
                } else {
                    ui.label(format!("→ {:?}", target_entity));
                }
            } else if let Some(target) = action_state.target_position {
                ui.label(format!("→ ({:.0}, {:.0})", target.x, target.y));
            }

            if action_state.ticks_remaining > 0 && action_state.ticks_remaining < u32::MAX {
                ui.label(format!("[{} ticks left]", action_state.ticks_remaining));
            }
        });
    }

    if let Some(emotions) = world.get::<crate::agent::psyche::emotions::EmotionalState>(entity) {
        let mood = emotions.current_mood;
        let mood_text = if mood > 0.5 {
            "Joyful"
        } else if mood > 0.1 {
            "Content"
        } else if mood > -0.1 {
            "Neutral"
        } else if mood > -0.5 {
            "Unhappy"
        } else {
            "Miserable"
        };
        let color = if mood > 0.0 {
            Color32::GREEN
        } else {
            Color32::RED
        };
        ui.colored_label(color, format!("Mood: {}", mood_text));
    }

    ui.separator();

    // --- 2. Nervous System (The Mind) ---
    egui::CollapsingHeader::new("🧠 Nervous System")
        .default_open(true)
        .show(ui, |ui| {
            // A. Consciousness Arbitration (3 Brains)
            if let Some(brain_state) =
                world.get::<crate::agent::brains::proposal::BrainState>(entity)
            {
                ui.heading("Consciousness Arbitration");

                // Power Bars
                let p = &brain_state.powers;
                ui.columns(3, |cols| {
                    cols[0].vertical_centered(|ui| {
                        ui.label("Survival");
                        ui.add(
                            egui::ProgressBar::new(p.survival).text(format!("{:.1}", p.survival)),
                        );
                    });
                    cols[1].vertical_centered(|ui| {
                        ui.label("Emotional");
                        ui.add(
                            egui::ProgressBar::new(p.emotional).text(format!("{:.1}", p.emotional)),
                        );
                    });
                    cols[2].vertical_centered(|ui| {
                        ui.label("Rational");
                        ui.add(
                            egui::ProgressBar::new(p.rational).text(format!("{:.1}", p.rational)),
                        );
                    });
                });

                // Winner
                if let Some(winner) = brain_state.winner {
                    ui.horizontal(|ui| {
                        ui.label("Controller:");
                        let text = format!("{:?}", winner).to_uppercase();
                        let color = match winner {
                            crate::agent::brains::proposal::BrainType::Survival => Color32::RED,
                            crate::agent::brains::proposal::BrainType::Emotional => {
                                Color32::from_rgb(255, 105, 180)
                            } // Hot Pink
                            crate::agent::brains::proposal::BrainType::Rational => Color32::CYAN,
                        };
                        ui.colored_label(color, egui::RichText::new(text).strong());
                    });
                }

                // Proposals
                ui.label("Proposals:");
                for prop in &brain_state.proposals {
                    let color = match prop.brain {
                        crate::agent::brains::proposal::BrainType::Survival => Color32::LIGHT_RED,
                        crate::agent::brains::proposal::BrainType::Emotional => {
                            Color32::from_rgb(255, 182, 193)
                        }
                        crate::agent::brains::proposal::BrainType::Rational => Color32::LIGHT_BLUE,
                    };
                    ui.colored_label(
                        color,
                        format!(
                            "• {:?}: {} ({:.1}) - {}",
                            prop.brain, prop.action.name, prop.urgency, prop.reasoning
                        ),
                    );
                }
                ui.separator();
            }

            // B. CNS (Drives)
            if let Some(cns) =
                world.get::<crate::agent::nervous_system::cns::CentralNervousSystem>(entity)
            {
                ui.heading("Central Nervous System (Drives)");

                // Urgencies
                if cns.urgencies.is_empty() {
                    ui.label("No active urgencies.");
                } else {
                    for urgency in &cns.urgencies {
                        ui.horizontal(|ui| {
                            ui.label(format!("{:?}", urgency.source));
                            ui.add(
                                egui::ProgressBar::new(urgency.value)
                                    .text(format!("{:.2}", urgency.value)),
                            );
                        });
                    }
                }

                // Goal
                if let Some(goal) = &cns.current_goal {
                    ui.horizontal(|ui| {
                        ui.strong("Current Goal:");
                        for pattern in &goal.conditions {
                            ui.label(format_pattern(pattern));
                        }
                    });
                    ui.label(format!("Priority: {:.2}", goal.priority));
                } else {
                    ui.label("No active goal.");
                }
                ui.separator();
            }

            // C. Rational Brain (The Planner)
            if let Some(brain) = world.get::<crate::agent::brains::rational::RationalBrain>(entity)
            {
                ui.heading("Rational Brain (Planner)");

                if let Some(plan) = &brain.current_plan {
                    ui.label(format!(
                        "Plan Status: EXECUTING (Step {}/{})",
                        brain.plan_index + 1,
                        plan.len()
                    ));

                    egui::ScrollArea::vertical()
                        .max_height(150.0)
                        .show(ui, |ui| {
                            for (i, step) in plan.iter().enumerate() {
                                let is_current = i == brain.plan_index;
                                let is_done = i < brain.plan_index;

                                // Build step text with target info
                                let target_info = if let Some(target_entity) = step.target_entity {
                                    if let Some(target_name) = world.get::<Name>(target_entity) {
                                        format!(" → {}", target_name.as_str())
                                    } else {
                                        format!(" → {:?}", target_entity)
                                    }
                                } else if let Some(pos) = step.target_position {
                                    format!(" → ({:.0}, {:.0})", pos.x, pos.y)
                                } else {
                                    String::new()
                                };
                                let step_text = format!("{}. {}{}", i + 1, step.name, target_info);

                                ui.horizontal(|ui| {
                                    if is_current {
                                        ui.colored_label(Color32::YELLOW, "▶");
                                        ui.strong(&step_text);
                                    } else if is_done {
                                        ui.colored_label(Color32::GRAY, "✓");
                                        ui.label(
                                            egui::RichText::new(&step_text)
                                                .strikethrough()
                                                .color(Color32::GRAY),
                                        );
                                    } else {
                                        ui.label(format!("   {}", step_text));
                                    }
                                });
                            }
                        });
                } else {
                    ui.label("Plan Status: IDLE (No Plan)");
                }
            }
        });

    ui.separator();

    // --- 3. Body & Physiology ---
    egui::CollapsingHeader::new("💪 Body & Physiology").show(ui, |ui| {
        if let Some(physical) = world.get::<crate::agent::body::needs::PhysicalNeeds>(entity) {
            egui::Grid::new("state_grid").show(ui, |ui| {
                ui.label("Hunger");
                ui.add(
                    egui::ProgressBar::new(physical.hunger / 100.0)
                        .text(format!("{:.1}", physical.hunger)),
                );
                ui.end_row();

                ui.label("Energy");
                ui.add(
                    egui::ProgressBar::new(physical.energy / 100.0)
                        .text(format!("{:.1}", physical.energy)),
                );
                ui.end_row();

                if let Some(consciousness) =
                    world.get::<crate::agent::body::needs::Consciousness>(entity)
                {
                    ui.label("Alertness");
                    ui.add(
                        egui::ProgressBar::new(consciousness.alertness)
                            .text(format!("{:.2}", consciousness.alertness)),
                    );
                    ui.end_row();
                }

                if let Some(emotions) =
                    world.get::<crate::agent::psyche::emotions::EmotionalState>(entity)
                {
                    // Stress display with color coding
                    let stress = emotions.stress_level;
                    let stress_color = if stress > 70.0 {
                        Color32::RED
                    } else if stress > 40.0 {
                        Color32::YELLOW
                    } else {
                        Color32::GREEN
                    };
                    ui.label("Stress");
                    ui.add(
                        egui::ProgressBar::new(stress / 100.0)
                            .text(format!("{:.1}", stress))
                            .fill(stress_color),
                    );
                    ui.end_row();
                }
            });
        }

        if let Some(body) = world.get::<crate::agent::biology::body::Body>(entity) {
            ui.separator();
            ui.strong("Anatomy Status");
            egui::Grid::new("anatomy_grid")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Part");
                    ui.strong("Health");
                    ui.strong("Status");
                    ui.end_row();

                    for (name, part) in [
                        ("Head", &body.head),
                        ("Torso", &body.torso),
                        ("L.Arm", &body.left_arm),
                        ("R.Arm", &body.right_arm),
                        ("L.Leg", &body.left_leg),
                        ("R.Leg", &body.right_leg),
                    ] {
                        ui.label(name);
                        let hp = part.current_hp / part.max_hp;
                        let color = if hp < 0.5 {
                            Color32::RED
                        } else {
                            Color32::GREEN
                        };
                        ui.add(egui::ProgressBar::new(hp).fill(color));

                        if part.injuries.is_empty() {
                            ui.label("OK");
                        } else {
                            ui.colored_label(Color32::RED, format!("{} Inj", part.injuries.len()));
                        }
                        ui.end_row();
                    }
                });
        }
    });

    ui.separator();

    // --- 4. Identity ---
    egui::CollapsingHeader::new("🆔 Personality").show(ui, |ui| {
        if let Some(personality) =
            world.get::<crate::agent::psyche::personality::Personality>(entity)
        {
            let t = &personality.traits;
            egui::Grid::new("traits_grid").show(ui, |ui| {
                ui.label("Openness");
                ui.add(egui::ProgressBar::new(t.openness).text(format!("{:.2}", t.openness)));
                ui.end_row();
                ui.label("Conscientiousness");
                ui.add(
                    egui::ProgressBar::new(t.conscientiousness)
                        .text(format!("{:.2}", t.conscientiousness)),
                );
                ui.end_row();
                ui.label("Extraversion");
                ui.add(
                    egui::ProgressBar::new(t.extraversion).text(format!("{:.2}", t.extraversion)),
                );
                ui.end_row();
                ui.label("Agreeableness");
                ui.add(
                    egui::ProgressBar::new(t.agreeableness).text(format!("{:.2}", t.agreeableness)),
                );
                ui.end_row();
                ui.label("Neuroticism");
                ui.add(egui::ProgressBar::new(t.neuroticism).text(format!("{:.2}", t.neuroticism)));
                ui.end_row();
            });
        }
    });

    ui.separator();

    // --- 5. Memory & Beliefs ---
    egui::CollapsingHeader::new("📜 Memory & Beliefs").show(ui, |ui| {
        // Working Memory
        ui.heading("Working Memory");
        if let Some(wm) = world.get::<crate::agent::mind::memory::WorkingMemory>(entity) {
            if wm.buffer.is_empty() {
                ui.label(egui::RichText::new("Empty").italics());
            } else {
                egui::ScrollArea::vertical()
                    .max_height(80.0)
                    .show(ui, |ui| {
                        for item in &wm.buffer {
                            ui.label(format!("{:?}", item.event));
                        }
                    });
            }
        }

        ui.separator();

        // Episodic (Events from MindGraph)
        ui.heading("Episodic Memory (MindGraph)");
        if let Some(mind) = world.get::<crate::agent::mind::knowledge::MindGraph>(entity) {
            ui.push_id("episodic_memory", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(150.0)
                    .show(ui, |ui| {
                        // Find all events: Subjects that are Node::Event(_)
                        // This is inefficient (scan all triples).
                        // In future, MindGraph should have an index for "Events".

                        use crate::agent::mind::knowledge::{Node, Predicate, Value};

                        // Collect events by ID to reconstruction
                        let mut events = std::collections::HashMap::new();

                        for triple in &mind.triples {
                            if let Node::Event(eid) = triple.subject {
                                let entry = events.entry(eid).or_insert_with(Vec::new);
                                entry.push(triple);
                            }
                        }

                        // Sort by ID (descending time)
                        let mut sorted_events: Vec<_> = events.into_iter().collect();
                        sorted_events.sort_by_key(|k| std::cmp::Reverse(k.0));

                        for (eid, triples) in sorted_events.iter().take(20) {
                            // Extract details
                            let mut _action = "Unknown";
                            let mut action_str = "Unknown".to_string(); // Changed to String
                            let mut target_str = "None".to_string(); // Changed to String
                            let mut timestamp = 0;

                            for t in triples {
                                match t.predicate {
                                    Predicate::Action => {
                                        if let Value::Action(a) = t.object {
                                            action_str = format!("{:?}", a); // Changed
                                        }
                                    }
                                    Predicate::Target => {
                                        if let Value::Entity(e) = t.object {
                                            target_str = format!("{:?}", e); // Changed
                                        }
                                    } // simpl
                                    Predicate::Timestamp => {
                                        if let Value::Int(ts) = t.object {
                                            timestamp = ts as u64;
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            let time_str = crate::core::GameTime::format_tick(timestamp);
                            ui.label(format!(
                                "{} [{}] {} -> {}",
                                time_str, eid, action_str, target_str
                            ));
                        }
                    });
            });
        }

        ui.separator();

        // Beliefs (Relationships from MindGraph)
        ui.heading("Beliefs (MindGraph)");
        if let Some(mind) = world.get::<crate::agent::mind::knowledge::MindGraph>(entity) {
            ui.push_id("beliefs", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(100.0)
                    .show(ui, |ui| {
                        // Query: (Entity, Relationship, Attitude)
                        // Or (Self, Relationship, Attitude)?
                        // In beliefs.rs I implemented: (Entity, Condition/Relationship, Attitude)
                        // Actually: (OtherEntity, Relationship, Attitude(val))

                        use crate::agent::mind::knowledge::{Node, Predicate, Value};

                        for triple in &mind.triples {
                            // Looking for "Relationship" or "HasTrait"
                            if triple.predicate == Predicate::Relationship
                                && let Node::Entity(subject) = triple.subject
                                && let Value::Attitude(score) = triple.object
                            {
                                ui.label(format!("{:?} -> Attitude {:.2}", subject, score));
                            }
                            if triple.predicate == Predicate::HasTrait
                                && let Node::Entity(subject) = triple.subject
                                && let Value::Concept(c) = triple.object
                            {
                                ui.label(format!("{:?} is {:?}", subject, c));
                            }
                        }
                    });
            });
        }
    });

    ui.separator();

    // --- 6. Inventory ---
    egui::CollapsingHeader::new("🎒 Inventory").show(ui, |ui| {
        if let Some(inventory) = world.get::<crate::agent::inventory::Inventory>(entity) {
            if inventory.items.is_empty() {
                ui.label("Empty");
            } else {
                for item in &inventory.items {
                    ui.horizontal(|ui| {
                        ui.label(format!("{:?}", item.concept));
                        ui.strong(format!("x{}", item.quantity));
                    });
                }
            }
        }
    });
}

fn format_pattern(p: &crate::agent::brains::thinking::TriplePattern) -> String {
    let s = p
        .subject
        .as_ref()
        .map(format_node)
        .unwrap_or("*".to_string());
    let pred = p
        .predicate
        .map(|pr| format!("{:?}", pr))
        .unwrap_or("*".to_string());
    let o = p
        .object
        .as_ref()
        .map(format_value)
        .unwrap_or("*".to_string());

    format!("{} {} {}", s, pred, o)
}

fn format_node(n: &crate::agent::mind::knowledge::Node) -> String {
    use crate::agent::mind::knowledge::Node;
    match n {
        Node::Self_ => "Self".to_string(),
        Node::Entity(e) => format!("Entity({:?})", e),
        Node::Concept(c) => format!("{:?}", c),
        Node::Tile((x, y)) => format!("Tile({},{})", x, y),
        Node::Action(a) => format!("Action({:?})", a),
        _ => format!("{:?}", n),
    }
}

fn format_value(v: &crate::agent::mind::knowledge::Value) -> String {
    match v {
        crate::agent::mind::knowledge::Value::Concept(c) => format!("{c:?}"),
        crate::agent::mind::knowledge::Value::Entity(e) => format!("Entity({:?})", e.index()),
        crate::agent::mind::knowledge::Value::Tile(t) => format!("Tile({},{})", t.0, t.1),
        crate::agent::mind::knowledge::Value::Float(f) => format!("{f:.2}"),
        crate::agent::mind::knowledge::Value::Int(i) => format!("{i}"),
        crate::agent::mind::knowledge::Value::Boolean(b) => format!("{b}"),
        crate::agent::mind::knowledge::Value::Item(c, qty) => format!("{c:?}({qty})"),
        crate::agent::mind::knowledge::Value::Emotion(e, i) => format!("{e:?}({i:.2})"),
        crate::agent::mind::knowledge::Value::Action(a) => format!("{a:?}"),
        crate::agent::mind::knowledge::Value::Attitude(a) => format!("{a:?}"),
        crate::agent::mind::knowledge::Value::Text(t) => format!("\"{t}\""),
    }
}

/// Render the Social UI showing relationships and conversations
fn render_social_ui(world: &mut World, ui: &mut egui::Ui, selected_entities: &[Entity]) {
    let entity = match selected_entities.first() {
        Some(e) => *e,
        None => {
            ui.label("Select an agent to view social info.");
            return;
        }
    };

    // Get agent name
    let agent_name = world
        .get::<Name>(entity)
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("{:?}", entity));

    ui.heading(format!("🤝 Social: {}", agent_name));
    ui.separator();

    // === RELATIONSHIPS ===
    ui.collapsing("💕 Relationships", |ui| {
        if let Some(mind) = world.get::<crate::agent::mind::knowledge::MindGraph>(entity) {
            use crate::agent::mind::knowledge::{Node, Predicate, Value};

            // Find all entities we "Know"
            let known_entities =
                mind.query(None, Some(Predicate::Knows), Some(&Value::Boolean(true)));

            if known_entities.is_empty() {
                ui.label("No known relationships yet.");
            } else {
                egui::Grid::new("relationships_grid")
                    .striped(true)
                    .min_col_width(80.0)
                    .show(ui, |ui| {
                        ui.strong("Person");
                        ui.strong("Trust");
                        ui.strong("Affection");
                        ui.end_row();

                        for triple in known_entities {
                            if let Node::Entity(other_entity) = triple.subject {
                                // Get their name
                                let other_name = world
                                    .get::<Name>(other_entity)
                                    .map(|n| n.to_string())
                                    .unwrap_or_else(|| format!("{:?}", other_entity));

                                // Get trust level
                                let trust = mind
                                    .query(
                                        Some(&Node::Entity(other_entity)),
                                        Some(Predicate::Trust),
                                        None,
                                    )
                                    .first()
                                    .and_then(|t| match &t.object {
                                        Value::Float(v) => Some(*v),
                                        _ => None,
                                    })
                                    .unwrap_or(0.0);

                                // Get affection level
                                let affection = mind
                                    .query(
                                        Some(&Node::Entity(other_entity)),
                                        Some(Predicate::Affection),
                                        None,
                                    )
                                    .first()
                                    .and_then(|t| match &t.object {
                                        Value::Float(v) => Some(*v),
                                        _ => None,
                                    })
                                    .unwrap_or(0.0);

                                ui.label(&other_name);

                                // Color trust bar
                                let trust_color = if trust > 0.0 {
                                    Color32::from_rgb(100, 200, 100)
                                } else {
                                    Color32::from_rgb(200, 100, 100)
                                };
                                ui.add(
                                    egui::ProgressBar::new((trust + 1.0) / 2.0) // normalize -1..1 to 0..1
                                        .fill(trust_color)
                                        .text(format!("{:.2}", trust)),
                                );

                                // Color affection bar
                                let affection_color = if affection > 0.0 {
                                    Color32::from_rgb(200, 150, 200)
                                } else {
                                    Color32::from_rgb(150, 150, 200)
                                };
                                ui.add(
                                    egui::ProgressBar::new((affection + 1.0) / 2.0)
                                        .fill(affection_color)
                                        .text(format!("{:.2}", affection)),
                                );

                                ui.end_row();
                            }
                        }
                    });
            }
        } else {
            ui.label("No MindGraph component.");
        }
    });

    ui.add_space(10.0);

    // === ACTIVE CONVERSATIONS ===
    ui.collapsing("💬 Active Conversations", |ui| {
        world.resource_scope::<crate::agent::mind::conversation::ConversationManager, _>(
            |inner_world, cm| {
                let active = cm.active_conversations();
                let my_conversations: Vec<_> = active
                    .filter(|c| c.participants.contains(&entity))
                    .collect();

                if my_conversations.is_empty() {
                    ui.label("No active conversations.");
                } else {
                    for conv in my_conversations {
                        let partner = conv.participants.iter().find(|&&p| p != entity).copied();

                        let partner_name = partner
                            .and_then(|p| inner_world.get::<Name>(p).map(|n| n.to_string()))
                            .unwrap_or_else(|| "Unknown".to_string());

                        ui.group(|ui| {
                            ui.label(format!(
                                "Conversation with {} ({} turns)",
                                partner_name,
                                conv.turns.len()
                            ));

                            // Show last few turns
                            let recent_turns: Vec<_> = conv.turns.iter().rev().take(3).collect();
                            for turn in recent_turns.iter().rev() {
                                let speaker_name = inner_world
                                    .get::<Name>(turn.speaker)
                                    .map(|n| n.to_string())
                                    .unwrap_or_else(|| format!("{:?}", turn.speaker));

                                let intent_icon = match turn.intent {
                                    crate::agent::mind::conversation::Intent::Greet => "👋",
                                    crate::agent::mind::conversation::Intent::Ask => "❓",
                                    crate::agent::mind::conversation::Intent::Share => "💡",
                                    crate::agent::mind::conversation::Intent::Acknowledge => "✅",
                                    crate::agent::mind::conversation::Intent::Thank => "🙏",
                                    crate::agent::mind::conversation::Intent::Farewell => "👋",
                                    _ => "💬",
                                };

                                ui.horizontal(|ui| {
                                    ui.label(format!(
                                        "{} {}: {:?}",
                                        intent_icon, speaker_name, turn.topic
                                    ));
                                    if !turn.content.is_empty() {
                                        ui.label(format!("({} facts)", turn.content.len()));
                                    }
                                });
                            }
                        });
                    }
                }
            },
        );
    });

    ui.add_space(10.0);

    // === PAST CONVERSATIONS ===
    ui.collapsing("📜 Past Conversations", |ui| {
        world.resource_scope::<crate::agent::mind::conversation::ConversationManager, _>(
            |inner_world, cm| {
                let ended_conversations: Vec<_> = cm
                    .conversations
                    .values()
                    .filter(|c| c.state == crate::agent::mind::conversation::ConversationState::Ended)
                    .filter(|c| c.participants.contains(&entity))
                    .collect();

                if ended_conversations.is_empty() {
                    ui.label("No past conversations.");
                } else {
                    ui.label(format!("Showing {} past conversation(s)", ended_conversations.len()));

                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for conv in ended_conversations.iter().rev().take(10) {
                                let partner = conv.participants.iter().find(|&&p| p != entity).copied();

                                let partner_name = partner
                                    .and_then(|p| inner_world.get::<Name>(p).map(|n| n.to_string()))
                                    .unwrap_or_else(|| "Unknown".to_string());

                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(format!(
                                            "With {} ({} turns)",
                                            partner_name,
                                            conv.turns.len()
                                        ));
                                        ui.label(format!("ID: {}", conv.id));
                                    });

                                    // Show all turns
                                    for turn in &conv.turns {
                                        let speaker_name = inner_world
                                            .get::<Name>(turn.speaker)
                                            .map(|n| n.to_string())
                                            .unwrap_or_else(|| format!("{:?}", turn.speaker));

                                        let intent_icon = match turn.intent {
                                            crate::agent::mind::conversation::Intent::Greet => "👋",
                                            crate::agent::mind::conversation::Intent::Ask => "❓",
                                            crate::agent::mind::conversation::Intent::Answer => "💬",
                                            crate::agent::mind::conversation::Intent::Share => "💡",
                                            crate::agent::mind::conversation::Intent::Acknowledge => "✅",
                                            crate::agent::mind::conversation::Intent::Thank => "🙏",
                                            crate::agent::mind::conversation::Intent::Farewell => "👋",
                                            crate::agent::mind::conversation::Intent::Empathize => "❤️",
                                            crate::agent::mind::conversation::Intent::Agree => "👍",
                                            crate::agent::mind::conversation::Intent::Disagree => "👎",
                                        };

                                        let me = turn.speaker == entity;
                                        let prefix = if me { "  Me" } else { "Them" };

                                        ui.horizontal(|ui| {
                                            ui.label(format!(
                                                "{} {}: {:?}",
                                                prefix, intent_icon, turn.topic
                                            ));
                                            if !turn.content.is_empty() {
                                                ui.label(format!("({} facts)", turn.content.len()));
                                            }
                                            if turn.expects_response {
                                                ui.colored_label(egui::Color32::YELLOW, "❓");
                                            }
                                        });
                                    }
                                });
                                ui.add_space(5.0);
                            }
                        });
                }
            },
        );
    });

    ui.add_space(10.0);

    // === SOCIAL NEEDS ===
    ui.collapsing("🧠 Social State", |ui| {
        if let Some(mind) = world.get::<crate::agent::mind::knowledge::MindGraph>(entity) {
            use crate::agent::mind::knowledge::{Node, Predicate, Value};

            // Social drive
            let social_drive = mind
                .query(Some(&Node::Self_), Some(Predicate::SocialDrive), None)
                .first()
                .and_then(|t| match &t.object {
                    Value::Float(v) => Some(*v),
                    Value::Int(v) => Some(*v as f32),
                    _ => None,
                })
                .unwrap_or(0.0);

            ui.horizontal(|ui| {
                ui.label("Loneliness:");
                ui.add(
                    egui::ProgressBar::new(social_drive)
                        .text(format!("{:.0}%", social_drive * 100.0)),
                );
            });

            // Count known people
            let known_count = mind
                .query(None, Some(Predicate::Knows), Some(&Value::Boolean(true)))
                .len();

            ui.label(format!("Known people: {}", known_count));
        }
    });
}
