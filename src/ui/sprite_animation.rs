//! Procedural sprite animation: hop-based movement with squash-and-stretch.
//!
//! The root entity moves smoothly (tick-based). This system makes the SpriteBody
//! visually "stay put" between hops, then catch up to the root during the hop.
//! The result: sprites hop from point to point instead of sliding.
//!
//! Reads: Transform (root position), Time
//! Writes: Transform on SpriteBody entities (visual offsets only)
//! Upstream: movement systems (root Transform changes)
//! Downstream: purely visual — no simulation systems read these offsets

use bevy::prelude::*;
use bevy::transform::TransformSystems;

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
    /// Where the sprite visually "landed" last. Between hops, the sprite
    /// stays here even though the root keeps moving.
    landed_pos: Option<Vec2>,
}

impl SpriteBody {
    pub fn new(root: Entity, phase: f32) -> Self {
        Self {
            phase,
            root,
            landed_pos: None,
        }
    }
}

// ── Hop math ────────────────────────────────────────────────────────────────

/// One hop cycle. `cycle` goes from 0.0 to 1.0.
///   0.00 - 0.10  squish down (crouch before jump)
///   0.10 - 0.50  airborne (parabolic arc, sprite moves from landed_pos to root)
///   0.50 - 0.60  land squish
///   0.60 - 1.00  rest on ground
///
/// Returns (y_offset, x_scale, y_scale, horizontal_progress).
/// horizontal_progress is 0.0 at start, 1.0 when caught up to root.
fn hop_frame(cycle: f32, height: f32) -> (f32, f32, f32, f32) {
    if cycle < 0.1 {
        // Pre-jump crouch
        let t = cycle / 0.1;
        let squish = t * 0.15;
        (0.0, 1.0 + squish, 1.0 - squish, 0.0)
    } else if cycle < 0.5 {
        // Airborne: parabolic arc + move horizontally toward root
        let t = (cycle - 0.1) / 0.4; // 0 to 1
        let y = 4.0 * t * (1.0 - t) * height;
        let stretch = if t < 0.3 {
            t / 0.3 * 0.08
        } else if t > 0.7 {
            (1.0 - t) / 0.3 * 0.08
        } else {
            0.08
        };
        // Smooth horizontal progress (ease in-out)
        let progress = t * t * (3.0 - 2.0 * t);
        (y, 1.0 - stretch * 0.5, 1.0 + stretch, progress)
    } else if cycle < 0.6 {
        // Landing squish (already at root position)
        let t = (cycle - 0.5) / 0.1;
        let squish = (1.0 - t) * 0.15;
        (0.0, 1.0 + squish, 1.0 - squish, 1.0)
    } else {
        // Resting on ground
        (0.0, 1.0, 1.0, 1.0)
    }
}

// ── Main system ─────────────────────────────────────────────────────────────

fn animate_sprite_bodies(
    time: Res<Time>,
    mut body_query: Query<(Entity, &mut SpriteBody)>,
    mut transforms: Query<&mut Transform>,
) {
    let t = time.elapsed_secs();

    for (body_entity, mut body) in body_query.iter_mut() {
        let root_pos = transforms
            .get(body.root)
            .map(|t| t.translation.truncate())
            .unwrap_or(Vec2::ZERO);

        // Initialize landed_pos on first frame
        let landed = body.landed_pos.unwrap_or(root_pos);

        // Is the root ahead of where we landed? (i.e. agent is moving)
        let drift = root_pos.distance(landed);
        let is_moving = drift > 1.0;

        let hops_per_sec = 2.0;
        let cycle = ((t * hops_per_sec + body.phase) % 1.0).clamp(0.0, 1.0);

        let (y_offset, x_scale, y_scale, progress) = if is_moving {
            hop_frame(cycle, 3.0)
        } else {
            // Idle: stay put
            body.landed_pos = Some(root_pos);
            (0.0, 1.0, 1.0, 1.0)
        };

        // Interpolate visual position between landed_pos and root_pos
        let visual_pos = landed.lerp(root_pos, progress);
        // Offset from root (since SpriteBody is a child of root, offset is relative)
        let offset = visual_pos - root_pos;

        if let Ok(mut body_transform) = transforms.get_mut(body_entity) {
            body_transform.translation.x = offset.x;
            body_transform.translation.y = offset.y + y_offset;
            body_transform.scale = Vec3::new(x_scale, y_scale, 1.0);
        }

        // When hop completes (progress=1.0 and in rest/landing phase), update landed_pos
        if progress >= 1.0 {
            body.landed_pos = Some(root_pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crouch_phase_squishes_and_stays_put() {
        let (y, sx, sy, progress) = hop_frame(0.05, 3.0);
        assert!(y.abs() < 0.01, "on ground during crouch");
        assert!(sx > 1.0, "wider during crouch");
        assert!(sy < 1.0, "shorter during crouch");
        assert!(progress.abs() < 0.01, "hasn't moved yet");
    }

    #[test]
    fn airborne_phase_reaches_peak_and_moves() {
        let (y, _, _, progress) = hop_frame(0.3, 3.0);
        assert!((y - 3.0).abs() < 0.01, "at peak, got y={y}");
        assert!(progress > 0.3 && progress < 0.7, "mid-flight progress");
    }

    #[test]
    fn landing_phase_is_at_destination() {
        let (y, sx, sy, progress) = hop_frame(0.51, 3.0);
        assert!(y.abs() < 0.01, "on ground at landing");
        assert!(sx > 1.0, "squished on landing");
        assert!(sy < 1.0, "squished on landing");
        assert!((progress - 1.0).abs() < 0.01, "arrived at destination");
    }

    #[test]
    fn rest_phase_is_still_at_destination() {
        for i in 6..10 {
            let cycle = i as f32 / 10.0;
            let (y, sx, sy, progress) = hop_frame(cycle, 3.0);
            assert!(y.abs() < 0.01, "on ground");
            assert!((sx - 1.0).abs() < 0.01, "no scale change");
            assert!((sy - 1.0).abs() < 0.01, "no scale change");
            assert!((progress - 1.0).abs() < 0.01, "at destination");
        }
    }

    #[test]
    fn hop_never_goes_below_ground() {
        for i in 0..100 {
            let cycle = i as f32 / 100.0;
            let (y, _, _, _) = hop_frame(cycle, 3.0);
            assert!(y >= 0.0, "y={y} at cycle={cycle}");
        }
    }

    #[test]
    fn progress_is_monotonic() {
        let mut prev = 0.0_f32;
        for i in 0..100 {
            let cycle = i as f32 / 100.0;
            let (_, _, _, progress) = hop_frame(cycle, 3.0);
            assert!(
                progress >= prev - 0.001,
                "progress should never decrease: {prev} -> {progress} at cycle={cycle}"
            );
            prev = progress;
        }
    }
}
