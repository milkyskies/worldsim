//! House spawning logic.
//!
//! Reads: nothing (data + spawn primitives)
//! Writes: House entities (HouseMarker, EntityType, Physical,
//!         ShelterProvider, Durability, Flammable, Transform)
//! Upstream: `becomes_system` (`spawn_concept_entity` dispatch when a
//!           house construction site finishes), test fixtures
//! Downstream: world entities (perceivable; included in shelter queries)

use bevy::prelude::*;

use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::house::{
    CAPACITY, DURABILITY_DECAY_PER_TICK, FLAMMABLE_BURN_TIME, INITIAL_DURABILITY, PROTECTION,
};
use crate::palette::{Palette, PaletteColor};
use crate::world::map::TILE_SIZE;
use crate::world::property::{Durability, Flammable, ShelterProvider};

/// Marker component identifying a house entity.
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct HouseMarker;

/// Component bundle for a freshly-built house.
pub fn house_components(position: Vec2) -> impl Bundle {
    (
        Name::new("House"),
        EntityType(Concept::House),
        HouseMarker,
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
pub fn spawn_house_headless(commands: &mut Commands, position: Vec2) -> Entity {
    commands.spawn(house_components(position)).id()
}

/// Spawn with a simple sprite for the windowed game.
pub fn spawn_house(commands: &mut Commands, palette: &Palette, position: Vec2) -> Entity {
    let body_color = palette.srgb(PaletteColor::LeafDeep);
    let footprint = Vec2::new(TILE_SIZE * 2.0, TILE_SIZE * 1.6);

    commands
        .spawn((
            house_components(position),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .with_children(|parent| {
            parent.spawn((
                Sprite {
                    color: palette.shadow(),
                    custom_size: Some(Vec2::new(footprint.x * 1.05, footprint.y * 0.4)),
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
