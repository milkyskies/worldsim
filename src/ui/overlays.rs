use crate::agent::mind::perception::{VisibleObjects, Vision};
use crate::agent::{Agent, TargetPosition};
use crate::world::field_grid::FIELD_CHUNK_SIZE;
use crate::world::field_grid_plugin::FieldGrids;
use crate::world::map::TILE_SIZE;
use crate::world::spatial_index::world_pos_to_tile;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContext, EguiPrimaryContextPass, PrimaryEguiContext, egui};

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlayState>()
            .register_type::<OverlayState>()
            .add_systems(Update, (draw_overlays, draw_temperature_overlay))
            // Tooltip has to draw in the egui primary pass; running it
            // in Update silently drops the call because no egui
            // context is active there.
            .add_systems(EguiPrimaryContextPass, temperature_hover_tooltip);
    }
}

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
pub struct OverlayState {
    pub show_vision: bool,
    pub show_intent: bool,
    pub show_temperature: bool,
}

/// Marker for the per-cell sprites drawn by `draw_temperature_overlay`.
/// Despawned and respawned each frame when the overlay is on — simple
/// and cheap at the ~hundreds-of-cells scale the grid ever reaches.
#[derive(Component)]
struct TemperatureOverlayCell;

/// Delta magnitude (°C) that saturates the overlay's color intensity.
const OVERLAY_SATURATION_DELTA_C: f32 = 20.0;
/// Max alpha for a saturated-delta cell. Kept translucent so the world
/// underneath stays readable, but high enough to actually notice.
const OVERLAY_MAX_ALPHA: f32 = 0.75;
/// Minimum alpha applied to any rendered cell so faint perturbations
/// are still visible (otherwise sub-saturation cells would be all but
/// invisible, making the overlay seem broken).
const OVERLAY_MIN_ALPHA: f32 = 0.25;
/// Delta magnitude (°C) below which a cell is skipped entirely.
const OVERLAY_SKIP_THRESHOLD_C: f32 = 0.1;
/// Z depth for overlay sprites — above tiles and entities.
const OVERLAY_Z: f32 = 50.0;

fn draw_overlays(
    mut gizmos: Gizmos,
    overlay_state: Res<OverlayState>,
    agents: Query<(&Transform, &Vision, &VisibleObjects, &TargetPosition), With<Agent>>,
) {
    for (transform, vision, visible_objects, target) in agents.iter() {
        let pos = transform.translation.truncate();
        let _pos3 = transform.translation;

        // Vision Overlay
        if overlay_state.show_vision {
            // Draw Range Circle
            gizmos.circle_2d(pos, vision.range, Color::srgba(0.0, 0.0, 1.0, 0.3));

            // Draw Lines to Visible Objects
            for &_entity in visible_objects.entities.iter() {
                // We'd need to query the entity's position to draw a line to it.
                // Since this query doesn't have it, we can't easily draw the line without another query or looking up components.
                // However, we can roughly estimate or skip for now to keep performance high,
                // OR we can add a helper or do a paramset query?
                // Let's keep it simple: Just the range circle is a huge help.
            }
        }

        // Intent Overlay
        if overlay_state.show_intent
            && let Some(target_pos) = target.0
        {
            // Draw line to target
            gizmos.line_2d(pos, target_pos, Color::srgba(1.0, 0.5, 0.0, 0.8));

            // Draw target X
            let x_size = 5.0;
            gizmos.line_2d(
                target_pos + Vec2::new(-x_size, -x_size),
                target_pos + Vec2::new(x_size, x_size),
                Color::srgba(1.0, 0.5, 0.0, 0.8),
            );
            gizmos.line_2d(
                target_pos + Vec2::new(-x_size, x_size),
                target_pos + Vec2::new(x_size, -x_size),
                Color::srgba(1.0, 0.5, 0.0, 0.8),
            );
        }
    }
}

