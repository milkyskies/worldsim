//! Lean-to spawning logic.
//!
//! Reads: nothing (data + spawn primitives)
//! Writes: Lean-to entities (LeanToMarker, EntityType, Physical,
//!         ShelterProvider, Durability, Flammable, Transform)
//! Upstream: `becomes_system` (`spawn_concept_entity` dispatch when a
//!           construction site finishes), test fixtures
//! Downstream: world entities (perceivable; included in shelter queries)

use bevy::prelude::*;

use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::lean_to::{
    CAPACITY, DURABILITY_DECAY_PER_TICK, FLAMMABLE_BURN_TIME, INITIAL_DURABILITY, PROTECTION,
};
use crate::palette::{Palette, PaletteColor};
use crate::world::map::TILE_SIZE;
use crate::world::property::{Durability, Flammable, ShelterProvider};

/// Marker component identifying a lean-to entity.
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct LeanToMarker;

/// Component bundle for a freshly-built lean-to.
pub fn lean_to_components(position: Vec2) -> impl Bundle {
    (
        Name::new("LeanTo"),
        EntityType(Concept::LeanTo),
        LeanToMarker,
        ShelterProvider {
            capacity: CAPACITY,
            protection: PROTECTION,
        },
        Durability {
            current: INITIAL_DURABILITY,
            max: INITIAL_DURABILITY,
            decay_rate: DURABILITY_DECAY_PER_TICK,
        },
        Flammable {
            burn_time: FLAMMABLE_BURN_TIME,
        },
        crate::world::Physical,
        Transform::from_translation(position.extend(1.0)),
        GlobalTransform::default(),
    )
}

/// Logic-only spawner for headless / test environments.
pub fn spawn_lean_to_headless(commands: &mut Commands, position: Vec2) -> Entity {
    commands.spawn(lean_to_components(position)).id()
}

/// Spawn with a simple sprite for the windowed game.
pub fn spawn_lean_to(commands: &mut Commands, palette: &Palette, position: Vec2) -> Entity {
    let body_color = palette.srgb(PaletteColor::LeafForest);
    let footprint = Vec2::new(TILE_SIZE * 1.4, TILE_SIZE * 1.0);

    commands
        .spawn((
            lean_to_components(position),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .with_children(|parent| {
            parent.spawn((
                Sprite {
                    color: palette.shadow(),
                    custom_size: Some(Vec2::new(footprint.x * 1.1, footprint.y * 0.4)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, -footprint.y * 0.5, -0.05)),
            ));
            parent.spawn((
                Sprite {
                    color: body_color,
                    custom_size: Some(footprint),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            ));
        })
        .id()
}
