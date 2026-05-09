//! Storage chest spawner. Public-access `ItemSlots` so any agent can
//! deposit and any agent can take. Same general shape as the campfire
//! and shelter spawners.

use bevy::prelude::*;

use crate::agent::inventory::EntityType;
use crate::agent::item_slots::{Access, ItemSlots, Slot, SlotFilter, SlotRole};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::storage_chest::{
    CAPACITY, DURABILITY_DECAY_PER_TICK, FLAMMABLE_BURN_TIME, INITIAL_DURABILITY,
};
use crate::palette::{Palette, PaletteColor};
use crate::world::map::TILE_SIZE;
use crate::world::property::{Durability, Flammable};

fn chest_slot() -> Slot {
    Slot {
        role: SlotRole::Free,
        filter: SlotFilter::Any,
        capacity: Some(CAPACITY),
        contents: Vec::new(),
        deposit_access: Access::Public,
        extract_access: Access::Public,
    }
}

pub fn storage_chest_components(position: Vec2) -> impl Bundle {
    (
        Name::new("StorageChest"),
        EntityType(Concept::StorageChest),
        ItemSlots {
            slots: vec![chest_slot()],
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

pub fn spawn_storage_chest_headless(commands: &mut Commands, position: Vec2) -> Entity {
    commands.spawn(storage_chest_components(position)).id()
}

pub fn spawn_storage_chest(commands: &mut Commands, palette: &Palette, position: Vec2) -> Entity {
    let body_color = palette.srgb(PaletteColor::LeafDeep);
    let footprint = Vec2::new(TILE_SIZE * 0.9, TILE_SIZE * 0.7);

    commands
        .spawn((
            storage_chest_components(position),
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
