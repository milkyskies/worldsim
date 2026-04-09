//! Logic-only entity spawning helpers used by TestWorld.
//!
//! Reads: agent components, knowledge types, world map
//! Writes: spawned entities (no sprites, no children, no name tags)
//! Upstream: testing::config::AgentConfig
//! Downstream: testing::world::TestWorld

use bevy::prelude::*;

use crate::agent::actions::ActionType;
use crate::agent::actions::ActiveActions;
use crate::agent::affordance::Affordance;
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::body::species::SpeciesProfile;
use crate::agent::brains::proposal::BrainState;
use crate::agent::brains::rational::RationalBrain;
use crate::agent::inventory::{EntityType, Inventory};
use crate::agent::mind::knowledge::{Concept, MindGraph, Ontology};
use crate::agent::mind::memory::WorkingMemory;
use crate::agent::mind::perception::{VisibleObjects, Vision};
use crate::agent::movement::MovementState;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::psyche::emotions::EmotionalState;
use crate::agent::{Agent, Person, TargetPosition};
use crate::testing::config::AgentConfig;
use crate::world::Physical;
use crate::world::apple_tree::ResourceRegeneration;
use crate::world::deer::Deer;

/// Spawns a Person agent with all logic components but no sprites/children/name tags.
/// The MindGraph is initialized with the world Ontology and any pre-loaded knowledge.
pub(super) fn spawn_test_person(
    world: &mut World,
    ontology: Ontology,
    config: AgentConfig,
) -> Entity {
    let mut mind = MindGraph::new(ontology);
    for triple in config.knowledge {
        mind.assert(triple);
    }

    world
        .spawn((
            Name::new("TestPerson"),
            Agent,
            Person,
            EntityType(Concept::Person),
            SpeciesProfile::human(),
            Physical,
            TargetPosition::default(),
            MovementState::default(),
            Inventory::default(),
            config.personality,
            Transform::from_translation(config.pos.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            Affordance::default(),
            mind,
            Vision { range: 100.0 },
            VisibleObjects::default(),
        ))
        .insert((
            WorkingMemory::default(),
            RationalBrain::default(),
            BrainState::default(),
            CentralNervousSystem::default(),
            PhysicalNeeds {
                hunger: config.hunger,
                thirst: 0.0,
                energy: config.energy,
                health: 100.0,
            },
            Consciousness::default(),
            PsychologicalDrives {
                social: config.social_drive,
                ..Default::default()
            },
            ActiveActions::default(),
            EmotionalState::default(),
            // Body is normally added by `setup_biology` on the next Update;
            // pre-insert it so brain queries that read `Option<&Body>` see it
            // immediately and tests can inspect injuries without an extra tick.
            Body::default(),
        ))
        .id()
}

/// Spawns a Deer animal agent with all logic components but no visuals.
pub(super) fn spawn_test_deer(world: &mut World, ontology: Ontology, pos: Vec2) -> Entity {
    use crate::agent::mind::knowledge::{
        MemoryType, Metadata, Node, Predicate, Source, Triple, Value,
    };
    use crate::agent::psyche::personality::Personality;

    let mut mind = MindGraph::new(ontology);

    // Deer-specific innate knowledge: berries are food, persons are dangerous.
    let intrinsic = Metadata {
        source: Source::Intrinsic,
        memory_type: MemoryType::Intrinsic,
        timestamp: 0,
        confidence: 1.0,
        ..Default::default()
    };
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Berry),
        Predicate::IsA,
        Value::Concept(Concept::Food),
        intrinsic.clone(),
    ));
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::BerryBush),
        Predicate::Produces,
        Value::Item(Concept::Berry, 1),
        intrinsic.clone(),
    ));
    mind.assert(Triple::with_meta(
        Node::Concept(Concept::Person),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        intrinsic,
    ));

    world
        .spawn((
            Name::new("TestDeer"),
            Agent,
            Deer,
            EntityType(Concept::Deer),
            SpeciesProfile::deer(),
            Physical,
            TargetPosition::default(),
            MovementState::default(),
            Inventory::default(),
            Personality::default(),
            Transform::from_translation(pos.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            Affordance::default(),
            mind,
            Vision { range: 128.0 },
            VisibleObjects::default(),
        ))
        .insert((
            WorkingMemory::default(),
            RationalBrain::default(),
            BrainState::default(),
            CentralNervousSystem::default(),
            PhysicalNeeds::default(),
            Consciousness::default(),
            ActiveActions::default(),
            EmotionalState::default(),
        ))
        .id()
}

/// Spawns a berry bush with the given starting berry count, no visuals.
pub(super) fn spawn_test_berry_bush(world: &mut World, pos: Vec2, berries: u32) -> Entity {
    let mut inventory = Inventory::default();
    if berries > 0 {
        inventory.add(Concept::Berry, berries);
    }

    world
        .spawn((
            Name::new("TestBerryBush"),
            EntityType(Concept::BerryBush),
            Physical,
            Transform::from_translation(pos.extend(1.0)),
            GlobalTransform::default(),
            inventory,
            Affordance {
                action_type: ActionType::Harvest,
                cost: 3.0,
                distance: 24.0,
                risk: 0.0,
            },
            ResourceRegeneration {
                timer: 0.0,
                interval: 8.0,
                item: Concept::Berry,
                max_amount: 15,
            },
        ))
        .id()
}

/// Spawns an apple tree with the given starting apple count, no visuals.
pub(super) fn spawn_test_apple_tree(world: &mut World, pos: Vec2, apples: u32) -> Entity {
    let mut inventory = Inventory::default();
    if apples > 0 {
        inventory.add(Concept::Apple, apples);
    }

    world
        .spawn((
            Name::new("TestAppleTree"),
            EntityType(Concept::AppleTree),
            Physical,
            Transform::from_translation(pos.extend(1.0)),
            GlobalTransform::default(),
            inventory,
            Affordance {
                action_type: ActionType::Harvest,
                cost: 5.0,
                distance: 32.0,
                risk: 0.0,
            },
            ResourceRegeneration {
                timer: 0.0,
                interval: 10.0,
                item: Concept::Apple,
                max_amount: 20,
            },
        ))
        .id()
}
