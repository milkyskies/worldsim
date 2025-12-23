use crate::ui::UiState;
use bevy::input::gestures::PinchGesture;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::egui;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (camera_zoom, camera_drag, touchpad_pinch_zoom, touchpad_pan),
        );
    }
}

// Scroll Wheel Zoom (mouse only - skips trackpad pixel scrolling)
fn camera_zoom(
    mut events: MessageReader<MouseWheel>,
    mut cameras: Query<&mut Projection, With<Camera>>,
    ui_state: Option<Res<UiState>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    use bevy::input::mouse::MouseScrollUnit;

    // Only zoom if cursor is inside game viewport
    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        // Consume events but don't apply
        for _ in events.read() {}
        return;
    };

    // Check if cursor is in viewport
    if let Some(ui_state) = ui_state {
        let viewport = ui_state.viewport_rect;
        if !viewport.contains(egui::pos2(cursor_pos.x, cursor_pos.y)) {
            // Consume events but don't apply
            for _ in events.read() {}
            return;
        }
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

// Right-Click Drag Panning
fn camera_drag(
    mut events: MessageReader<MouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut cameras: Query<(&mut Transform, &Projection), With<Camera>>,
    ui_state: Option<Res<UiState>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    // Only drag if cursor is inside game viewport
    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    // Check if cursor is in viewport
    if let Some(ui_state) = ui_state {
        let viewport = ui_state.viewport_rect;
        if !viewport.contains(egui::pos2(cursor_pos.x, cursor_pos.y)) {
            return;
        }
    }

    if buttons.pressed(MouseButton::Right) {
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
) {
    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        for _ in events.read() {}
        return;
    };

    if let Some(ui_state) = ui_state {
        let viewport = ui_state.viewport_rect;
        if !viewport.contains(egui::pos2(cursor_pos.x, cursor_pos.y)) {
            for _ in events.read() {}
            return;
        }
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
) {
    use bevy::input::mouse::MouseScrollUnit;

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        for _ in events.read() {}
        return;
    };

    if let Some(ui_state) = ui_state {
        let viewport = ui_state.viewport_rect;
        if !viewport.contains(egui::pos2(cursor_pos.x, cursor_pos.y)) {
            for _ in events.read() {}
            return;
        }
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
