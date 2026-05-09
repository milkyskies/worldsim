//! Cute pixel outline shared by silhouettes, plants, world objects, and
//! tile edges. One universal dark color so the world reads as a single
//! cohesive style instead of each thing carrying its own slightly-tinted
//! rim. Achieved via a sibling sprite slightly larger than the target,
//! placed just behind it on z — no shader or post-process pass.

use bevy::prelude::*;

/// Outline rim width per side, in world pixels.
pub const OUTLINE_WIDTH: f32 = 1.0;

/// Z offset relative to the source sprite. Places the outline behind it.
pub const OUTLINE_Z_OFFSET: f32 = -0.001;

/// The single outline color used everywhere. Matches palette `FurBlack`
/// (0.08, 0.08, 0.08); duplicated as a const here so callers without a
/// `Palette` handle (e.g. inside `setup_map`) can render it identically
/// to silhouette/object outlines.
pub const OUTLINE_COLOR: Color = Color::srgb(0.08, 0.08, 0.08);

pub fn outline_color() -> Color {
    OUTLINE_COLOR
}

/// Components for an outline sibling rendered at `(offset, z + OUTLINE_Z_OFFSET)`.
/// Caller spawns this alongside the main sprite under the same parent.
pub fn outline_bundle(size: Vec2, offset: Vec2, z: f32) -> (Sprite, Transform) {
    (
        Sprite {
            color: OUTLINE_COLOR,
            custom_size: Some(size + Vec2::splat(OUTLINE_WIDTH * 2.0)),
            ..default()
        },
        Transform::from_translation(Vec3::new(offset.x, offset.y, z + OUTLINE_Z_OFFSET)),
    )
}
