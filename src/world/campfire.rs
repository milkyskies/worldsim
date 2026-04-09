//! Campfire spawning logic.
//!
//! Reads: ItemSlots (none — campfires have no inventory)
//! Writes: Campfire entities (marker, EntityType, Physical, Transform)
//! Upstream: execution system (Build action on_complete → SpawnRequest)
//! Downstream: world entities (visible, perceivable by agents)

use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::Concept;
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;

/// Marker component identifying a campfire entity.
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct CampfireMarker;

/// Spawns a campfire entity at the given position.
///
/// The campfire is a persistent world entity — it has no inventory and does
/// not regenerate. Future systems can use the `CampfireMarker` to make
/// campfires perceivable (warmth radius, light source, safety zone).
pub fn spawn_campfire(commands: &mut Commands, position: Vec2) -> Entity {
    let fire_size = Vec2::new(TILE_SIZE * 0.8, TILE_SIZE * 0.8);
    let fire_color = Color::srgb(1.0, 0.5, 0.1);

    commands
        .spawn((
            Name::new("Campfire"),
            EntityType(Concept::Campfire),
            CampfireMarker,
            crate::world::Physical,
            Transform::from_translation(position.extend(1.0)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .with_children(|parent| {
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
    commands
        .spawn((
            Name::new("Campfire"),
            EntityType(Concept::Campfire),
            CampfireMarker,
            crate::world::Physical,
            Transform::from_translation(position.extend(1.0)),
            GlobalTransform::default(),
        ))
        .id()
}
