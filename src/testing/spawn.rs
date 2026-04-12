//! Logic-only entity spawning helpers used by TestWorld.
//!
//! Reads: agent components, knowledge types, world map
//! Writes: spawned entities (no sprites, no children, no name tags)
//! Upstream: testing::config::AgentConfig
//! Downstream: testing::world::TestWorld

use std::sync::Arc;

use bevy::prelude::*;

use crate::agent::actions::{ActionType, ActiveActions};
use crate::agent::affordance::Affordance;
use crate::agent::biology::body::Body;
use crate::agent::body::genetics::genome::Genome;
use crate::agent::body::needs::{
    Consciousness, PhysicalNeeds, PsychologicalDrives, SocialDriveOverride,
};
use crate::agent::body::species::SpeciesProfile;
use crate::agent::brains::plan_memory::PlanMemory;
use crate::agent::brains::proposal::BrainState;
use crate::agent::brains::rational::RationalBrain;
use crate::agent::culture::{Culture, create_cultural_knowledge};
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, MindGraph, Ontology};
use crate::agent::mind::memory::WorkingMemory;
use crate::agent::mind::perception::{VisibleObjects, Vision};
use crate::agent::mind::recognition::initialize_relationship_with_affection;
use crate::agent::movement::MovementState;
use crate::agent::naming::NameCounters;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::psyche::emotions::EmotionalState;
use crate::agent::skills::Skills;
use crate::agent::spawn_human::{PersonInit, build_person_logic};
use crate::agent::{Agent, TargetPosition};
use crate::testing::config::AgentConfig;
use crate::world::Physical;
use crate::world::apple_tree::ResourceRegeneration;
use crate::world::deer::Deer;
use crate::world::property::HarvestableComponent;
use crate::world::wolf::Wolf;

/// Spawns a Person agent with all logic components but no sprites/children/name tags.
///
/// Goes through the same `build_person_logic` helper as the real game spawner
/// in `world::human::spawn_person`, so brain-relevant components cannot drift
/// between the two paths (#306).
pub(super) fn spawn_test_person(
    world: &mut World,
    ontology: Ontology,
    config: AgentConfig,
) -> Entity {
    let display_name = config
        .name
        .clone()
        .unwrap_or_else(|| world.resource_mut::<NameCounters>().next_human());

    let cultural_knowledge = Arc::new(create_cultural_knowledge(Culture::default()));

    let social_drive_override = config.social_drive;

    let (core, perception, brain) = build_person_logic(
        PersonInit {
            name: display_name,
            position: config.pos,
            genome: config.genome,
            physical_needs: PhysicalNeeds {
                metabolism: config.metabolism.clone(),
                hydration: config.hydration,
                stamina: crate::agent::body::needs::Stamina {
                    aerobic: config.stamina,
                    ..Default::default()
                },
                health: 100.0,
                wakefulness: config.wakefulness,
                last_health_damage: None,
            },
            cultural_knowledge,
            extra_knowledge: config.knowledge,
        },
        ontology,
    );

    // Body is normally added by `setup_biology` on the next Update;
    // pre-insert it so brain queries that read `Option<&Body>` see it
    // immediately and tests can inspect injuries without an extra tick.
    let entity = world
        .spawn(core)
        .insert(perception)
        .insert(brain)
        .insert(Body::human())
        .id();

    if let Some(v) = social_drive_override {
        world.entity_mut(entity).insert(SocialDriveOverride(v));
    }

    entity
}

