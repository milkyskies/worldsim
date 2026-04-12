//! Human (Person) spawning logic.

use crate::agent::body::genetics::founder::random_genome;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::body::species::Species;
use crate::agent::mind::knowledge::Ontology;
use crate::agent::naming::human_name;
use crate::agent::spawn_human::{PersonInit, build_person_logic};
use crate::world::environment::{AgentBodySprite, BaseColor};
use bevy::prelude::*;
use rand::Rng;

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
            physical_needs: PhysicalNeeds::default(),
            cultural_knowledge,
            extra_knowledge: Vec::new(),
        },
        ontology,
    );

    // Random Skin Color
    let skin_tones = [
        Color::srgb(1.0, 0.87, 0.76),  // Pale
        Color::srgb(0.96, 0.80, 0.69), // Fair
        Color::srgb(0.89, 0.70, 0.53), // Tan
        Color::srgb(0.76, 0.57, 0.35), // Medium
        Color::srgb(0.55, 0.38, 0.22), // Dark
        Color::srgb(0.39, 0.25, 0.12), // Deep
    ];
    let skin_color = skin_tones[rng.random_range(0..skin_tones.len())];

    let entity = commands
        .spawn(core)
        .insert(perception)
        .insert((
            // Rendering visibility — needed by Bevy's visibility propagation,
            // not part of the logic-only bundle since TestWorld skips it.
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            crate::ui::sprite_animation::VisualOffset::default(),
        ))
        .insert(brain)
        .id();

    commands.entity(entity).with_children(|parent| {
        // Ground shadow — dark ellipse at the agent's feet. Tracks terrain
        // elevation but not the bounce so the sprite visibly hops above it.
        parent.spawn((
            crate::ui::sprite_animation::GroundShadow::new(entity, Vec2::new(0.0, -8.0)),
            Sprite {
                color: Color::srgba(0.0, 0.0, 0.0, 0.35),
                custom_size: Some(Vec2::new(10.0, 4.0)),
                ..default()
            },
            Transform::from_translation(Vec3::new(0.0, -8.0, -0.05)),
        ));

        // SpriteBody wrapper — animated (hops), contains all visual sprite parts
        parent
            .spawn((
                crate::ui::sprite_animation::SpriteBody::new(entity, index as f32 * 1.618),
                Transform::default(),
                GlobalTransform::default(),
                Visibility::default(),
                InheritedVisibility::default(),
                ViewVisibility::default(),
            ))
            .with_children(|body| {
                // BODY (Torso)
                body.spawn((
                    Sprite {
                        color: skin_color,
                        custom_size: Some(Vec2::new(10.0, 12.0)),
                        ..default()
                    },
                    BaseColor(skin_color),
                    AgentBodySprite,
                    Transform::from_translation(Vec3::new(0.0, -2.0, 0.0)),
                ));

                // HEAD
                body.spawn((
                    Sprite {
                        color: skin_color,
                        custom_size: Some(Vec2::new(10.0, 10.0)),
                        ..default()
                    },
                    BaseColor(skin_color),
                    AgentBodySprite,
                    Transform::from_translation(Vec3::new(0.0, 9.0, 0.1)),
                ))
                .with_children(|head| {
                    let eye_color = Color::BLACK;
                    let eye_size = Vec2::new(2.0, 2.0);

                    head.spawn((
                        Sprite {
                            color: eye_color,
                            custom_size: Some(eye_size),
                            ..default()
                        },
                        Transform::from_translation(Vec3::new(-2.5, 1.0, 0.1)),
                    ));

                    head.spawn((
                        Sprite {
                            color: eye_color,
                            custom_size: Some(eye_size),
                            ..default()
                        },
                        Transform::from_translation(Vec3::new(2.5, 1.0, 0.1)),
                    ));
                });
            });

        // NAME TAG — direct child of root, stays still
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
