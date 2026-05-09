//! Human (Person) spawning logic.

use crate::agent::biology::body::BodyNodeKind;
use crate::agent::body::genetics::founder::random_genome;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::species::Species;
use crate::agent::mind::knowledge::Ontology;
use crate::agent::naming::human_name;
use crate::agent::spawn_human::{PersonInit, build_person_logic};
use crate::markings::{Markings, apply_markings};
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

const HUMAN_HAIR_COLORS: [PaletteColor; 4] = [
    PaletteColor::FurBlack,
    PaletteColor::FurCharcoal,
    PaletteColor::SkinDeep,
    PaletteColor::SkinDark,
];

/// Canonical chibi-style human silhouette: big round head, small body,
/// stubby arms and legs, hair cap, simple face with two eyes and a tiny
/// smile. Body parts get day/night agent tinting; face features (eyes,
/// mouth) and hair stay readable through dim hours.
pub fn human_silhouette(skin: PaletteColor, hair: PaletteColor) -> CreatureSilhouette {
    let eye = |x: f32| SilhouettePart {
        body_node: None,
        shape: Shape::Circle,
        size: Vec2::new(1.8, 1.8),
        offset: Vec2::new(x, 9.5),
        rotation: 0.0,
        color: PaletteColor::FurBlack,
        z_bias: 2,
        role: PartRole::Eye,
        tint_with_environment: false,
    };
    let arm = |x: f32| SilhouettePart {
        body_node: None,
        shape: Shape::Capsule,
        size: Vec2::new(2.2, 6.0),
        offset: Vec2::new(x, -1.5),
        rotation: 0.0,
        color: skin,
        z_bias: 0,
        role: PartRole::Limb,
        tint_with_environment: true,
    };
    let leg = |x: f32| SilhouettePart {
        body_node: None,
        shape: Shape::Capsule,
        size: Vec2::new(2.5, 5.0),
        offset: Vec2::new(x, -7.5),
        rotation: 0.0,
        color: skin,
        z_bias: 0,
        role: PartRole::Limb,
        tint_with_environment: true,
    };
    CreatureSilhouette {
        parts: vec![
            // Smaller torso - chibi proportions favor a big head over a big body.
            SilhouettePart {
                body_node: Some(BodyNodeKind::Torso),
                shape: Shape::Capsule,
                size: Vec2::new(7.0, 7.0),
                offset: Vec2::new(0.0, -1.0),
                rotation: 0.0,
                color: skin,
                z_bias: 0,
                role: PartRole::Body,
                tint_with_environment: true,
            },
            arm(-4.5),
            arm(4.5),
            leg(-2.0),
            leg(2.0),
            // Tiny visible neck so the head doesn't sit directly on the chest.
            SilhouettePart {
                body_node: None,
                shape: Shape::Capsule,
                size: Vec2::new(3.0, 1.8),
                offset: Vec2::new(0.0, 3.5),
                rotation: 0.0,
                color: skin,
                z_bias: 0,
                role: PartRole::Body,
                tint_with_environment: true,
            },
            // Big chibi head.
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
            // Hair cap on top of the head.
            SilhouettePart {
                body_node: None,
                shape: Shape::Capsule,
                size: Vec2::new(10.0, 4.0),
                offset: Vec2::new(0.0, 12.5),
                rotation: 0.0,
                color: hair,
                z_bias: 1,
                role: PartRole::Marking,
                tint_with_environment: true,
            },
            eye(-2.2),
            eye(2.2),
            // Tiny smile.
            SilhouettePart {
                body_node: None,
                shape: Shape::Ellipse,
                size: Vec2::new(2.0, 0.6),
                offset: Vec2::new(0.0, 6.8),
                rotation: 0.0,
                color: PaletteColor::FurBlack,
                z_bias: 2,
                role: PartRole::Marking,
                tint_with_environment: false,
            },
        ],
        shadow_size: Vec2::new(9.0, 3.5),
        shadow_offset_y: -10.0,
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
    let genome = random_genome(rng, Species::Human);
    let markings = Markings::from_genome(&genome);
    let skin = HUMAN_SKIN_TONES[rng.random_range(0..HUMAN_SKIN_TONES.len())];
    let hair = HUMAN_HAIR_COLORS[rng.random_range(0..HUMAN_HAIR_COLORS.len())];
    let silhouette = apply_markings(human_silhouette(skin, hair), &markings)
        .with_hop_phase(index as f32 * 1.618);
    let name_tag_y = silhouette.top_y() + 16.0;
    let (core, perception, brain) = build_person_logic(
        PersonInit {
            name: display_name.clone(),
            position,
            genome,
            // Game agents spawn in the morning (START_HOUR = 06:00) after
            // a full night's sleep — empty stomach, moderate thirst. Tests
            // that want fresh-well-fed agents still use `PhysicalNeeds::default()`.
            physical_needs: PhysicalNeeds::just_woke_up(),
            cultural_knowledge,
            extra_knowledge: Vec::new(),
        },
        ontology,
    );

    let entity = commands
        .spawn(core)
        .insert(perception)
        .insert((
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            crate::ui::sprite_animation::VisualOffset::default(),
            markings,
            silhouette,
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
            Transform::from_translation(Vec3::new(0.0, name_tag_y, 1.0)),
            crate::ui::sprite_animation::NameTag::new(entity, name_tag_y),
        ));
    });

    entity
}
