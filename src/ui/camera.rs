use crate::agent::player::{PlayerControlled, follow_position};
use crate::ui::UiState;
use bevy::input::gestures::PinchGesture;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContext, PrimaryEguiContext, egui};

/// Per-frame catch-up factor for adventure-mode camera follow. Tuned so a
/// player teleporting across the map snaps to the new position in ~1 second
/// at 60 FPS while normal walk steps look glued to the camera.
const FOLLOW_ALPHA: f32 = 0.15;

/// Orthographic scale to use when entering Adventure mode. Smaller = more
/// zoomed in. Default scale in Bevy is 1.0; 0.5 doubles the apparent
/// detail, which matches the closer-in feel a single-character POV wants
/// (you care about your immediate surroundings, not a 50×50 overview).
const ADVENTURE_DEFAULT_ZOOM: f32 = 0.5;

/// Should the camera respond to a gesture at this cursor position?
///
/// Two layers of gating:
/// 1. If egui already wants to handle this pointer (cursor over a panel like
///    the character sheet), the camera stays out of the way.
/// 2. If the debug dock is enabled, only the inner game-view rect counts as
///    the game viewport. Without the dock, the whole window does.
pub fn cursor_in_game_viewport(
    cursor: Vec2,
    ui_state: Option<&UiState>,
    ctx: &mut egui::Context,
) -> bool {
    if ctx.is_pointer_over_area() {
        return false;
    }
    let Some(ui_state) = ui_state else {
        return true;
    };
    let viewport = ui_state.viewport_rect;
    if viewport.width() <= 0.0 || viewport.height() <= 0.0 {
        return true;
    }
    viewport.contains(egui::pos2(cursor.x, cursor.y))
}

/// Gate by `cursor_in_game_viewport` and project to world coords. Returns
/// `None` when the cursor is over egui chrome, outside the dock's game rect,
/// or when the projection fails.
pub fn cursor_to_world(
    cursor: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    ui_state: Option<&UiState>,
    ctx: &mut egui::Context,
) -> Option<Vec2> {
    if !cursor_in_game_viewport(cursor, ui_state, ctx) {
        return None;
    }
    camera.viewport_to_world_2d(camera_transform, cursor).ok()
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                camera_zoom,
                camera_drag,
                touchpad_pinch_zoom,
                touchpad_pan,
                // Follow runs after the manual pan/zoom systems so a held
                // middle-click still wins the frame; releasing the button
                // lets the lerp pull the camera back toward the player.
                camera_follow_player.after(camera_drag).after(touchpad_pan),
            )
                .run_if(crate::menu::sim_interactive),
        )
        .add_systems(
            OnEnter(crate::menu::AppState::InSim),
            apply_adventure_default_zoom.run_if(in_adventure_mode),
        );
    }
}

fn in_adventure_mode(sim_config: Option<Res<crate::menu::SimConfig>>) -> bool {
    sim_config
        .map(|c| matches!(c.mode, crate::menu::SimMode::Adventure))
        .unwrap_or(false)
}

/// Pin the orthographic camera scale to `ADVENTURE_DEFAULT_ZOOM` when
/// entering Adventure mode. Subsequent scroll-wheel/pinch input still
/// adjusts from this starting point — we just bias the initial framing
/// toward the player's immediate surroundings instead of the full map.
fn apply_adventure_default_zoom(mut cameras: Query<&mut Projection, With<Camera>>) {
    for mut projection in cameras.iter_mut() {
        if let Projection::Orthographic(ref mut ortho) = *projection {
            ortho.scale = ADVENTURE_DEFAULT_ZOOM;
        }
    }
}

/// Lerp the 2D camera toward the possessed agent's transform each frame.
/// Suspended while the user is mid middle-click drag so manual panning
/// works as before — releasing the button hands control back to the
/// follow lerp.
fn camera_follow_player(
    buttons: Res<ButtonInput<MouseButton>>,
    player_q: Query<&Transform, (With<PlayerControlled>, Without<Camera>)>,
    mut camera_q: Query<&mut Transform, With<Camera>>,
) {
    if buttons.pressed(MouseButton::Middle) {
        return;
    }
    let Ok(player) = player_q.single() else {
        return;
    };
    let Ok(mut camera) = camera_q.single_mut() else {
        return;
    };
    let target = player.translation.truncate();
    let current = camera.translation.truncate();
    let next = follow_position(current, target, FOLLOW_ALPHA);
    camera.translation.x = next.x;
    camera.translation.y = next.y;
}

