//! Deer spawning logic.
//!
//! Deer are simple animal agents that:
//! - Wander around the world
//! - Eat berries (not apples) when hungry
//! - Flee from humans (they know Person is Dangerous)
//! - Have basic survival instincts

use crate::agent::body::species::SpeciesProfile;
use crate::agent::mind::knowledge::{Concept, MindGraph, Ontology};
use crate::agent::{Agent, inventory::EntityType, item_slots::ItemSlots};
use bevy::prelude::*;

/// Marker component for deer entities.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Deer;

/// Spawns a Deer (Animal Agent)
pub fn spawn_deer(
    commands: &mut Commands,
    ontology: Ontology,
    position: Vec2,
    index: usize,
) -> Entity {
    use crate::agent::psyche::personality::Personality;

    let species_profile = SpeciesProfile::deer();
    let inventory = ItemSlots::agent_carry();

    // Deer get a simplified personality (for now, reusing human system)
    // TODO: Implement Universal traits (boldness, aggression) for animals
    let personality = Personality::random();

    // Initialize Mind with Ontology
    let mut mind = MindGraph::new(ontology);

    // Add deer-specific innate knowledge
    add_deer_knowledge(&mut mind);

    // Deer colors
    let body_color = Color::srgb(0.6, 0.4, 0.2); // Brown
    let head_color = Color::srgb(0.65, 0.45, 0.25); // Slightly lighter brown

    let entity = commands
        .spawn((
            Name::new(format!("Deer {}", index)),
            Agent, // All thinking entities have this
            Deer,  // Deer-specific marker
            EntityType(Concept::Deer),
            species_profile,
            crate::world::Physical,
            crate::agent::TargetPosition::default(),
            crate::agent::movement::MovementState::default(),
            inventory,
            personality,
            // ROOT HAS NO SPRITE (Invisible Container)
            Transform::from_translation(position.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            crate::agent::affordance::Affordance::default(),
            mind,
            // Vision/Perception (from species profile)
            crate::agent::mind::perception::Vision { range: 128.0 },
            crate::agent::mind::perception::VisibleObjects::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            crate::ui::sprite_animation::VisualOffset::default(),
        ))
        .insert((
            // Brains / Systems
            crate::agent::mind::memory::WorkingMemory::default(),
            crate::agent::brains::rational::RationalBrain::default(),
            crate::agent::brains::proposal::BrainState::default(),
            crate::agent::nervous_system::cns::CentralNervousSystem::default(),
            crate::agent::body::needs::PhysicalNeeds::default(),
            crate::agent::body::needs::Consciousness::default(),
            // Deer need `PsychologicalDrives` so they can feel isolation
            // stress and drift back toward herd-mates. Before #260 they had
            // no drives at all — no social, no fun, no curiosity — and
            // therefore no herd cohesion emerging from urgency.
            crate::agent::body::needs::PsychologicalDrives::default(),
            crate::agent::actions::ActiveActions::default(),
            crate::agent::psyche::emotions::EmotionalState::default(),
            crate::agent::skills::Skills::default(),
        ))
        .id();

    commands.entity(entity).with_children(|parent| {
        // Ground shadow — sits on the terrain, no bounce.
        parent.spawn((
            crate::ui::sprite_animation::GroundShadow::new(entity, Vec2::new(0.0, -6.0)),
            Sprite {
                color: Color::srgba(0.0, 0.0, 0.0, 0.35),
                custom_size: Some(Vec2::new(14.0, 5.0)),
                ..default()
            },
            Transform::from_translation(Vec3::new(0.0, -6.0, -0.05)),
        ));

        // SpriteBody wrapper — animated (hops)
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
                body.spawn((
                    Sprite {
                        color: body_color,
                        custom_size: Some(Vec2::new(14.0, 8.0)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                ));

                body.spawn((
                    Sprite {
                        color: head_color,
                        custom_size: Some(Vec2::new(6.0, 6.0)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(8.0, 2.0, 0.1)),
                ));

                let leg_color = Color::srgb(0.5, 0.35, 0.18);
                let leg_size = Vec2::new(2.0, 5.0);
                let leg_positions = [
                    Vec3::new(-4.0, -5.0, 0.0),
                    Vec3::new(-1.0, -5.0, 0.0),
                    Vec3::new(2.0, -5.0, 0.0),
                    Vec3::new(5.0, -5.0, 0.0),
                ];

                for pos in leg_positions {
                    body.spawn((
                        Sprite {
                            color: leg_color,
                            custom_size: Some(leg_size),
                            ..default()
                        },
                        Transform::from_translation(pos),
                    ));
                }
            });

        // NAME TAG — direct child of root, stays still
        parent.spawn((
            Text2d::new(format!("Deer {}", index)),
            TextFont {
                font_size: 8.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Transform::from_translation(Vec3::new(0.0, 12.0, 1.0)),
        ));
    });

    entity
}

/// Adds deer-specific innate knowledge to the mind.
/// Deer know:
/// - Berries are food (but NOT apples)
/// - Persons are dangerous (triggers fear → flee)
pub(crate) fn add_deer_knowledge(mind: &mut MindGraph) {
    use crate::agent::mind::knowledge::{Metadata, Node, Predicate, Triple, Value};

    let meta = Metadata::default(); // Source::Intrinsic, confidence 1.0

    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Berry),
        Predicate::IsA,
        Value::Concept(Concept::Food),
        meta.clone(),
    ));

    mind.assert(Triple::with_meta(
        Node::Concept(Concept::BerryBush),
        Predicate::Produces,
        Value::Item(Concept::Berry, 1),
        meta.clone(),
    ));

    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Person),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        meta.clone(),
    ));

    // Wolves are predators — deer are born knowing to flee them.
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Wolf),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        meta,
    ));

    // Deer do NOT know apples are food - they won't try to eat them!
}
