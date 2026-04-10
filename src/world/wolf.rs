//! Wolf spawning logic — carnivorous pack predator entity.
//!
//! Reads: SpeciesProfile::wolf(), Ontology
//! Writes: Wolf component, MindGraph (innate prey/threat knowledge), world entities
//! Upstream: world::spawner (calls spawn_wolf), world::map (biome placement)
//! Downstream: agent brains (fear/flee in humans/deer, anger/attack in wolves)

use crate::agent::Agent;
use crate::agent::body::needs::PsychologicalDrives;
use crate::agent::body::species::SpeciesProfile;
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, MindGraph, Ontology};
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;

/// Marker component for wolf entities.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Wolf;

/// Spawns a Wolf (Predator Agent)
pub fn spawn_wolf(
    commands: &mut Commands,
    ontology: Ontology,
    position: Vec2,
    index: usize,
) -> Entity {
    use crate::agent::psyche::personality::Personality;

    let species_profile = SpeciesProfile::wolf();
    let inventory = ItemSlots::agent_carry();
    let personality = Personality::random();

    let spawn_tile = (
        (position.x / TILE_SIZE) as i32,
        (position.y / TILE_SIZE) as i32,
    );

    let mut mind = MindGraph::new(ontology);
    add_wolf_knowledge(&mut mind, spawn_tile);

    let body_color = Color::srgb(0.55, 0.55, 0.55); // Gray
    let head_color = Color::srgb(0.60, 0.60, 0.60); // Slightly lighter gray

    let entity = commands
        .spawn((
            Name::new(format!("Wolf {}", index)),
            Agent,
            Wolf,
            EntityType(Concept::Wolf),
            species_profile,
            crate::world::Physical,
            crate::agent::TargetPosition::default(),
            crate::agent::movement::MovementState::default(),
            inventory,
            personality,
            Transform::from_translation(position.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            crate::agent::affordance::Affordance::default(),
            mind,
            crate::agent::mind::perception::Vision { range: 120.0 },
            crate::agent::mind::perception::VisibleObjects::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .insert((
            crate::agent::mind::memory::WorkingMemory::default(),
            crate::agent::brains::rational::RationalBrain::default(),
            crate::agent::brains::proposal::BrainState::default(),
            crate::agent::nervous_system::cns::CentralNervousSystem::default(),
            crate::agent::body::needs::PhysicalNeeds::default(),
            crate::agent::body::needs::Consciousness::default(),
            PsychologicalDrives::default(),
            crate::agent::actions::ActiveActions::default(),
            crate::agent::psyche::emotions::EmotionalState::default(),
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
                        custom_size: Some(Vec2::new(16.0, 9.0)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                ));

                body.spawn((
                    Sprite {
                        color: head_color,
                        custom_size: Some(Vec2::new(8.0, 7.0)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(9.0, 1.0, 0.1)),
                ));

                // Ears
                body.spawn((
                    Sprite {
                        color: body_color,
                        custom_size: Some(Vec2::new(3.0, 4.0)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(7.0, 6.0, 0.2)),
                ));

                body.spawn((
                    Sprite {
                        color: body_color,
                        custom_size: Some(Vec2::new(3.0, 4.0)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(10.0, 6.0, 0.2)),
                ));

                // Legs
                let leg_color = Color::srgb(0.45, 0.45, 0.45);
                let leg_size = Vec2::new(2.5, 5.0);
                let leg_positions = [
                    Vec3::new(-5.0, -6.0, 0.0),
                    Vec3::new(-2.0, -6.0, 0.0),
                    Vec3::new(2.0, -6.0, 0.0),
                    Vec3::new(5.0, -6.0, 0.0),
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

                // Tail
                body.spawn((
                    Sprite {
                        color: body_color,
                        custom_size: Some(Vec2::new(7.0, 3.0)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(-10.0, 2.0, 0.0)),
                ));
            });

        // Name tag — direct child of root, stays still
        parent.spawn((
            Text2d::new(format!("Wolf {}", index)),
            TextFont {
                font_size: 8.0,
                ..default()
            },
            TextColor(Color::srgb(1.0, 0.3, 0.3)),
            Transform::from_translation(Vec3::new(0.0, 14.0, 1.0)),
        ));
    });

    entity
}

/// Adds wolf-specific innate biological knowledge.
///
/// Wolves do not have hardcoded emotion triggers. Their behavior emerges from
/// drives (hunger → hunt deer), threat assessment (humans are dangerous → fear/flee
/// when outnumbered), and territorial drive (intruder on owned tile → attack).
pub(crate) fn add_wolf_knowledge(mind: &mut MindGraph, spawn_tile: (i32, i32)) {
    use crate::agent::mind::knowledge::{Metadata, Node, Predicate, Triple, Value};

    let meta = Metadata::default(); // Source::Intrinsic, confidence 1.0

    // Prey recognition: wolves are born knowing deer is food.
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Deer),
        Predicate::IsA,
        Value::Concept(Concept::Food),
        meta.clone(),
    ));

    // Prey trait: deer is prey (affects hunt action selection).
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Deer),
        Predicate::HasTrait,
        Value::Concept(Concept::Prey),
        meta.clone(),
    ));

    // Humans are dangerous — real wolves have centuries of learned wariness.
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Person),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        meta.clone(),
    ));

    // Fire-fear: react_to_danger reacts to any visible entity with the
    // Dangerous trait, so this triple alone keeps wolves away from campfires.
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Campfire),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        meta.clone(),
    ));

    // Natal territory: mark the spawn tile as owned territory.
    // The Territoriality drive system queries (tile, HasTrait, Territory) triples.
    mind.assert(Triple::with_meta(
        Node::Tile(spawn_tile),
        Predicate::HasTrait,
        Value::Concept(Concept::Territory),
        meta,
    ));
}
