//! World liquids: blood, water, oil, etc. as first-class tile entities.
//!
//! Reads: TickCount (for decay), Transform (for spatial queries)
//! Writes: Liquid component, liquid spawn entities
//! Upstream: combat system (spawns blood), future water/oil/vomit systems
//! Downstream: rendering (sprite), perception (agents can see liquid tiles),
//!             future forensics (agents reason about blood trails)
//!
//! # Model
//!
//! A `Liquid` is a tile-scale puddle. It carries a kind (Blood today, Water /
//! Oil / Vomit later), a magnitude that represents both visual size and
//! remaining volume, and a `created_at_tick` used for decay. Liquids drain
//! slowly on the decay system and despawn when their magnitude hits zero.
//!
//! When new liquid is spawned on a tile that already holds a liquid of the
//! *same kind*, the magnitudes merge (capped) and the `created_at_tick`
//! refreshes — a drying puddle that gets another splash becomes wet again.
//! Different kinds don't merge (blood on a water puddle is a new blood
//! entity on top of the water).

use bevy::prelude::*;

use crate::core::tick::TickCount;
use crate::world::Physical;
use crate::world::map::TILE_SIZE;

/// What kind of liquid this puddle is made of. Gates merging behaviour and
/// the rendered color / decay rate. Extend with new variants as systems
/// need them — water from spilled buckets, oil from lamps, vomit from
/// disgust reactions — without touching the Liquid component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum LiquidKind {
    Blood,
}

impl LiquidKind {
    /// Magnitude drained per game second. Blood coagulates and dries over
    /// a few minutes of game time; water would evaporate much slower.
    pub fn decay_per_sec(self) -> f32 {
        match self {
            LiquidKind::Blood => 0.25,
        }
    }

    /// Cap on a single puddle's magnitude — visually clamps the sprite
    /// size and prevents unbounded merging at a gory battle site.
    pub fn max_magnitude(self) -> f32 {
        match self {
            LiquidKind::Blood => 60.0,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            LiquidKind::Blood => "blood",
        }
    }
}

/// A puddle of liquid on a tile. The tile is inferred from the entity's
/// `Transform` — snapping to tile centre happens at spawn time so
/// multiple splashes on the same tile merge deterministically.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct Liquid {
    pub kind: LiquidKind,
    /// Remaining "volume" in abstract units. Visual size scales from this;
    /// the decay system drains it toward zero.
    pub magnitude: f32,
    /// Tick the puddle was last refreshed. Merging bumps this so a mostly
    /// dry puddle that gets a fresh splash resets its drying clock.
    pub created_at_tick: u64,
}

/// Snap a world position to the centre of its tile. Merging relies on two
/// splashes at slightly different positions landing on the same integer
/// tile coordinate so they collide in the spatial lookup.
pub fn snap_to_tile_centre(pos: Vec2) -> Vec2 {
    let tx = (pos.x / TILE_SIZE).floor();
    let ty = (pos.y / TILE_SIZE).floor();
    Vec2::new(
        tx * TILE_SIZE + TILE_SIZE * 0.5,
        ty * TILE_SIZE + TILE_SIZE * 0.5,
    )
}

/// Spawn or merge a liquid puddle at `position`.
///
/// If an existing puddle of the same kind already sits on the target
/// tile, its magnitude is bumped (capped at `max_magnitude`) and its
/// timestamp refreshes. Otherwise a fresh entity is spawned via
/// `commands`.
///
/// `existing` is the caller's mutable query over `(Entity, &Transform,
/// &mut Liquid)`. Taking it by generic bound lets combat systems pass
/// in their own query without coupling this helper to a specific one.
pub fn spawn_or_merge_liquid(
    commands: &mut Commands,
    existing: &mut Query<(Entity, &Transform, &mut Liquid)>,
    kind: LiquidKind,
    position: Vec2,
    amount: f32,
    tick: u64,
) -> Entity {
    let centre = snap_to_tile_centre(position);

    for (entity, transform, mut liquid) in existing.iter_mut() {
        if liquid.kind != kind {
            continue;
        }
        let existing_pos = transform.translation.truncate();
        if (existing_pos - centre).length_squared() < 1.0 {
            liquid.magnitude = (liquid.magnitude + amount).min(kind.max_magnitude());
            liquid.created_at_tick = tick;
            return entity;
        }
    }

    commands
        .spawn((
            Name::new(format!("{} puddle", kind.display_name())),
            Liquid {
                kind,
                magnitude: amount.min(kind.max_magnitude()),
                created_at_tick: tick,
            },
            Physical,
            Transform::from_translation(centre.extend(0.5)),
            GlobalTransform::default(),
        ))
        .id()
}

/// System: drain liquid magnitude over time and despawn empty puddles.
pub fn decay_liquids(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Liquid)>,
    tick: Res<TickCount>,
) {
    let dt = tick.dt();
    for (entity, mut liquid) in query.iter_mut() {
        let drain = liquid.kind.decay_per_sec() * dt;
        liquid.magnitude -= drain;
        if liquid.magnitude <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

pub struct LiquidPlugin;

impl Plugin for LiquidPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Liquid>()
            .add_systems(FixedUpdate, decay_liquids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_to_tile_centre_is_idempotent_per_tile() {
        // Two positions inside the same tile snap to the same centre.
        let a = snap_to_tile_centre(Vec2::new(10.0, 5.0));
        let b = snap_to_tile_centre(Vec2::new(15.9, 15.9));
        assert!(
            (a - b).length() < 0.01,
            "same-tile positions should snap identical (got {a} vs {b})"
        );

        // Crossing a tile boundary yields a different centre.
        let c = snap_to_tile_centre(Vec2::new(17.0, 5.0));
        assert!(
            (a - c).length() > 0.01,
            "neighbouring tiles should have distinct centres"
        );
    }
}