fn draw_temperature_overlay(
    mut commands: Commands,
    overlay_state: Res<OverlayState>,
    fields: Res<FieldGrids>,
    existing: Query<Entity, With<TemperatureOverlayCell>>,
) {
    for entity in existing.iter() {
        commands.entity(entity).despawn();
    }

    if !overlay_state.show_temperature {
        return;
    }

    let grid = fields.temperature();
    for (chunk_coord, chunk) in grid.iter_chunks() {
        for local_y in 0..FIELD_CHUNK_SIZE {
            for local_x in 0..FIELD_CHUNK_SIZE {
                let delta = chunk.delta_at(local_x, local_y);
                if delta.abs() < OVERLAY_SKIP_THRESHOLD_C {
                    continue;
                }
                let world_tile_x = chunk_coord.x * FIELD_CHUNK_SIZE + local_x;
                let world_tile_y = chunk_coord.y * FIELD_CHUNK_SIZE + local_y;
                let center = Vec2::new(
                    (world_tile_x as f32 + 0.5) * TILE_SIZE,
                    (world_tile_y as f32 + 0.5) * TILE_SIZE,
                );
                commands.spawn((
                    TemperatureOverlayCell,
                    Sprite {
                        color: heat_color(delta),
                        custom_size: Some(Vec2::splat(TILE_SIZE)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(center.x, center.y, OVERLAY_Z)),
                    Visibility::default(),
                    InheritedVisibility::default(),
                    ViewVisibility::default(),
                ));
            }
        }
    }
}

/// Map a cell delta (°C above/below ambient) to an RGBA tint. Hot
/// cells go red, cold cells blue, alpha scales with magnitude but
/// stays above a visible floor so faint cells don't disappear.
fn heat_color(delta_c: f32) -> Color {
    let intensity = (delta_c.abs() / OVERLAY_SATURATION_DELTA_C).clamp(0.0, 1.0);
    let alpha = OVERLAY_MIN_ALPHA + intensity * (OVERLAY_MAX_ALPHA - OVERLAY_MIN_ALPHA);
    if delta_c >= 0.0 {
        Color::srgba(1.0, 0.35, 0.1, alpha)
    } else {
        Color::srgba(0.1, 0.4, 1.0, alpha)
    }
}

/// RimWorld-style hover probe: when the Temperature overlay is on,
/// render a small floating popup near the cursor with the sampled
/// tile's °C. Uses `egui::Area` rather than `show_tooltip_at_pointer`
/// because the latter requires an associated widget's LayerId — for a
/// free-floating probe over the game viewport, an Area at a fixed
/// cursor-relative offset is the right tool.
fn temperature_hover_tooltip(
    overlay_state: Res<OverlayState>,
    fields: Res<FieldGrids>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut egui_contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>,
) {
    if !overlay_state.show_temperature {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Ok(mut egui_context) = egui_contexts.single_mut() else {
        return;
    };
    let ctx = egui_context.get_mut();
    if ctx.is_pointer_over_area() {
        return;
    }
    let Some((camera, camera_transform)) = cameras.iter().next() else {
        return;
    };
    let Ok(world_position) = camera.viewport_to_world_2d(camera_transform, cursor_position) else {
        return;
    };
    let tile = world_pos_to_tile(world_position);
    let temp = fields.temperature().sample_tile(tile);
    let delta = fields.temperature().delta_at_tile(tile);

    let cursor_egui_pos = egui::pos2(cursor_position.x, cursor_position.y);
    egui::Area::new("temp_probe".into())
        .order(egui::Order::Tooltip)
        .fixed_pos(cursor_egui_pos + egui::vec2(14.0, 14.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(&ctx.style()).show(ui, |ui| {
                ui.label(format!("tile ({}, {})", tile.x, tile.y));
                ui.strong(format!("{:.1} °C", temp));
                if delta.abs() >= 0.1 {
                    let sign = if delta >= 0.0 { "+" } else { "" };
                    ui.label(format!("{sign}{:.1}°C vs ambient", delta));
                }
            });
        });
}
