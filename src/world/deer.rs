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
use crate::markings::{Markings, apply_markings};
use crate::palette::PaletteColor;
use crate::silhouette::{CreatureSilhouette, PartRole, Shape, SilhouettePart};
use bevy::prelude::*;
use rand::Rng;

/// Marker component for deer entities.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Deer;

/// Canonical deer silhouette - Bambi-flavored: slender body, head + snout
/// up and forward, tall thin legs in two pairs (front/back), perky ears,
/// small upturned tail.
pub fn deer_silhouette() -> CreatureSilhouette {
    let fur = PaletteColor::SkinDark;
    let leg_fur = PaletteColor::SkinDeep;
    let leg = |x: f32| SilhouettePart {
        body_node: None,
        shape: Shape::Capsule,
        size: Vec2::new(1.4, 6.0),
        offset: Vec2::new(x, -6.0),
        rotation: 0.0,
        color: leg_fur,
        z_bias: 0,
        role: PartRole::Limb,
        tint_with_environment: false,
    };
    let ear = |x: f32, y: f32| SilhouettePart {
        body_node: None,
        shape: Shape::Triangle,
        size: Vec2::new(1.8, 3.5),
        offset: Vec2::new(x, y),
        rotation: 0.0,
        color: fur,
        z_bias: 2,
        role: PartRole::Ear,
        tint_with_environment: false,
    };
    CreatureSilhouette {
        parts: vec![
            // Torso - slender, longer than tall, slightly elevated.
            SilhouettePart {
                body_node: Some(BodyNodeKind::Torso),
                shape: Shape::Ellipse,
                size: Vec2::new(13.0, 5.5),
                offset: Vec2::new(0.0, -1.0),
                rotation: 0.0,
                color: fur,
                z_bias: 0,
                role: PartRole::Body,
                tint_with_environment: false,
            },
            // Neck rising forward-up from front of torso to base of head.
            SilhouettePart {
                body_node: None,
                shape: Shape::Capsule,
                size: Vec2::new(2.5, 5.0),
                offset: Vec2::new(5.5, 2.5),
                rotation: 0.0,
                color: fur,
                z_bias: 0,
                role: PartRole::Body,
                tint_with_environment: false,
            },
            // Head - elongated ellipse pointing forward (snout-shaped).
            SilhouettePart {
                body_node: Some(BodyNodeKind::Head),
                shape: Shape::Ellipse,
                size: Vec2::new(5.5, 3.5),
                offset: Vec2::new(8.0, 5.5),
                rotation: 0.0,
                color: fur,
                z_bias: 1,
                role: PartRole::Body,
                tint_with_environment: false,
            },
            // Snout tip slightly darker.
            SilhouettePart {
                body_node: None,
                shape: Shape::Ellipse,
                size: Vec2::new(2.2, 1.6),
                offset: Vec2::new(10.5, 4.8),
                rotation: 0.0,
                color: PaletteColor::SkinDeep,
                z_bias: 2,
                role: PartRole::Snout,
                tint_with_environment: false,
            },
            // Big cute eye, forward on the head.
            SilhouettePart {
                body_node: None,
                shape: Shape::Circle,
                size: Vec2::new(1.4, 1.4),
                offset: Vec2::new(9.0, 6.2),
                rotation: 0.0,
                color: PaletteColor::FurBlack,
                z_bias: 2,
                role: PartRole::Eye,
                tint_with_environment: false,
            },
            ear(6.5, 8.5),
            ear(8.0, 9.0),
            // Tail - small upturned tuft at the back.
            SilhouettePart {
                body_node: None,
                shape: Shape::Teardrop,
                size: Vec2::new(2.2, 2.2),
                offset: Vec2::new(-7.0, 1.0),
                rotation: 0.0,
                color: PaletteColor::FurWhite,
                z_bias: 1,
                role: PartRole::Tail,
                tint_with_environment: false,
            },
            // Front leg pair (under shoulders, x positive = head side).
            leg(3.5),
            leg(5.0),
            // Back leg pair (under hips, x negative = tail side).
            leg(-4.5),
            leg(-3.0),
        ],
        shadow_size: Vec2::new(14.0, 4.0),
        shadow_offset_y: -8.5,
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
    let markings = Markings::from_genome(&genome);
    let silhouette =
        apply_markings(deer_silhouette(), &markings).with_hop_phase(index as f32 * 1.618);
    let name_tag_y = silhouette.top_y() + 16.0;

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
            markings,
            silhouette,
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
            Transform::from_translation(Vec3::new(0.0, name_tag_y, 1.0)),
            crate::ui::sprite_animation::NameTag::new(entity, name_tag_y),
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
