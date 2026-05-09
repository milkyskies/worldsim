//! Deer spawning logic.
//!
//! Deer are simple animal agents that:
//! - Wander around the world
//! - Eat berries (not apples) when hungry
//! - Flee from humans (they know Person is Dangerous)
//! - Have basic survival instincts

use crate::agent::biology::body::BodyNodeKind;
use crate::agent::body::genetics::founder::random_genome;
use crate::agent::body::species::{Species, SpeciesProfile};
use crate::agent::mind::knowledge::{Concept, MindGraph, Ontology};
use crate::agent::naming::deer_name;
use crate::agent::{Agent, Alive, inventory::EntityType, item_slots::ItemSlots};
use crate::palette::PaletteColor;
use crate::silhouette::{CreatureSilhouette, PartRole, Shape, SilhouettePart};
use bevy::prelude::*;
use rand::Rng;

/// Marker component for deer entities.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Deer;

/// Canonical deer silhouette.
pub fn deer_silhouette() -> CreatureSilhouette {
    let fur = PaletteColor::SkinDark;
    let leg_fur = PaletteColor::SkinDeep;
    let leg = |x: f32| SilhouettePart {
        body_node: None,
        shape: Shape::Capsule,
        size: Vec2::new(2.0, 5.0),
        offset: Vec2::new(x, -5.0),
        rotation: 0.0,
        color: leg_fur,
        z_bias: 0,
        role: PartRole::Limb,
        tint_with_environment: false,
    };
    CreatureSilhouette {
        parts: vec![
            SilhouettePart {
                body_node: Some(BodyNodeKind::Torso),
                shape: Shape::Ellipse,
                size: Vec2::new(14.0, 8.0),
                offset: Vec2::ZERO,
                rotation: 0.0,
                color: fur,
                z_bias: 0,
                role: PartRole::Body,
                tint_with_environment: false,
            },
            SilhouettePart {
                body_node: Some(BodyNodeKind::Head),
                shape: Shape::Circle,
                size: Vec2::new(6.0, 6.0),
                offset: Vec2::new(8.0, 2.0),
                rotation: 0.0,
                color: fur,
                z_bias: 1,
                role: PartRole::Body,
                tint_with_environment: false,
            },
            leg(-4.0),
            leg(-1.0),
            leg(2.0),
            leg(5.0),
        ],
        shadow_size: Vec2::new(14.0, 5.0),
        shadow_offset_y: -6.0,
        hop_phase: 0.0,
    }
}

/// Spawns a Deer (Animal Agent)
pub fn spawn_deer<R: Rng>(
    commands: &mut Commands,
    ontology: Ontology,
    position: Vec2,
    index: usize,
    rng: &mut R,
) -> Entity {
    let species_profile = SpeciesProfile::deer();
    let inventory = ItemSlots::agent_carry();
    let genome = random_genome(rng, Species::Deer);

    let mut mind = MindGraph::new(ontology);
    add_deer_knowledge(&mut mind);

    let entity = commands
        .spawn((
            Name::new(deer_name(index)),
            Agent,
            Alive,
            Deer,
            EntityType(Concept::Deer),
            species_profile,
            crate::world::Physical,
            crate::agent::TargetPosition::default(),
            crate::agent::movement::MovementState::default(),
            inventory,
            genome,
            Transform::from_translation(position.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            crate::agent::affordance::Affordance::default(),
            mind,
            // Vision range is set by develop_phenotype_system from species.vision_range
            // × phenotype.vision; this placeholder is overwritten before first perception tick.
            crate::agent::mind::perception::Vision {
                range: SpeciesProfile::deer().vision_range,
            },
            crate::agent::mind::perception::VisibleObjects::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            crate::ui::sprite_animation::VisualOffset::default(),
            deer_silhouette().with_hop_phase(index as f32 * 1.618),
        ))
        .insert((
            crate::agent::mind::memory::WorkingMemory::default(),
            crate::agent::brains::rational::RationalBrain,
            crate::agent::brains::plan_memory::PlanMemory::default(),
            crate::agent::brains::proposal::BrainState::default(),
            crate::agent::nervous_system::cns::CentralNervousSystem::default(),
            crate::agent::body::needs::PhysicalNeeds::default(),
            crate::agent::body::needs::Consciousness::default(),
            crate::agent::body::needs::PsychologicalDrives::default(),
            crate::agent::actions::ActiveActions::default(),
            crate::agent::psyche::emotions::EmotionalState::default(),
            crate::agent::skills::Skills::default(),
        ))
        .id();

    commands.entity(entity).with_children(|parent| {
        parent.spawn((
            Text2d::new(deer_name(index)),
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
