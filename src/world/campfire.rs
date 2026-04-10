//! Campfire spawning logic.
//!
//! Reads: ItemSlots (none — campfires have no inventory)
//! Writes: Campfire entities (CampfireMarker, EntityType, Physical, LightSource, HeatSource, FuelConsumer, Transform)
//! Upstream: execution system (Build action on_complete → SpawnRequest)
//! Downstream: world entities (visible, perceivable by agents), perceive_temperature (reads HeatSource)

use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::Concept;
use crate::world::map::TILE_SIZE;
use crate::world::property::{FuelConsumer, HeatSource, LightSource};
use bevy::prelude::*;

/// Marker component identifying a campfire entity.
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct CampfireMarker;

/// Campfire component bundle. Composed from property components — no special campfire logic.
///
/// - [`LightSource`]: emits light in a radius, perceivable by agents at night
/// - [`HeatSource`]: emits heat, perceivable by agents via temperature sense
/// - [`FuelConsumer`]: burns wood; removes LightSource and HeatSource when fuel runs out
pub fn campfire_components(position: Vec2) -> impl Bundle {
    (
        Name::new("Campfire"),
        EntityType(Concept::Campfire),
        CampfireMarker,
        LightSource {
            radius: 80.0,
            intensity: 0.8,
        },
        HeatSource {
            radius: 64.0,
            intensity: 0.8,
        },
        FuelConsumer {
            fuel_type: Concept::Wood,
            fuel_remaining: 200.0,
            consumption_rate: 1.0,
        },
        crate::world::Physical,
        Transform::from_translation(position.extend(1.0)),
        GlobalTransform::default(),
    )
}

/// Spawns a campfire entity at the given position (with sprites for the visual game).
pub fn spawn_campfire(commands: &mut Commands, position: Vec2) -> Entity {
    let fire_size = Vec2::new(TILE_SIZE * 0.8, TILE_SIZE * 0.8);
    let fire_color = Color::srgb(1.0, 0.5, 0.1);

    commands
        .spawn((
            campfire_components(position),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .with_children(|parent| {
            // Shadow — dark ellipse underneath the fire.
            parent.spawn((
                Sprite {
                    color: Color::srgba(0.0, 0.0, 0.0, 0.35),
                    custom_size: Some(Vec2::new(fire_size.x * 1.2, fire_size.y * 0.35)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, -fire_size.y * 0.45, -0.05)),
            ));

            // Base glow circle
            parent.spawn((
                Sprite {
                    color: fire_color,
                    custom_size: Some(fire_size),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            ));
            // Bright core
            parent.spawn((
                Sprite {
                    color: Color::srgb(1.0, 0.9, 0.3),
                    custom_size: Some(fire_size * 0.4),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.1)),
            ));
        })
        .id()
}

/// Logic-only campfire spawner for headless/test environments (no sprites).
pub fn spawn_campfire_headless(commands: &mut Commands, position: Vec2) -> Entity {
    commands.spawn(campfire_components(position)).id()
}
