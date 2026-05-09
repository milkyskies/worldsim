//! Lean-to shelter spawner. Component bundle is shared via
//! `world::property::shelter_components`; sprite presentation is local.

use bevy::prelude::*;

use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::lean_to::{
    CAPACITY, DURABILITY_DECAY_PER_TICK, FLAMMABLE_BURN_TIME, INITIAL_DURABILITY, PROTECTION,
};
use crate::palette::{Palette, PaletteColor};
use crate::world::map::TILE_SIZE;
use crate::world::property::{ShelterSpec, shelter_components};

fn spec() -> ShelterSpec {
    ShelterSpec {
        name: "LeanTo",
        concept: Concept::LeanTo,
        capacity: CAPACITY,
        protection: PROTECTION,
        initial_durability: INITIAL_DURABILITY,
        durability_decay_per_tick: DURABILITY_DECAY_PER_TICK,
        flammable_burn_time: FLAMMABLE_BURN_TIME,
    }
}

pub fn lean_to_components(position: Vec2) -> impl Bundle {
    shelter_components(spec(), position)
}

pub fn spawn_lean_to_headless(commands: &mut Commands, position: Vec2) -> Entity {
    commands.spawn(lean_to_components(position)).id()
}

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
