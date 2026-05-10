//! Procedural sprite animation: bouncy arcs + squash-and-stretch.
//!
//! Reads: Transform, Time, WorldMap
//! Writes: Transform (SpriteBody, y + scale), VisualOffset (root)
//! Upstream: movement systems
//! Downstream: UI click hit-testing and selection gizmos (read VisualOffset)

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use std::collections::HashMap;

use crate::palette::Palette;
use crate::particles::spawn_dust_puff;
use crate::world::map::{ELEVATION_LIFT, SEA_LEVEL, TILE_SIZE, WorldMap};

pub struct SpriteAnimationPlugin;

impl Plugin for SpriteAnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            animate_sprite_bodies.before(TransformSystems::Propagate),
        );
    }
}

/// Additive visual-only offset from an entity's logical position (root
/// `Transform`) to where it is drawn on screen. Captures terrain elevation
/// lift, bounce arcs, and any other view-layer effects in a single vector.
///
/// Game logic reads `Transform`; the view layer reads `Transform + VisualOffset`.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Reflect)]
#[reflect(Component)]
pub struct VisualOffset(pub Vec2);

impl VisualOffset {
    /// Apply an optional `VisualOffset` to a logical position, returning
    /// the drawn position. `None` is treated as the zero offset.
    pub fn apply(offset: Option<&Self>, logical: Vec2) -> Vec2 {
        logical + offset.map_or(Vec2::ZERO, |v| v.0)
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

/// Marker on a shadow sprite entity. Follows the root's terrain elevation
/// so the shadow always sits on the ground — no bounce, unlike SpriteBody.
/// `base_offset` is the local position of the shadow at sea level
/// (typically negative y for feet, positive x to push away from a NW sun).
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct GroundShadow {
    pub root: Entity,
    pub base_offset: Vec2,
}

impl GroundShadow {
    pub fn new(root: Entity, base_offset: Vec2) -> Self {
        Self { root, base_offset }
    }
}

/// Marker on the floating name-tag text entity. Sits above the silhouette
/// and tracks terrain elevation but not the hop, same contract as
/// [`GroundShadow`]. `base_offset_y` is the y position above the root at
/// sea level (callers derive it from `CreatureSilhouette::top_y`).
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct NameTag {
    pub root: Entity,
    pub base_offset_y: f32,
}

impl NameTag {
    pub fn new(root: Entity, base_offset_y: f32) -> Self {
        Self {
            root,
            base_offset_y,
        }
    }
}

/// Whole-body animation pose picked from the agent's current activity. The
/// hop is the default; sleeping creatures slump; everything else gets a tiny
/// breath-cycle idle. Per-emotion intensity (fear, joy) further modulates
/// the active pose via [`PoseModulators`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnimationPose {
    Hop,
    Sleeping,
    Idle,
}

#[derive(Clone, Copy, Debug)]
struct PoseModulators {
    hop_frequency: f32,
    hop_amplitude: f32,
}

impl Default for PoseModulators {
    fn default() -> Self {
        Self {
            hop_frequency: 1.0,
            hop_amplitude: 1.0,
        }
    }
}

fn pick_pose(
    state: Option<(
        &crate::agent::actions::registry::ActiveActions,
        &crate::agent::psyche::emotions::EmotionalState,
    )>,
    is_moving: bool,
) -> AnimationPose {
    use crate::agent::actions::types::ActionType;
    if let Some((active, _)) = state
        && active.contains(ActionType::Sleep)
    {
        return AnimationPose::Sleeping;
    }
    if is_moving {
        AnimationPose::Hop
    } else {
        AnimationPose::Idle
    }
}

fn pose_modulators(
    state: Option<(
        &crate::agent::actions::registry::ActiveActions,
        &crate::agent::psyche::emotions::EmotionalState,
    )>,
) -> PoseModulators {
    use crate::agent::psyche::emotions::EmotionType;
    let mut m = PoseModulators::default();
    let Some((_, emotions)) = state else {
        return m;
    };
    let fear = emotions
        .active_emotions
        .iter()
        .find(|e| e.emotion_type == EmotionType::Fear)
        .map(|e| e.intensity)
        .unwrap_or(0.0);
    let joy = emotions
        .active_emotions
        .iter()
        .find(|e| e.emotion_type == EmotionType::Joy)
        .map(|e| e.intensity)
        .unwrap_or(0.0);
    if fear > 0.6 {
        m.hop_frequency *= 1.5;
        m.hop_amplitude *= 0.7;
    }
    if joy > 0.5 {
        m.hop_amplitude *= 1.15;
    }
    m
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
struct MoveTracker {
    prev_pos: Option<Vec2>,
    /// Last wall-clock time the root position changed.
    last_moved_at: f32,
    /// +1 facing right (default), -1 facing left. Updated whenever the
    /// agent's horizontal movement crosses a small deadband, sticky
    /// otherwise so a stationary creature keeps the direction it last
    /// faced instead of snapping back to the default.
    facing: f32,
    /// Last hop cycle position, used to detect the wrap from ~1.0 → 0.0
    /// that marks a landing — that's when dust puffs.
    prev_cycle: f32,
}

impl Default for MoveTracker {
    fn default() -> Self {
        Self {
            prev_pos: None,
            last_moved_at: 0.0,
            facing: 1.0,
            prev_cycle: 0.0,
        }
    }
}

fn animate_sprite_bodies(
    mut commands: Commands,
    time: Res<Time>,
    world_map: Option<Res<WorldMap>>,
    palette: Option<Res<Palette>>,
    body_query: Query<(Entity, &SpriteBody)>,
    shadow_query: Query<(Entity, &GroundShadow)>,
    name_tag_query: Query<(Entity, &NameTag)>,
    agent_state: Query<(
        &crate::agent::actions::registry::ActiveActions,
        &crate::agent::psyche::emotions::EmotionalState,
    )>,
    mut transforms: Query<&mut Transform>,
    mut visual_offsets: Query<&mut VisualOffset>,
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
        let dx = root_pos.x - prev.x;
        if dx.abs() > 0.05 {
            tracker.facing = if dx >= 0.0 { 1.0 } else { -1.0 };
        }
        tracker.prev_pos = Some(root_pos);
        let facing = tracker.facing;

        // Consider "moving" if position changed within the last 0.2 seconds
        let is_moving = (t - tracker.last_moved_at) < 0.2;

        let pose = pick_pose(agent_state.get(body.root).ok(), is_moving);
        let modulators = pose_modulators(agent_state.get(body.root).ok());

        let (bounce_y, x_scale, y_scale) = match pose {
            AnimationPose::Sleeping => (0.0, 1.15, 0.65),
            AnimationPose::Hop => {
                let bounces_per_sec = 2.5 * modulators.hop_frequency;
                let cycle = ((t * bounces_per_sec + body.phase) % 1.0).clamp(0.0, 1.0);
                let (y, sx, sy) = bounce_frame(cycle, 3.0 * modulators.hop_amplitude);
                let tracker = trackers.get_mut(&body.root).expect("inserted above");
                if let Some(palette) = palette.as_deref()
                    && tracker.prev_cycle > 0.7
                    && cycle < 0.3
                {
                    let foot = Vec2::new(root_pos.x, root_pos.y - 6.0);
                    let seed = (body.root.to_bits() ^ ((t * 1000.0) as u64)).wrapping_mul(0x9E37);
                    spawn_dust_puff(&mut commands, palette, foot, seed);
                }
                tracker.prev_cycle = cycle;
                (y, sx, sy)
            }
            AnimationPose::Idle => {
                // Subtle breath cycle: ±2% y-scale.
                let phase = (t * 0.7 + body.phase) % std::f32::consts::TAU;
                let breath = 1.0 + 0.02 * phase.sin();
                (0.0, 1.0, breath)
            }
        };

        // Lift the sprite body by the underlying tile's elevation so the
        // agent sits on top of the visual terrain relief instead of at the
        // flat grid position.
        let elevation_lift = elevation_lift_at(world_map.as_deref(), root_pos);
        let total_y_offset = bounce_y + elevation_lift;

        if let Ok(mut bt) = transforms.get_mut(body_entity) {
            bt.translation.y = total_y_offset;
            bt.scale = Vec3::new(x_scale * facing, y_scale, 1.0);
        }

        if let Ok(mut offset) = visual_offsets.get_mut(body.root) {
            offset.set_if_neq(VisualOffset(Vec2::new(0.0, total_y_offset)));
        }
    }

    // Shadows track the terrain surface — elevation lift only, no bounce —
    // so sprites visibly hop above their shadow.
    for (shadow_entity, shadow) in shadow_query.iter() {
        let root_pos = transforms
            .get(shadow.root)
            .map(|tr| tr.translation.truncate())
            .unwrap_or(Vec2::ZERO);
        let elevation_lift = elevation_lift_at(world_map.as_deref(), root_pos);
        if let Ok(mut st) = transforms.get_mut(shadow_entity) {
            st.translation.x = shadow.base_offset.x;
            st.translation.y = shadow.base_offset.y + elevation_lift;
        }
    }

    // Name tags follow elevation but not the hop, so they sit a fixed
    // distance above the silhouette regardless of terrain height.
    for (tag_entity, tag) in name_tag_query.iter() {
        let root_pos = transforms
            .get(tag.root)
            .map(|tr| tr.translation.truncate())
            .unwrap_or(Vec2::ZERO);
        let elevation_lift = elevation_lift_at(world_map.as_deref(), root_pos);
        if let Ok(mut tt) = transforms.get_mut(tag_entity) {
            tt.translation.y = tag.base_offset_y + elevation_lift;
        }
    }

    if trackers.len() > alive.len() {
        trackers.retain(|e, _| alive.contains(e));
    }
}

/// Convert a world position to a vertical lift in screen pixels, matching
/// how terrain tiles are lifted in `setup_map`.
///
/// Bilinearly interpolates between the four tile-center elevations
/// surrounding `pos` so an agent walking across a tile boundary glides
/// smoothly between elevations instead of teleporting at the seam. The
/// terrain tiles themselves still render at discrete per-tile heights —
/// this only affects entity sprites that read this lift.
fn elevation_lift_at(world_map: Option<&WorldMap>, pos: Vec2) -> f32 {
    let Some(map) = world_map else {
        return 0.0;
    };
    sample_smoothed_elevation(map, pos)
        .map(|e| (e - SEA_LEVEL) * ELEVATION_LIFT)
        .unwrap_or(0.0)
}

/// Bilinear sample of the elevation field at a sub-tile world position.
/// Tile centers sit at `(tx*TILE_SIZE + TILE_SIZE/2, …)` — shifting
/// `pos` by half a tile puts the integer grid exactly on tile centers,
/// so the floor + fractional split works directly. Returns `None` only
/// when *every* surrounding tile is out of bounds (interior holes get
/// treated as elevation 0).
fn sample_smoothed_elevation(map: &WorldMap, pos: Vec2) -> Option<f32> {
    let half = TILE_SIZE * 0.5;
    let gx = (pos.x - half) / TILE_SIZE;
    let gy = (pos.y - half) / TILE_SIZE;
    let x0 = gx.floor();
    let y0 = gy.floor();
    let fx = (gx - x0).clamp(0.0, 1.0);
    let fy = (gy - y0).clamp(0.0, 1.0);
    let sample = |x: f32, y: f32| -> Option<f32> {
        if x < 0.0 || y < 0.0 {
            return None;
        }
        map.elevation_at(x as u32, y as u32)
    };
    let e00 = sample(x0, y0);
    let e10 = sample(x0 + 1.0, y0);
    let e01 = sample(x0, y0 + 1.0);
    let e11 = sample(x0 + 1.0, y0 + 1.0);
    if e00.is_none() && e10.is_none() && e01.is_none() && e11.is_none() {
        return None;
    }
    // For any single missing corner, fall back to the average of the
    // present ones — better than jumping to 0 on the map edge.
    let fallback = {
        let present: Vec<f32> = [e00, e10, e01, e11].into_iter().flatten().collect();
        present.iter().sum::<f32>() / present.len() as f32
    };
    let e00 = e00.unwrap_or(fallback);
    let e10 = e10.unwrap_or(fallback);
    let e01 = e01.unwrap_or(fallback);
    let e11 = e11.unwrap_or(fallback);
    let elev = (1.0 - fx) * (1.0 - fy) * e00
        + fx * (1.0 - fy) * e10
        + (1.0 - fx) * fy * e01
        + fx * fy * e11;
    Some(elev)
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

    /// 2×2 map with a fixed elevation gradient: tile (0,0)=10, (1,0)=20,
    /// (0,1)=30, (1,1)=40. The smoothed sampler should:
    /// - return each tile's exact elevation when sampled at its center
    /// - return the bilinear interpolant midway between centers
    /// - never produce a step at the tile boundary
    fn gradient_map() -> WorldMap {
        use crate::world::map::Chunk;
        let mut map = WorldMap::new(2, 2);
        let mut chunk = Chunk::new(0, 0);
        chunk.set_elevation(0, 0, 10.0);
        chunk.set_elevation(1, 0, 20.0);
        chunk.set_elevation(0, 1, 30.0);
        chunk.set_elevation(1, 1, 40.0);
        map.chunks.insert(IVec2::new(0, 0), chunk);
        map
    }

    #[test]
    fn smoothed_elevation_matches_tile_center() {
        let map = gradient_map();
        let center_00 = Vec2::new(TILE_SIZE * 0.5, TILE_SIZE * 0.5);
        let elev = sample_smoothed_elevation(&map, center_00).unwrap();
        assert!((elev - 10.0).abs() < 1e-3, "got {elev} at (0,0) center");
    }

    #[test]
    fn smoothed_elevation_interpolates_between_centers() {
        let map = gradient_map();
        // Halfway between (0,0) center and (1,0) center: pure x-interp,
        // expect (10 + 20) / 2 = 15.
        let between = Vec2::new(TILE_SIZE, TILE_SIZE * 0.5);
        let elev = sample_smoothed_elevation(&map, between).unwrap();
        assert!(
            (elev - 15.0).abs() < 1e-3,
            "got {elev} between (0,0) and (1,0) centers"
        );
    }

    /// The original per-tile sampler had a discontinuity at the tile
    /// boundary — sampling at center-1 and center returned different
    /// elevations from neighboring tiles. The bilinear sampler must
    /// produce values that change continuously as `pos` slides across
    /// the seam.
    #[test]
    fn smoothed_elevation_has_no_seam_at_tile_boundary() {
        let map = gradient_map();
        let just_before = Vec2::new(TILE_SIZE - 0.01, TILE_SIZE * 0.5);
        let just_after = Vec2::new(TILE_SIZE + 0.01, TILE_SIZE * 0.5);
        let a = sample_smoothed_elevation(&map, just_before).unwrap();
        let b = sample_smoothed_elevation(&map, just_after).unwrap();
        assert!(
            (a - b).abs() < 0.1,
            "smooth sampler produced a seam: {a} vs {b}"
        );
    }

    #[test]
    fn bounce_is_symmetric() {
        let (y1, _, _) = bounce_frame(0.3, 3.0);
        let (y2, _, _) = bounce_frame(0.7, 3.0);
        assert!((y1 - y2).abs() < 0.01, "should be symmetric: {y1} vs {y2}");
    }

    #[test]
    fn visual_offset_apply_lifts_logical_position() {
        let offset = VisualOffset(Vec2::new(0.0, 10.0));
        let logical = Vec2::new(50.0, 20.0);
        assert_eq!(
            VisualOffset::apply(Some(&offset), logical),
            Vec2::new(50.0, 30.0)
        );
    }

    #[test]
    fn visual_offset_apply_none_is_identity() {
        let logical = Vec2::new(5.0, 7.0);
        assert_eq!(VisualOffset::apply(None, logical), logical);
    }

    #[test]
    fn visual_offset_apply_none_matches_zero_offset() {
        let logical = Vec2::new(3.0, 4.0);
        assert_eq!(
            VisualOffset::apply(None, logical),
            VisualOffset::apply(Some(&VisualOffset(Vec2::ZERO)), logical)
        );
    }
}
