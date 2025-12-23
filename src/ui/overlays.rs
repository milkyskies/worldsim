use crate::agent::{Agent, TargetPosition};
use crate::agent::mind::perception::{VisibleObjects, Vision};
use bevy::prelude::*;

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlayState>()
            .register_type::<OverlayState>()
            .add_systems(Update, draw_overlays);
    }
}

#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
pub struct OverlayState {
    pub show_vision: bool,
    pub show_intent: bool,
}

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
            && let Some(target_pos) = target.0 {
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
