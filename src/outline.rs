//! Cute pixel outline shared by silhouettes, plants, and world objects.
//! Spawn a sibling sprite slightly larger and darker than the target,
//! placed just behind it on z. The target reads as outlined without any
//! shader or post-process pass.
//!
//! Tile boundaries already get per-side darker bands in `world::map`,
//! which is the same idea applied to a grid; this module covers everything
//! else (sprites that float on top of the grid).

use bevy::prelude::*;

/// Outline rim width per side, in world pixels.
pub const OUTLINE_WIDTH: f32 = 1.0;

/// Z offset relative to the source sprite. Places the outline behind it.
pub const OUTLINE_Z_OFFSET: f32 = -0.001;

/// Multiplier applied to the source sprite's RGB channels to produce a
/// darker outline color. Alpha is preserved so transparent things stay
/// transparent.
pub const OUTLINE_DARKEN: f32 = 0.5;

pub fn outline_color(base: Color) -> Color {
    let s = base.to_srgba();
    Color::srgba(
        s.red * OUTLINE_DARKEN,
        s.green * OUTLINE_DARKEN,
        s.blue * OUTLINE_DARKEN,
        s.alpha,
    )
}

/// Components for an outline sibling rendered at `(offset, z + OUTLINE_Z_OFFSET)`.
/// Caller spawns this alongside the main sprite under the same parent.
pub fn outline_bundle(color: Color, size: Vec2, offset: Vec2, z: f32) -> (Sprite, Transform) {
    (
        Sprite {
            color: outline_color(color),
            custom_size: Some(size + Vec2::splat(OUTLINE_WIDTH * 2.0)),
            ..default()
        },
        Transform::from_translation(Vec3::new(offset.x, offset.y, z + OUTLINE_Z_OFFSET)),
    )
}
