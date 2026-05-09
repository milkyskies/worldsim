//! Short-lived sprite particles - dust puffs on hop landings, blood
//! splatter on damage, etc. Decoupled from any specific trigger; the
//! `Particle` component + `tick_particles` system handle lifetime/fade,
//! and individual triggers (in animate_sprite_bodies, biology, etc.)
//! call the `spawn_*` helpers when they fire.
//!
//! Reads: Time, Particle (per-tick decay)
//! Writes: Particle (lifetime), Sprite (alpha fade), Transform (velocity)
//! Upstream: any system that calls `spawn_dust_puff` / `spawn_damage_flash`
//! Downstream: visual only - no simulation effect

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::agent::biology::body::{Body, BodyNode};
use crate::palette::{Palette, PaletteColor};

pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (tick_particles, flash_on_damage));
    }
}

#[derive(Component, Clone, Debug)]
pub struct Particle {
    /// Per-frame world-pixel velocity.
    pub velocity: Vec2,
    /// Seconds until despawn.
    pub remaining: f32,
    /// Initial lifetime, used to compute fade alpha (remaining / initial).
    pub initial: f32,
    /// Untinted base color; sprite color is `base * alpha` each frame.
    pub base: Color,
}

fn tick_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p, mut tr, mut sprite) in particles.iter_mut() {
        p.remaining -= dt;
        if p.remaining <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        tr.translation.x += p.velocity.x * dt;
        tr.translation.y += p.velocity.y * dt;
        // Apply gravity to vertical velocity so dust/blood arcs fall.
        p.velocity.y -= 30.0 * dt;
        let alpha = (p.remaining / p.initial).clamp(0.0, 1.0);
        sprite.color = with_alpha(p.base, alpha);
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    let s = c.to_srgba();
    Color::srgba(s.red, s.green, s.blue, a)
}

/// Spawn a small dust puff at the given world position. Used by hop
/// landings - 3 grey ellipses scattering up-and-out.
pub fn spawn_dust_puff(commands: &mut Commands, palette: &Palette, world_pos: Vec2, seed: u64) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let dust = palette.srgb(PaletteColor::FurLightGrey);
    for _ in 0..3 {
        let dx: f32 = rng.random_range(-2.5..2.5);
        let vx: f32 = rng.random_range(-12.0..12.0);
        let vy: f32 = rng.random_range(8.0..18.0);
        commands.spawn((
            Particle {
                velocity: Vec2::new(vx, vy),
                remaining: 0.4,
                initial: 0.4,
                base: dust,
            },
            Sprite {
                color: dust,
                custom_size: Some(Vec2::new(2.0, 1.5)),
                ..default()
            },
            Transform::from_translation(Vec3::new(world_pos.x + dx, world_pos.y, 10.0)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));
    }
}

fn tree_severity(node: &BodyNode) -> f32 {
    let here: f32 = node
        .injuries
        .iter()
        .map(|i| i.severity * (1.0 - i.healed_amount))
        .sum();
    here + node.children.iter().map(tree_severity).sum::<f32>()
}

/// Per-entity severity from the previous frame. When a body's total
/// unhealed severity rises significantly, we spawn a flash. We deliberately
/// ignore *decreases* (healing) so recovery is silent.
fn flash_on_damage(
    mut commands: Commands,
    palette: Option<Res<Palette>>,
    bodies: Query<(Entity, &Body, &Transform)>,
    mut prev: Local<HashMap<Entity, f32>>,
) {
    let Some(palette) = palette.as_deref() else {
        return;
    };
    let mut alive = Vec::with_capacity(bodies.iter().len());
    for (entity, body, tr) in bodies.iter() {
        alive.push(entity);
        let total: f32 = body.parts.iter().map(tree_severity).sum();
        let last = prev.get(&entity).copied().unwrap_or(total);
        if total - last > 0.05 {
            let world_pos = tr.translation.truncate();
            let seed = entity.to_bits().wrapping_mul(0x9E37) ^ total.to_bits() as u64;
            spawn_damage_flash(&mut commands, palette, world_pos, seed);
        }
        prev.insert(entity, total);
    }
    if prev.len() > alive.len() {
        prev.retain(|e, _| alive.contains(e));
    }
}

/// Spawn a blood-splatter flash at the given world position. Used by
/// damage-taken events - 4 red dots scattering outward, fading fast.
pub fn spawn_damage_flash(commands: &mut Commands, palette: &Palette, world_pos: Vec2, seed: u64) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let blood = palette.srgb(PaletteColor::BloodFresh);
    for _ in 0..4 {
        let dx: f32 = rng.random_range(-1.5..1.5);
        let dy: f32 = rng.random_range(-1.5..1.5);
        let vx: f32 = rng.random_range(-25.0..25.0);
        let vy: f32 = rng.random_range(5.0..20.0);
        commands.spawn((
            Particle {
                velocity: Vec2::new(vx, vy),
                remaining: 0.5,
                initial: 0.5,
                base: blood,
            },
            Sprite {
                color: blood,
                custom_size: Some(Vec2::new(1.5, 1.5)),
                ..default()
            },
            Transform::from_translation(Vec3::new(world_pos.x + dx, world_pos.y + dy, 10.0)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));
    }
}
