//! Human (Person) spawning logic.

use crate::agent::biology::body::BodyNodeKind;
use crate::agent::body::genetics::founder::random_genome;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::species::Species;
use crate::agent::mind::knowledge::Ontology;
use crate::agent::naming::human_name;
use crate::agent::spawn_human::{PersonInit, build_person_logic};
use crate::palette::PaletteColor;
use crate::silhouette::{CreatureSilhouette, PartRole, Shape, SilhouettePart};
use bevy::prelude::*;
use rand::Rng;

const HUMAN_SKIN_TONES: [PaletteColor; 6] = [
    PaletteColor::SkinPale,
    PaletteColor::SkinFair,
    PaletteColor::SkinTan,
    PaletteColor::SkinMedium,
    PaletteColor::SkinDark,
    PaletteColor::SkinDeep,
];

/// Canonical human silhouette parameterized by skin tone (gene-driven).
/// Body and head opt into agent-style day/night tinting; eyes do not, so
/// they stay readable through dim hours.
pub fn human_silhouette(skin: PaletteColor) -> CreatureSilhouette {
    let eye = |x: f32| SilhouettePart {
        body_node: None,
        shape: Shape::Circle,
        size: Vec2::new(2.0, 2.0),
        offset: Vec2::new(x, 10.0),
        rotation: 0.0,
        color: PaletteColor::FurBlack,
        z_bias: 2,
        role: PartRole::Eye,
        tint_with_environment: false,
    };
    CreatureSilhouette {
        parts: vec![
            SilhouettePart {
                body_node: Some(BodyNodeKind::Torso),
                shape: Shape::Capsule,
                size: Vec2::new(10.0, 12.0),
                offset: Vec2::new(0.0, -2.0),
                rotation: 0.0,
                color: skin,
                z_bias: 0,
                role: PartRole::Body,
                tint_with_environment: true,
            },
            SilhouettePart {
                body_node: Some(BodyNodeKind::Head),
                shape: Shape::Circle,
                size: Vec2::new(10.0, 10.0),
                offset: Vec2::new(0.0, 9.0),
                rotation: 0.0,
                color: skin,
                z_bias: 1,
                role: PartRole::Body,
                tint_with_environment: true,
            },
            eye(-2.5),
            eye(2.5),
        ],
        shadow_size: Vec2::new(10.0, 4.0),
        shadow_offset_y: -8.0,
        hop_phase: 0.0,
    }
}

/// Spawns a Person (Human Agent)
pub fn spawn_person<R: Rng>(
    commands: &mut Commands,
    ontology: Ontology,
    position: Vec2,
    index: usize,
    _culture: crate::agent::culture::Culture,
    cultural_knowledge: std::sync::Arc<Vec<crate::agent::mind::knowledge::Triple>>,
    rng: &mut R,
) -> Entity {
    let display_name = human_name(index);
    let (core, perception, brain) = build_person_logic(
        PersonInit {
            name: display_name.clone(),
            position,
            genome: random_genome(rng, Species::Human),
            // Game agents spawn in the morning (START_HOUR = 06:00) after
            // a full night's sleep — empty stomach, moderate thirst. Tests
            // that want fresh-well-fed agents still use `PhysicalNeeds::default()`.
            physical_needs: PhysicalNeeds::just_woke_up(),
            cultural_knowledge,
            extra_knowledge: Vec::new(),
        },
        ontology,
    );

    let skin = HUMAN_SKIN_TONES[rng.random_range(0..HUMAN_SKIN_TONES.len())];

    let entity = commands
        .spawn(core)
        .insert(perception)
        .insert((
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            crate::ui::sprite_animation::VisualOffset::default(),
            human_silhouette(skin).with_hop_phase(index as f32 * 1.618),
        ))
        .insert(brain)
        .id();

    commands.entity(entity).with_children(|parent| {
        parent.spawn((
            Text2d::new(display_name.clone()),
            TextFont {
                font_size: 10.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Transform::from_translation(Vec3::new(0.0, 20.0, 1.0)),
        ));
    });

    entity
}
