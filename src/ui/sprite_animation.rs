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

/// Full hop cycle with squash-and-stretch. Takes a phase angle (0 to PI = one full hop).
/// Returns (y_offset, x_scale, y_scale).
///
/// The cycle:
///   0.0       → on ground, squishing down (crouch)
///   0.0-0.3   → launch (stretching tall)
///   0.3-0.7   → airborne (stretched, peak at 0.5)
///   0.7-1.0   → landing (squishing wide)
///   1.0       → on ground, squishing down (next crouch)
fn hop_frame(phase_angle: f32, hop_height: f32, squish: f32) -> (f32, f32, f32) {
    let t = phase_angle.sin().abs(); // 0 at ground, 1 at peak

    let y = t * hop_height;

    // Squash-and-stretch: volume-preserving-ish
    // At ground (t=0): wide + short (squash)
    // At peak (t=1): narrow + tall (stretch)
    let ground = 1.0 - t; // 1 at ground, 0 at peak
    let squash = ground * ground * squish; // strong squash near ground, none at peak

    let x_scale = 1.0 + squash * 0.6; // wider on ground
    let y_scale = 1.0 - squash; // shorter on ground

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
            // ~1.2 hops per second — slow, deliberate, cute
            let hop_freq = 4.0;
            let height = 3.0;
            hop_frame(t * hop_freq + phase, height, 0.25)
        } else {
            // Idle: barely-there breathing squish
            let breath = (t * 0.8 + phase).sin();
            let squish = breath.abs() * 0.03;
            (0.0, 1.0 + squish * 0.3, 1.0 - squish)
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
    fn hop_at_ground_squishes_wide_and_short() {
        // phase_angle = 0 means sin = 0, so t = 0 (ground)
        let (y, sx, sy) = hop_frame(0.0, 4.0, 0.4);
        assert!(y.abs() < 0.01, "should be on ground, got y={y}");
        assert!(sx > 1.0, "should be wider on ground, got sx={sx}");
        assert!(sy < 1.0, "should be shorter on ground, got sy={sy}");
    }

    #[test]
    fn hop_at_peak_stretches_tall() {
        // phase_angle = PI/2 means sin = 1, so t = 1 (peak)
        let (y, sx, sy) = hop_frame(std::f32::consts::FRAC_PI_2, 4.0, 0.4);
        assert!(
            (y - 4.0).abs() < 0.01,
            "should be at peak height, got y={y}"
        );
        assert!((sx - 1.0).abs() < 0.01, "no stretch at peak, got sx={sx}");
        assert!((sy - 1.0).abs() < 0.01, "no squash at peak, got sy={sy}");
    }

    #[test]
    fn hop_height_is_always_non_negative() {
        for i in 0..100 {
            let angle = i as f32 * 0.1;
            let (y, _, _) = hop_frame(angle, 4.0, 0.4);
            assert!(y >= 0.0, "hop should never go below ground, got y={y}");
        }
    }

    #[test]
    fn squash_and_stretch_preserves_approximate_volume() {
        // At any point, x_scale * y_scale should be roughly ~1.0
        for i in 0..100 {
            let angle = i as f32 * 0.1;
            let (_, sx, sy) = hop_frame(angle, 4.0, 0.4);
            let volume = sx * sy;
            assert!(
                volume > 0.7 && volume < 1.3,
                "volume should be roughly preserved, got {volume} at angle={angle}"
            );
        }
    }
}
