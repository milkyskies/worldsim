//! Procedural sprite animation: hop-based movement with squash-and-stretch.
//!
//! Reads: Transform (position delta for velocity), Time
//! Writes: Transform on SpriteBody entities (visual offsets only)
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

/// Marker for the intermediate entity that wraps all sprite parts (but not the name tag).
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct SpriteBody {
    pub phase: f32,
    pub root: Entity,
}

impl SpriteBody {
    pub fn new(root: Entity, phase: f32) -> Self {
        Self { phase, root }
    }
}

// ── Hop math ────────────────────────────────────────────────────────────────

/// One hop cycle. `cycle` goes from 0.0 to 1.0.
///   0.00 - 0.10  squish down (crouch before jump)
///   0.10 - 0.50  airborne (parabolic arc)
///   0.50 - 0.60  land + squish
///   0.60 - 1.00  rest on ground (completely still)
///
/// Returns (y_offset, x_scale, y_scale).
fn hop_frame(cycle: f32, height: f32) -> (f32, f32, f32) {
    if cycle < 0.1 {
        // Pre-jump crouch: squish down
        let t = cycle / 0.1; // 0 to 1
        let squish = t * 0.2;
        (0.0, 1.0 + squish, 1.0 - squish)
    } else if cycle < 0.5 {
        // Airborne: parabolic arc
        let t = (cycle - 0.1) / 0.4; // 0 to 1
        let y = 4.0 * t * (1.0 - t) * height; // peaks at t=0.5
        // Stretch tall while airborne
        let stretch = if t < 0.3 {
            t / 0.3 * 0.1 // stretch up on launch
        } else if t > 0.7 {
            (1.0 - t) / 0.3 * 0.1 // compress on descent
        } else {
            0.1 // slight stretch at peak
        };
        (y, 1.0 - stretch * 0.5, 1.0 + stretch)
    } else if cycle < 0.6 {
        // Landing squish
        let t = (cycle - 0.5) / 0.1; // 0 to 1
        let squish = (1.0 - t) * 0.2; // decays from 0.2 to 0
        (0.0, 1.0 + squish, 1.0 - squish)
    } else {
        // Resting on ground — completely still
        (0.0, 1.0, 1.0)
    }
}

// ── Main system ─────────────────────────────────────────────────────────────

fn animate_sprite_bodies(
    time: Res<Time>,
    body_query: Query<(Entity, &SpriteBody)>,
    mut transforms: Query<&mut Transform>,
    mut prev_positions: Local<HashMap<Entity, Vec2>>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();

    let mut alive_roots = Vec::new();

    for (body_entity, body) in body_query.iter() {
        let root_entity = body.root;
        alive_roots.push(root_entity);

        let root_pos = transforms
            .get(root_entity)
            .map(|t| t.translation.truncate())
            .unwrap_or(Vec2::ZERO);

        let speed = if dt > 0.0 {
            prev_positions
                .get(&root_entity)
                .map(|prev| root_pos.distance(*prev) / dt)
                .unwrap_or(0.0)
        } else {
            0.0
        };
        prev_positions.insert(root_entity, root_pos);

        let is_moving = speed > 5.0;

        let (y_offset, x_scale, y_scale) = if is_moving {
            // ~1.5 hops per second. Each cycle: 10% crouch, 40% airborne, 10% land, 40% rest.
            let hops_per_sec = 1.5;
            let cycle = ((t * hops_per_sec + body.phase) % 1.0).clamp(0.0, 1.0);
            hop_frame(cycle, 3.0)
        } else {
            (0.0, 1.0, 1.0)
        };

        if let Ok(mut body_transform) = transforms.get_mut(body_entity) {
            body_transform.translation.y = y_offset;
            body_transform.scale = Vec3::new(x_scale, y_scale, 1.0);
        }
    }

    if prev_positions.len() > alive_roots.len() {
        prev_positions.retain(|e, _| alive_roots.contains(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crouch_phase_squishes_down() {
        let (y, sx, sy) = hop_frame(0.05, 3.0);
        assert!(y.abs() < 0.01, "should be on ground during crouch");
        assert!(sx > 1.0, "should be wider during crouch, got {sx}");
        assert!(sy < 1.0, "should be shorter during crouch, got {sy}");
    }

    #[test]
    fn airborne_phase_reaches_peak() {
        // Midpoint of airborne phase: cycle=0.3, t within airborne = 0.5
        let (y, _, _) = hop_frame(0.3, 3.0);
        assert!(
            (y - 3.0).abs() < 0.01,
            "should be at peak height, got y={y}"
        );
    }

    #[test]
    fn landing_phase_squishes() {
        let (y, sx, sy) = hop_frame(0.51, 3.0);
        assert!(y.abs() < 0.01, "should be on ground at landing");
        assert!(sx > 1.0, "should be wider on landing, got {sx}");
        assert!(sy < 1.0, "should be shorter on landing, got {sy}");
    }

    #[test]
    fn rest_phase_is_completely_still() {
        for i in 6..10 {
            let cycle = i as f32 / 10.0;
            let (y, sx, sy) = hop_frame(cycle, 3.0);
            assert!(y.abs() < 0.01, "should be on ground at rest, got y={y}");
            assert!(
                (sx - 1.0).abs() < 0.01,
                "no scale change at rest, got sx={sx}"
            );
            assert!(
                (sy - 1.0).abs() < 0.01,
                "no scale change at rest, got sy={sy}"
            );
        }
    }

    #[test]
    fn hop_never_goes_below_ground() {
        for i in 0..100 {
            let cycle = i as f32 / 100.0;
            let (y, _, _) = hop_frame(cycle, 3.0);
            assert!(
                y >= 0.0,
                "y should never be negative, got {y} at cycle={cycle}"
            );
        }
    }
}
