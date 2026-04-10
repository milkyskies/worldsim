//! Procedural sprite animation: bouncy arc movement with squash-and-stretch.
//!
//! Moving sprites trace little arcs ⌒⌒⌒⌒ instead of sliding flat.
//! Idle sprites are completely still.
//!
//! Reads: Transform (root position for velocity), Time
//! Writes: Transform on SpriteBody entities (visual Y offset + scale only)
//! Upstream: movement systems (root Transform changes)
//! Downstream: purely visual — no simulation systems read these offsets

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use std::collections::HashMap;

pub struct SpriteAnimationPlugin;

impl Plugin for SpriteAnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            animate_sprite_bodies.before(TransformSystems::Propagate),
        );
    }
}

/// Wraps all sprite parts. Animated vertically to create bounce.
/// Name tag is a sibling (not inside this), so it stays still.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct SpriteBody {
    pub root: Entity,
    phase: f32,
}

impl SpriteBody {
    pub fn new(root: Entity, phase: f32) -> Self {
        Self { root, phase }
    }
}

/// Bounce arc + squash-and-stretch from a normalized 0..1 cycle position.
/// Returns (y_offset, x_scale, y_scale).
fn bounce_frame(cycle: f32, height: f32) -> (f32, f32, f32) {
    // Parabolic arc: 4t(1-t) gives 0 at edges, 1 at center
    let arc = 4.0 * cycle * (1.0 - cycle);
    let y = arc * height;

    // Squash at bottom of arc (cycle near 0 or 1), stretch at top
    let near_ground = 1.0 - arc; // 1 at ground, 0 at peak
    let squish = near_ground * near_ground * 0.12;

    let x_scale = 1.0 + squish;
    let y_scale = 1.0 - squish;

    (y, x_scale, y_scale)
}

/// Per-entity movement tracking.
#[derive(Default)]
struct MoveTracker {
    prev_pos: Option<Vec2>,
    /// Last wall-clock time the root position changed.
    last_moved_at: f32,
}

fn animate_sprite_bodies(
    time: Res<Time>,
    body_query: Query<(Entity, &SpriteBody)>,
    mut transforms: Query<&mut Transform>,
    mut trackers: Local<HashMap<Entity, MoveTracker>>,
) {
    let t = time.elapsed_secs();

    let mut alive = Vec::new();

    for (body_entity, body) in body_query.iter() {
        alive.push(body.root);

        let root_pos = transforms
            .get(body.root)
            .map(|tr| tr.translation.truncate())
            .unwrap_or(Vec2::ZERO);

        let tracker = trackers.entry(body.root).or_default();
        let prev = tracker.prev_pos.unwrap_or(root_pos);

        if root_pos.distance(prev) > 0.01 {
            tracker.last_moved_at = t;
        }
        tracker.prev_pos = Some(root_pos);

        // Consider "moving" if position changed within the last 0.2 seconds
        let is_moving = (t - tracker.last_moved_at) < 0.2;

        let (y_offset, x_scale, y_scale) = if is_moving {
            let bounces_per_sec = 2.5;
            let cycle = ((t * bounces_per_sec + body.phase) % 1.0).clamp(0.0, 1.0);
            bounce_frame(cycle, 3.0)
        } else {
            (0.0, 1.0, 1.0)
        };

        if let Ok(mut bt) = transforms.get_mut(body_entity) {
            bt.translation.y = y_offset;
            bt.scale = Vec3::new(x_scale, y_scale, 1.0);
        }
    }

    if trackers.len() > alive.len() {
        trackers.retain(|e, _| alive.contains(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounce_at_start_is_on_ground() {
        let (y, sx, sy) = bounce_frame(0.0, 3.0);
        assert!(y.abs() < 0.01);
        assert!(sx > 1.0, "squished wide at ground");
        assert!(sy < 1.0, "squished short at ground");
    }

    #[test]
    fn bounce_at_peak_is_at_height() {
        let (y, sx, sy) = bounce_frame(0.5, 3.0);
        assert!((y - 3.0).abs() < 0.01, "at peak, got y={y}");
        assert!((sx - 1.0).abs() < 0.02, "no squish at peak");
        assert!((sy - 1.0).abs() < 0.02, "no squish at peak");
    }

    #[test]
    fn bounce_at_end_is_on_ground() {
        let (y, _, _) = bounce_frame(1.0, 3.0);
        assert!(y.abs() < 0.01);
    }

    #[test]
    fn bounce_never_negative() {
        for i in 0..100 {
            let c = i as f32 / 100.0;
            let (y, _, _) = bounce_frame(c, 3.0);
            assert!(y >= 0.0, "y={y} at cycle={c}");
        }
    }

    #[test]
    fn bounce_is_symmetric() {
        let (y1, _, _) = bounce_frame(0.3, 3.0);
        let (y2, _, _) = bounce_frame(0.7, 3.0);
        assert!((y1 - y2).abs() < 0.01, "should be symmetric: {y1} vs {y2}");
    }
}
