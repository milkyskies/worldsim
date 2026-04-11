//! Campfire spawning logic.
//!
//! Reads: ItemSlots (fuel slot), TickCount (started_tick for Becomes)
//! Writes: Campfire entities (CampfireMarker, EntityType, Physical, LightSource, HeatSource, FuelConsumer, EmitsEffect, ItemSlots, Becomes, Transform)
//! Upstream: execution system (Build action on_complete → SpawnRequest), becomes_system (construction site → campfire)
//! Downstream: world entities (perceivable by agents), perceive_temperature, emits_effect_system, fuel_system, becomes_system (→ Ash)

use crate::agent::inventory::EntityType;
use crate::agent::item_slots::{ItemSlots, Slot, Thing};
use crate::agent::mind::knowledge::Concept;
use crate::world::emits_effect::{EffectKind, EmitsEffect};
use crate::world::environment::CampfireGlowSprite;
use crate::world::map::TILE_SIZE;
use crate::world::property::{FuelConsumer, HeatSource, LightSource};
use bevy::prelude::*;

/// How much `fuel_remaining` one wood item provides when consumed from the fuel slot.
pub const FUEL_PER_WOOD: f32 = 200.0;

/// Number of wood items a freshly-built campfire starts with in its fuel slot.
pub const INITIAL_WOOD_COUNT: u32 = 3;

/// Maximum wood items the fuel slot can hold.
pub const FUEL_SLOT_CAPACITY: u32 = 5;

/// Marker component identifying a campfire entity.
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct CampfireMarker;

/// Campfire component bundle. Composed from property components — no special campfire logic.
///
/// - [`LightSource`]: emits light in a radius, perceivable by agents at night
/// - [`HeatSource`]: emits heat, perceivable by agents via temperature sense
/// - [`FuelConsumer`]: burns wood; auto-reloads from the fuel slot when depleted
/// - [`EmitsEffect`]: comfort aura — reduces stress and restores stamina for nearby agents
/// - [`ItemSlots`]: fuel slot holding wood items; agents refuel via Deposit
/// - [`Becomes`]: transforms to Ash when fuel is fully exhausted
pub fn campfire_components(position: Vec2) -> impl Bundle {
    let mut fuel_slot = Slot::fuel(Concept::Wood, FUEL_SLOT_CAPACITY);
    for _ in 0..INITIAL_WOOD_COUNT {
        fuel_slot.contents.push(Thing::new(Concept::Wood));
    }

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
            fuel_remaining: FUEL_PER_WOOD,
            consumption_rate: 1.0,
        },
        EmitsEffect::new(
            80.0,
            EffectKind::All(vec![
                EffectKind::StressPerSec(-0.5),
                EffectKind::StaminaPerSec(2.0),
            ]),
        ),
        ItemSlots {
            slots: vec![fuel_slot],
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

            // Night glow — large warm circle that brightens the area at night.
            // Alpha is driven by CampfireGlowSprite system; starts at 0 (daytime).
            // Size matches the LightSource radius defined in campfire_components.
            parent.spawn((
                CampfireGlowSprite,
                Sprite {
                    color: Color::srgba(1.0, 0.6, 0.2, 0.0),
                    custom_size: Some(Vec2::splat(160.0)), // 2× LightSource radius (80.0)
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, -0.1)),
            ));
        })
        .id()
}

/// Logic-only campfire spawner for headless/test environments (no sprites).
pub fn spawn_campfire_headless(commands: &mut Commands, position: Vec2) -> Entity {
    commands.spawn(campfire_components(position)).id()
}