// Scroll Wheel Zoom (mouse only - skips trackpad pixel scrolling)
fn camera_zoom(
    mut events: MessageReader<MouseWheel>,
    mut cameras: Query<&mut Projection, With<Camera>>,
    ui_state: Option<Res<UiState>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut egui_ctxs: Query<&mut EguiContext, With<PrimaryEguiContext>>,
) {
    use bevy::input::mouse::MouseScrollUnit;

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        for _ in events.read() {}
        return;
    };
    let Ok(mut egui_ctx) = egui_ctxs.single_mut() else {
        for _ in events.read() {}
        return;
    };

    if !cursor_in_game_viewport(cursor_pos, ui_state.as_deref(), egui_ctx.get_mut()) {
        for _ in events.read() {}
        return;
    }

    for event in events.read() {
        // Only handle line-based scrolling (mouse wheel)
        // Pixel-based scrolling (trackpad) is handled by touchpad_pan
        if event.unit != MouseScrollUnit::Line {
            continue;
        }

        for mut projection in cameras.iter_mut() {
            if let Projection::Orthographic(ref mut ortho) = *projection {
                let zoom_speed = 0.1;
                // Negative event.y means scrolling down (zoom out), positive means up (zoom in)
                // We subtract because smaller scale = zoomed in
                ortho.scale -= event.y * zoom_speed * ortho.scale;

                // Clamp zoom level
                ortho.scale = ortho.scale.clamp(0.1, 5.0);
            }
        }
    }
}

// Middle-Click Drag Panning
fn camera_drag(
    mut events: MessageReader<MouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut cameras: Query<(&mut Transform, &Projection), With<Camera>>,
    ui_state: Option<Res<UiState>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut egui_ctxs: Query<&mut EguiContext, With<PrimaryEguiContext>>,
) {
    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok(mut egui_ctx) = egui_ctxs.single_mut() else {
        return;
    };

    if !cursor_in_game_viewport(cursor_pos, ui_state.as_deref(), egui_ctx.get_mut()) {
        return;
    }

    if buttons.pressed(MouseButton::Middle) {
        for event in events.read() {
            for (mut transform, projection) in cameras.iter_mut() {
                if let Projection::Orthographic(ortho) = projection {
                    transform.translation.x -= event.delta.x * ortho.scale;
                    transform.translation.y += event.delta.y * ortho.scale;
                }
            }
        }
    }
}

// macOS Trackpad Pinch-to-Zoom
fn touchpad_pinch_zoom(
    mut events: MessageReader<PinchGesture>,
    mut cameras: Query<&mut Projection, With<Camera>>,
    ui_state: Option<Res<UiState>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut egui_ctxs: Query<&mut EguiContext, With<PrimaryEguiContext>>,
) {
    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        for _ in events.read() {}
        return;
    };
    let Ok(mut egui_ctx) = egui_ctxs.single_mut() else {
        for _ in events.read() {}
        return;
    };

    if !cursor_in_game_viewport(cursor_pos, ui_state.as_deref(), egui_ctx.get_mut()) {
        for _ in events.read() {}
        return;
    }

    for event in events.read() {
        for mut projection in cameras.iter_mut() {
            if let Projection::Orthographic(ref mut ortho) = *projection {
                // PinchGesture.0 is positive for zoom in, negative for zoom out
                // Subtract because smaller scale = zoomed in
                ortho.scale -= event.0 * ortho.scale;
                ortho.scale = ortho.scale.clamp(0.1, 5.0);
            }
        }
    }
}

// macOS Trackpad Two-Finger Pan (pixel-based scrolling)
fn touchpad_pan(
    mut events: MessageReader<MouseWheel>,
    mut cameras: Query<(&mut Transform, &Projection), With<Camera>>,
    ui_state: Option<Res<UiState>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut egui_ctxs: Query<&mut EguiContext, With<PrimaryEguiContext>>,
) {
    use bevy::input::mouse::MouseScrollUnit;

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        for _ in events.read() {}
        return;
    };
    let Ok(mut egui_ctx) = egui_ctxs.single_mut() else {
        for _ in events.read() {}
        return;
    };

    if !cursor_in_game_viewport(cursor_pos, ui_state.as_deref(), egui_ctx.get_mut()) {
        for _ in events.read() {}
        return;
    }

    for event in events.read() {
        // Only handle pixel-based scrolling (trackpad)
        // Line-based scrolling (mouse wheel) is handled by camera_zoom
        if event.unit != MouseScrollUnit::Pixel {
            continue;
        }

        for (mut transform, projection) in cameras.iter_mut() {
            if let Projection::Orthographic(ortho) = projection {
                // Two-finger scroll on trackpad
                let pan_speed = 1.0;
                transform.translation.x -= event.x * pan_speed * ortho.scale;
                transform.translation.y += event.y * pan_speed * ortho.scale;
            }
        }
    }
}
