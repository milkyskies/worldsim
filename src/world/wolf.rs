//! Wolf spawning logic — carnivorous pack predator entity.
//!
//! Reads: SpeciesProfile::wolf(), Ontology
//! Writes: Wolf component, MindGraph (innate prey/threat knowledge), world entities
//! Upstream: world::spawner (calls spawn_wolf), world::map (biome placement)
//! Downstream: agent brains (fear/flee in humans/deer, anger/attack in wolves)

use crate::agent::biology::body::BodyNodeKind;
use crate::agent::body::genetics::founder::random_genome;
use crate::agent::body::needs::PsychologicalDrives;
use crate::agent::body::species::{Species, SpeciesProfile};
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, MindGraph, Ontology};
use crate::agent::naming::wolf_name;
use crate::agent::{Agent, Alive};
use crate::markings::{Markings, apply_markings};
use crate::palette::{Palette, PaletteColor};
use crate::silhouette::{CreatureSilhouette, PartRole, Shape, SilhouettePart};
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;
use rand::Rng;

/// Marker component for wolf entities.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Wolf;

/// Canonical wolf silhouette - long lean body, level snout-shaped head,
/// pointy alert ears, bushy tail. Lower-slung profile than a deer (wolves
/// carry their head level with the body, not raised).
pub fn wolf_silhouette() -> CreatureSilhouette {
    let fur = PaletteColor::FurGrey;
    let leg_fur = PaletteColor::FurSlate;
    let leg = |x: f32| SilhouettePart {
        body_node: None,
        shape: Shape::Capsule,
        size: Vec2::new(2.0, 5.5),
        offset: Vec2::new(x, -5.5),
        rotation: 0.0,
        color: leg_fur,
        z_bias: 0,
        role: PartRole::Limb,
        tint_with_environment: false,
    };
    let ear = |x: f32, y: f32| SilhouettePart {
        body_node: None,
        shape: Shape::Triangle,
        size: Vec2::new(2.5, 3.5),
        offset: Vec2::new(x, y),
        rotation: 0.0,
        color: fur,
        z_bias: 2,
        role: PartRole::Ear,
        tint_with_environment: false,
    };
    CreatureSilhouette {
        parts: vec![
            // Torso - lean, lower-slung, longer than tall.
            SilhouettePart {
                body_node: Some(BodyNodeKind::Torso),
                shape: Shape::Ellipse,
                size: Vec2::new(15.0, 6.5),
                offset: Vec2::new(0.0, -0.5),
                rotation: 0.0,
                color: fur,
                z_bias: 0,
                role: PartRole::Body,
                tint_with_environment: false,
            },
            // Scruff - short neck/shoulder hump connecting torso to head.
            SilhouettePart {
                body_node: None,
                shape: Shape::Capsule,
                size: Vec2::new(4.0, 4.0),
                offset: Vec2::new(5.5, 1.5),
                rotation: 0.0,
                color: fur,
                z_bias: 0,
                role: PartRole::Body,
                tint_with_environment: false,
            },
            // Head - elongated forward, wolf snout silhouette.
            SilhouettePart {
                body_node: Some(BodyNodeKind::Head),
                shape: Shape::Ellipse,
                size: Vec2::new(7.0, 4.5),
                offset: Vec2::new(9.5, 3.0),
                rotation: 0.0,
                color: fur,
                z_bias: 1,
                role: PartRole::Body,
                tint_with_environment: false,
            },
            // Dark snout tip.
            SilhouettePart {
                body_node: None,
                shape: Shape::Ellipse,
                size: Vec2::new(2.5, 1.8),
                offset: Vec2::new(12.5, 2.2),
                rotation: 0.0,
                color: PaletteColor::FurCharcoal,
                z_bias: 2,
                role: PartRole::Snout,
                tint_with_environment: false,
            },
            // Cute eye, forward on the head.
            SilhouettePart {
                body_node: None,
                shape: Shape::Circle,
                size: Vec2::new(1.6, 1.6),
                offset: Vec2::new(10.5, 3.7),
                rotation: 0.0,
                color: PaletteColor::FurBlack,
                z_bias: 2,
                role: PartRole::Eye,
                tint_with_environment: false,
            },
            ear(7.5, 6.0),
            ear(9.0, 6.5),
            // Bushy tail - bigger than the old teardrop, dropped slightly low.
            SilhouettePart {
                body_node: None,
                shape: Shape::Teardrop,
                size: Vec2::new(6.0, 4.0),
                offset: Vec2::new(-9.5, 0.5),
                rotation: 0.0,
                color: fur,
                z_bias: 0,
                role: PartRole::Tail,
                tint_with_environment: false,
            },
            // Front leg pair (under shoulders, x positive = head side).
            leg(3.5),
            leg(5.0),
            // Back leg pair (under hips, x negative = tail side).
            leg(-5.0),
            leg(-3.5),
        ],
        shadow_size: Vec2::new(14.0, 4.5),
        shadow_offset_y: -8.0,
        hop_phase: 0.0,
    }
}

/// Spawns a Wolf (Predator Agent)
pub fn spawn_wolf<R: Rng>(
    commands: &mut Commands,
    ontology: Ontology,
    palette: &Palette,
    position: Vec2,
    index: usize,
    rng: &mut R,
) -> Entity {
    let species_profile = SpeciesProfile::wolf();
    let inventory = ItemSlots::agent_carry();
    let genome = random_genome(rng, Species::Wolf);
    let markings = Markings::from_genome(&genome);
    let silhouette =
        apply_markings(wolf_silhouette(), &markings).with_hop_phase(index as f32 * 1.618);
    let name_tag_y = silhouette.top_y() + 16.0;

    let spawn_tile = (
        (position.x / TILE_SIZE) as i32,
        (position.y / TILE_SIZE) as i32,
    );

    let mut mind = MindGraph::new(ontology);
    add_wolf_knowledge(&mut mind, spawn_tile);

    let entity = commands
        .spawn((
            Name::new(wolf_name(index)),
            Agent,
            Alive,
            Wolf,
            EntityType(Concept::Wolf),
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
            // Vision range overwritten by develop_phenotype_system; placeholder = species baseline.
            crate::agent::mind::perception::Vision {
                range: SpeciesProfile::wolf().vision_range,
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
            PsychologicalDrives::default(),
            crate::agent::actions::ActiveActions::default(),
            crate::agent::psyche::emotions::EmotionalState::default(),
            crate::agent::skills::Skills::default(),
        ))
        .id();

    commands.entity(entity).with_children(|parent| {
        parent.spawn((
            Text2d::new(wolf_name(index)),
            TextFont {
                font_size: 8.0,
                ..default()
            },
            TextColor(palette.srgb(PaletteColor::BloodFresh)),
            Transform::from_translation(Vec3::new(0.0, name_tag_y, 1.0)),
            crate::ui::sprite_animation::NameTag::new(entity, name_tag_y),
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

    // Prey recognition: wolves are born knowing deer yield meat when killed.
    // (Meat IsA Food lives in the shared ontology — wolves don't need to assert it.)
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Deer),
        Predicate::Produces,
        Value::Item(Concept::Meat, 1),
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
