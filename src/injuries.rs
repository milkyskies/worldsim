//! Render injury overlays onto silhouette parts whose `body_node` matches an
//! anatomical node carrying unhealed injuries.
//!
//! Reads: Body, SpriteBody, SilhouettePartLink, Palette
//! Writes: Sprite (color tint) on body-part sprite children
//! Upstream: silhouette renderer attaches `SilhouettePartLink { body_node, base_color, .. }`
//! Downstream: visual only - no simulation state effect
//!
//! Currently a single-channel overlay: severity 0..N is interpolated toward
//! `BloodFresh` so a hurt limb visibly reddens. Severe injuries push toward
//! `BloodDried`. Idempotent per frame: when severity drops to zero, the
//! sprite snaps back to `link.base_color`. Future passes can layer fracture
//! rotation, scar lines, infection green pulse, etc.

use bevy::prelude::*;

use crate::agent::biology::body::Body;
use crate::palette::{Palette, PaletteColor};
use crate::silhouette::SilhouettePartLink;
use crate::ui::sprite_animation::SpriteBody;

pub struct InjuriesPlugin;

impl Plugin for InjuriesPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::palette::PalettePlugin>() {
            app.add_plugins(crate::palette::PalettePlugin);
        }
        app.add_systems(Update, apply_injury_overlays);
    }
}

fn apply_injury_overlays(
    palette: Res<Palette>,
    bodies: Query<&Body>,
    sprite_bodies: Query<&SpriteBody>,
    mut parts: Query<(&mut Sprite, &SilhouettePartLink, &ChildOf)>,
) {
    let blood_fresh = palette.srgb(PaletteColor::BloodFresh);
    let blood_dried = palette.srgb(PaletteColor::BloodDried);
    for (mut sprite, link, child_of) in parts.iter_mut() {
        let Some(node_kind) = link.body_node else {
            continue;
        };
        let Ok(body_marker) = sprite_bodies.get(child_of.parent()) else {
            continue;
        };
        let Ok(body) = bodies.get(body_marker.root) else {
            continue;
        };
        let Some(node) = body.node(node_kind) else {
            continue;
        };
        let severity: f32 = node
            .injuries
            .iter()
            .map(|i| i.severity * (1.0 - i.healed_amount))
            .sum::<f32>()
            .min(1.0);
        if severity < 0.01 {
            // Healthy parts are owned by `apply_sprite_lighting` (day/night
            // tinting). Writing here would race with that system and flicker
            // every frame — let the lighting system be authoritative.
            continue;
        }
        sprite.color = if severity < 0.5 {
            lerp_srgb(link.base_color, blood_fresh, severity * 1.2)
        } else {
            lerp_srgb(link.base_color, blood_dried, (severity - 0.3).min(0.7))
        };
    }
}

fn lerp_srgb(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let sa = a.to_srgba();
    let sb = b.to_srgba();
    Color::srgba(
        sa.red + (sb.red - sa.red) * t,
        sa.green + (sb.green - sa.green) * t,
        sa.blue + (sb.blue - sa.blue) * t,
        sa.alpha + (sb.alpha - sa.alpha) * t,
    )
}
