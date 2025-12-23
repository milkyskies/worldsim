//! Deer spawning logic.
//!
//! Deer are simple animal agents that:
//! - Wander around the world
//! - Eat berries (not apples) when hungry
//! - Flee from humans (they know Person is Dangerous)
//! - Have basic survival instincts

use crate::agent::body::species::SpeciesProfile;
use crate::agent::mind::knowledge::{Concept, MindGraph, Ontology};
use crate::agent::{
    Agent,
    inventory::{EntityType, Inventory},
};
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
    let inventory = Inventory::default();

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
        ))
        .insert((
            // Brains / Systems
            crate::agent::mind::memory::WorkingMemory::default(),
            crate::agent::brains::rational::RationalBrain::default(),
            crate::agent::brains::proposal::BrainState::default(),
            crate::agent::nervous_system::cns::CentralNervousSystem::default(),
            crate::agent::body::needs::PhysicalNeeds::default(),
            crate::agent::body::needs::Consciousness::default(),
            crate::agent::actions::ActionState::default(),
            crate::agent::psyche::emotions::EmotionalState::default(),
        ))
        .id();

    // Add body parts as children
    commands.entity(entity).with_children(|parent| {
        // Body (horizontal oval-ish)
        parent.spawn((
            Sprite {
                color: body_color,
                custom_size: Some(Vec2::new(14.0, 8.0)), // Horizontal body
                ..default()
            },
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
        ));

        // Head (smaller, to the right)
        parent.spawn((
            Sprite {
                color: head_color,
                custom_size: Some(Vec2::new(6.0, 6.0)),
                ..default()
            },
            Transform::from_translation(Vec3::new(8.0, 2.0, 0.1)),
        ));

        // Legs (4 small rectangles)
        let leg_color = Color::srgb(0.5, 0.35, 0.18);
        let leg_size = Vec2::new(2.0, 5.0);
        let leg_positions = [
            Vec3::new(-4.0, -5.0, 0.0),
            Vec3::new(-1.0, -5.0, 0.0),
            Vec3::new(2.0, -5.0, 0.0),
            Vec3::new(5.0, -5.0, 0.0),
        ];

        for pos in leg_positions {
            parent.spawn((
                Sprite {
                    color: leg_color,
                    custom_size: Some(leg_size),
                    ..default()
                },
                Transform::from_translation(pos),
            ));
        }

        // NAME TAG
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
fn add_deer_knowledge(mind: &mut MindGraph) {
    use crate::agent::mind::knowledge::{
        MemoryType, Metadata, Node, Predicate, Source, Triple, Value,
    };

    let meta = Metadata {
        source: Source::Intrinsic,
        memory_type: MemoryType::Intrinsic,
        timestamp: 0,
        confidence: 1.0,
        ..Default::default()
    };

    // === FOOD KNOWLEDGE ===

    // "I know berries are food"
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Berry),
        Predicate::IsA,
        Value::Concept(Concept::Food),
        meta.clone(),
    ));

    // "I know berry bushes produce berries"
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::BerryBush),
        Predicate::Produces,
        Value::Item(Concept::Berry, 1),
        meta.clone(),
    ));

    // === DANGER KNOWLEDGE ===
    // This is the key! Deer know Person is Dangerous.
    // When they perceive a Person, the emotional brain will trigger Fear,
    // and the survival brain will propose fleeing.

    // "I know persons are dangerous"
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Person),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        meta.clone(),
    ));

    // Deer do NOT know apples are food - they won't try to eat them!
}