/// Spawns a Deer animal agent with all logic components but no visuals.
///
/// `genome` controls which phenotype, personality, and drives the deer ends up
/// with: `develop_phenotype_system` overwrites the placeholder `Personality`
/// and `PsychologicalDrives` from it on the first tick.
pub(super) fn spawn_test_deer(
    world: &mut World,
    ontology: Ontology,
    pos: Vec2,
    genome: Genome,
) -> Entity {
    use crate::agent::psyche::personality::Personality;

    let species = SpeciesProfile::deer();

    let mut mind = MindGraph::new(ontology);
    crate::world::deer::add_deer_knowledge(&mut mind);

    let display_name = world.resource_mut::<NameCounters>().next_deer();
    world
        .spawn((
            Name::new(display_name),
            Agent,
            Deer,
            EntityType(Concept::Deer),
            species,
            Physical,
            TargetPosition::default(),
            MovementState::default(),
            ItemSlots::agent_carry(),
            Personality::default(),
            genome,
            Transform::from_translation(pos.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            Affordance::default(),
            mind,
            Vision {
                range: SpeciesProfile::deer().vision_range,
            },
            VisibleObjects::default(),
        ))
        .insert((
            WorkingMemory::default(),
            RationalBrain,
            PlanMemory::default(),
            BrainState::default(),
            CentralNervousSystem::default(),
            PhysicalNeeds::default(),
            Consciousness::default(),
            PsychologicalDrives::default(),
            ActiveActions::default(),
            EmotionalState::default(),
            // Species-specific anatomy so channel queries against the deer
            // see real capacity numbers (no Manipulation, no Bite, four-leg
            // Locomotion). Pre-inserted for the same reason as humans —
            // tests shouldn't need to tick once before poking at the body.
            Body::deer(),
            Skills::default(),
        ))
        .id()
}

/// Spawns a Wolf predator agent with all logic components but no visuals.
pub(super) fn spawn_test_wolf(
    world: &mut World,
    ontology: Ontology,
    pos: Vec2,
    genome: Genome,
) -> Entity {
    use crate::agent::psyche::personality::Personality;
    use crate::world::map::TILE_SIZE;

    let spawn_tile = ((pos.x / TILE_SIZE) as i32, (pos.y / TILE_SIZE) as i32);
    let mut mind = MindGraph::new(ontology);
    crate::world::wolf::add_wolf_knowledge(&mut mind, spawn_tile);

    let display_name = world.resource_mut::<NameCounters>().next_wolf();
    world
        .spawn((
            Name::new(display_name),
            Agent,
            Wolf,
            EntityType(Concept::Wolf),
            SpeciesProfile::wolf(),
            Physical,
            TargetPosition::default(),
            MovementState::default(),
            ItemSlots::agent_carry(),
            Personality::default(),
            genome,
            Transform::from_translation(pos.extend(3.0)),
            GlobalTransform::default(),
        ))
        .insert((
            Affordance::default(),
            mind,
            Vision {
                range: SpeciesProfile::wolf().vision_range,
            },
            VisibleObjects::default(),
        ))
        .insert((
            WorkingMemory::default(),
            RationalBrain,
            PlanMemory::default(),
            BrainState::default(),
            CentralNervousSystem::default(),
            PhysicalNeeds::default(),
            Consciousness::default(),
            PsychologicalDrives::default(),
            ActiveActions::default(),
            EmotionalState::default(),
            // Wolf anatomy: four legs for fast Locomotion, jaws providing
            // Bite 1.0 + limited Manipulation 0.4. Pre-inserted so channel
            // lookups see it immediately.
            Body::wolf(),
            Skills::default(),
        ))
        .id()
}

/// Spawns a berry bush with the given starting berry count, no visuals.
pub(super) fn spawn_test_berry_bush(world: &mut World, pos: Vec2, berries: u32) -> Entity {
    let mut inventory = ItemSlots::agent_carry();
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
            HarvestableComponent {
                yields: Concept::Berry,
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

/// Spawns a stone node with the given starting stone count, no visuals.
pub(super) fn spawn_test_stone_node(world: &mut World, pos: Vec2, stones: u32) -> Entity {
    let mut inventory = ItemSlots::agent_carry();
    if stones > 0 {
        inventory.add(Concept::Stone, stones);
    }

    world
        .spawn((
            Name::new("TestStoneNode"),
            EntityType(Concept::StoneNode),
            Physical,
            Transform::from_translation(pos.extend(1.0)),
            GlobalTransform::default(),
            inventory,
            Affordance {
                action_type: ActionType::Harvest,
                cost: 6.0,
                distance: 28.0,
                risk: 0.0,
            },
            HarvestableComponent {
                yields: Concept::Stone,
            },
            ResourceRegeneration {
                timer: 0.0,
                interval: 60.0,
                item: Concept::Stone,
                max_amount: 8,
            },
        ))
        .id()
}

/// Spawns a wood log with the given starting wood count, no visuals.
pub(super) fn spawn_test_wood_log(world: &mut World, pos: Vec2, wood: u32) -> Entity {
    let mut inventory = ItemSlots::agent_carry();
    if wood > 0 {
        inventory.add(Concept::Wood, wood);
    }

    world
        .spawn((
            Name::new("TestWoodLog"),
            EntityType(Concept::WoodLog),
            Physical,
            Transform::from_translation(pos.extend(1.0)),
            GlobalTransform::default(),
            inventory,
            Affordance {
                action_type: ActionType::Harvest,
                cost: 4.0,
                distance: 24.0,
                risk: 0.0,
            },
            HarvestableComponent {
                yields: Concept::Wood,
            },
            ResourceRegeneration {
                timer: 0.0,
                interval: 45.0,
                item: Concept::Wood,
                max_amount: 6,
            },
        ))
        .id()
}

/// Spawns an apple tree with the given starting apple count, no visuals.
pub(super) fn spawn_test_apple_tree(world: &mut World, pos: Vec2, apples: u32) -> Entity {
    let mut inventory = ItemSlots::agent_carry();
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
            HarvestableComponent {
                yields: Concept::Apple,
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

/// Mutually introduce a set of kin (herd-mates, pack-mates) with a
/// pre-set affection level. Writes `Knows`, `Introduced`, `NameOf` and the
/// relationship dimensions from every member's MindGraph toward every
/// other member. Used by `TestWorld::apply_spawn_layout` to match what the
/// real game's spawner does when it places clustered species.
pub(crate) fn introduce_kin(
    world: &mut crate::testing::TestWorld,
    members: &[Entity],
    affection: f32,
) {
    let pairs: Vec<(Entity, String)> = members
        .iter()
        .map(|&e| {
            let name = world
                .app()
                .world()
                .get::<Name>(e)
                .map(|n| n.as_str().to_string())
                .unwrap_or_default();
            (e, name)
        })
        .collect();

    for (i, (entity_a, _)) in pairs.iter().enumerate() {
        for (j, (entity_b, name_b)) in pairs.iter().enumerate() {
            if i == j {
                continue;
            }
            if let Some(mut mind) = world.app_mut().world_mut().get_mut::<MindGraph>(*entity_a) {
                initialize_relationship_with_affection(&mut mind, *entity_b, name_b, 0, affection);
            }
        }
    }
}
