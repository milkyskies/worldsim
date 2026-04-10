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
/// The animation system targets this entity, so the name tag stays still.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct SpriteBody {
    /// Per-entity phase offset so agents don't all hop in sync
    pub phase: f32,
    /// The root agent entity (for reading logical position)
    pub root: Entity,
}

impl SpriteBody {
    pub fn new(root: Entity, phase: f32) -> Self {
        Self { phase, root }
    }
}

// ── Hop math ────────────────────────────────────────────────────────────────

/// Hop cycle with squash-and-stretch and a REST period on the ground.
/// Uses max(sin, 0) so the sprite spends half the cycle on the ground (resting)
/// and half in the air (hopping). Returns (y_offset, x_scale, y_scale).
fn hop_frame(phase_angle: f32, height: f32, squish: f32) -> (f32, f32, f32) {
    let raw = phase_angle.sin();
    let airborne = raw.max(0.0); // 0 when resting on ground, 0..1 when hopping

    let y = airborne * height;

    // Squash only near takeoff/landing (when airborne is small but nonzero,
    // or just after landing). Use a sharp falloff so it's a brief squish, not constant.
    let near_ground = if airborne > 0.0 && airborne < 0.3 {
        (0.3 - airborne) / 0.3 // 1.0 at ground, 0.0 at airborne=0.3
    } else if raw < 0.0 && raw > -0.3 {
        // Just landed — brief squish during early rest phase
        (0.3 + raw) / 0.3 // 1.0 at raw=0, 0.0 at raw=-0.3
    } else {
        0.0
    };

    let sq = near_ground * near_ground * squish;
    let x_scale = 1.0 + sq * 0.6;
    let y_scale = 1.0 - sq;

    (y, x_scale, y_scale)
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

        let phase = body.phase;
        let is_moving = speed > 0.5;

        let (y_offset, x_scale, y_scale) = if is_moving {
            // freq=5.0 with max(sin,0) → ~0.8 hops/sec, with rest between hops
            hop_frame(t * 5.0 + phase, 3.0, 0.25)
        } else {
            // Idle: completely still, no animation
            (0.0, 1.0, 1.0)
        };

        // Apply to the SpriteBody transform (not root)
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
    fn hop_at_peak_is_airborne() {
        // PI/2: sin = 1.0, fully airborne
        let (y, sx, sy) = hop_frame(std::f32::consts::FRAC_PI_2, 4.0, 0.4);
        assert!((y - 4.0).abs() < 0.01, "should be at peak, got y={y}");
        assert!((sx - 1.0).abs() < 0.01, "no squash at peak, got sx={sx}");
        assert!((sy - 1.0).abs() < 0.01, "no squash at peak, got sy={sy}");
    }

    #[test]
    fn hop_during_rest_is_on_ground() {
        // 3*PI/2: sin = -1.0, resting on ground
        let (y, sx, sy) = hop_frame(3.0 * std::f32::consts::FRAC_PI_2, 4.0, 0.4);
        assert!(y.abs() < 0.01, "should be on ground during rest, got y={y}");
        assert!(
            (sx - 1.0).abs() < 0.01,
            "no squash during rest, got sx={sx}"
        );
        assert!(
            (sy - 1.0).abs() < 0.01,
            "no squash during rest, got sy={sy}"
        );
    }

    #[test]
    fn hop_near_landing_squishes() {
        // Just after landing: sin is small and positive (e.g. 0.15)
        // phase_angle where sin ≈ 0.15 → asin(0.15) ≈ 0.15
        let (y, sx, sy) = hop_frame(0.15, 4.0, 0.4);
        assert!(y > 0.0, "should be slightly off ground");
        assert!(sx > 1.0, "should be wider near landing, got sx={sx}");
        assert!(sy < 1.0, "should be shorter near landing, got sy={sy}");
    }

    #[test]
    fn hop_height_is_always_non_negative() {
        for i in 0..100 {
            let angle = i as f32 * 0.1;
            let (y, _, _) = hop_frame(angle, 4.0, 0.4);
            assert!(y >= 0.0, "hop should never go below ground, got y={y}");
        }
    }
}
