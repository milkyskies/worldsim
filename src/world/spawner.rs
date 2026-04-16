//! Unified Spawner: The single source of truth for creating entities in the world.
//! Ensures consistent ECS components + Knowledge Graph assertions.
//!
//! Reads: WorldMap, Ontology, SimConfig (mode + seed), WorldSpawnConfig (layout computation)
//! Writes: Person, Deer, Wolf, BerryBush, AppleTree entities (initial population)
//! Upstream: world::map (terrain), world::spawn_config (placement layout), menu (SimConfig)
//! Downstream: agent systems consume the resulting entities
//!
//! Individual entity spawning logic is delegated to:
//! - `human.rs` - Person/Agent spawning
//! - `apple_tree.rs` - Apple Tree spawning
//! - `berry_bush.rs` - Berry Bush spawning
//! - `deer.rs` - Deer spawning
//! - `wolf.rs` - Wolf spawning
//! - `stone_node.rs` - Stone Node spawning
//! - `wood_log.rs` - Wood Log spawning

use crate::agent::mind::knowledge::{MindGraph, Ontology};
use crate::agent::mind::recognition::initialize_relationship_with_affection;
use crate::menu::{AppState, SimConfig, SimMode};
use crate::world::spawn_config::{SpawnLayout, WorldSpawnConfig};
use bevy::prelude::*;

/// Initial affection value written from each herd-mate's mind toward every
/// other herd-mate at spawn. Kin-level: well above the 0.5 neutral
/// stranger default, so the proximity-decay pathway in the flocking system
/// pulls them strongly toward each other.
const KIN_BASELINE_AFFECTION: f32 = 0.8;

/// Schedule mutual introductions between the members of a spawned herd or
/// pack. The entity IDs are valid immediately after spawn but the
/// `MindGraph` components are only accessible once the command buffer
/// flushes, so we queue a deferred closure to do the actual writes.
fn introduce_group_as_kin(commands: &mut Commands, members: Vec<Entity>, affection: f32) {
    commands.queue(move |world: &mut World| {
        let pairs: Vec<(Entity, String)> = members
            .iter()
            .map(|&e| {
                let name = world
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
                if let Some(mut mind) = world.get_mut::<MindGraph>(*entity_a) {
                    initialize_relationship_with_affection(
                        &mut mind, *entity_b, name_b, 0, affection,
                    );
                }
            }
        }
    });
}

// Re-export spawning functions for convenience
pub use super::apple_tree::{
    ResourceRegeneration, VisualApple, VisualLeaves, regenerate_resources, spawn_apple_tree,
    sync_apple_visuals,
};
pub use super::berry_bush::{VisualBerry, VisualBushLeaves, spawn_berry_bush, sync_berry_visuals};
pub use super::deer::{Deer, spawn_deer};
pub use super::human::spawn_person;
pub use super::stone_node::{
    StoneNodeMarker, VisualStoneChunk, spawn_stone_node, sync_stone_visuals,
};
pub use super::wolf::{Wolf, spawn_wolf};
pub use super::wood_log::{VisualWoodPiece, WoodLogMarker, spawn_wood_log, sync_wood_visuals};

pub struct SpawnerPlugin;

impl Plugin for SpawnerPlugin {
    fn build(&self, app: &mut App) {
        // Initialize the shared Ontology resource
        app.insert_resource(crate::agent::mind::knowledge::setup_ontology());

        app.register_type::<ResourceRegeneration>()
            .register_type::<Deer>()
            .register_type::<Wolf>()
            .add_systems(
                OnEnter(AppState::InSim),
                (
                    spawn_initial_population
                        .after(crate::world::map::setup_map)
                        .run_if(in_debug_mode),
                    setup_wolf_pack_bonds
                        .after(spawn_initial_population)
                        .run_if(in_debug_mode),
                    spawn_game_scaffold
                        .after(crate::world::map::setup_map)
                        .run_if(in_game_mode),
                ),
            )
            .add_systems(FixedUpdate, regenerate_resources)
            .add_systems(
                Update,
                (
                    sync_apple_visuals,
                    sync_berry_visuals,
                    sync_stone_visuals,
                    sync_wood_visuals,
                ),
            );
    }
}

fn spawn_initial_population(
    mut commands: Commands,
    map: Res<crate::world::map::WorldMap>,
    ontology: Res<Ontology>,
    mut sim_rng: ResMut<crate::core::SimRng>,
    sim_config: Option<Res<SimConfig>>,
) {
    let seed = sim_config.map(|c| c.seed as u64).unwrap_or(0);
    let config = WorldSpawnConfig {
        seed,
        ..WorldSpawnConfig::game_defaults()
    };
    let layout = config.compute_layout(&map);
    let spawned = apply_layout(&mut commands, &ontology, &layout, sim_rng.inner_mut());
    for entity in spawned {
        commands
            .entity(entity)
            .insert(DespawnOnExit(AppState::InSim));
    }
}

/// Run condition: spawn the full sandbox population only when the player picked Debug.
fn in_debug_mode(sim_config: Option<Res<SimConfig>>) -> bool {
    sim_config
        .map(|c| matches!(c.mode, SimMode::Debug))
        .unwrap_or(true)
}

/// Run condition: game-mode scaffold path. Future factory-game systems will
/// reuse this same condition to gate themselves to game runs.
fn in_game_mode(sim_config: Option<Res<SimConfig>>) -> bool {
    sim_config
        .map(|c| matches!(c.mode, SimMode::Game))
        .unwrap_or(false)
}

/// Intentionally empty placeholder for the factory-game entry point.
/// Picking Game today gives you terrain with no agents, visibly distinct
/// from Debug. Future factory features hook in here.
fn spawn_game_scaffold() {}

/// Spawns all entities described by `layout` into the Bevy world using full
/// visual spawners. Used by the windowed game path.
///
/// Returns the list of root entities created so callers can tag them with
/// state-scoped cleanup or other lifetime markers.
pub fn apply_layout(
    commands: &mut Commands,
    ontology: &Ontology,
    layout: &SpawnLayout,
    rng: &mut impl rand::Rng,
) -> Vec<Entity> {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::agent::culture::Culture;

    let mut spawned: Vec<Entity> = Vec::new();

    // First group stays on the original settlement side; second group spawns
    // across the river. Cultures are split so the two groups have different
    // starting knowledge and drift further apart behaviorally over time.
    // Hunter is intentionally excluded until #225 (hunting loop) lands — its
    // knowledge currently points at an action chain that doesn't exist.
    let first_group_cultures = [Culture::Nomad, Culture::Farmer];
    let second_group_cultures = [Culture::Gatherer];

    let mut cultural_knowledge_map = HashMap::new();
    for culture in first_group_cultures
        .iter()
        .chain(second_group_cultures.iter())
    {
        let triples = crate::agent::culture::create_cultural_knowledge(*culture);
        cultural_knowledge_map.insert(*culture, Arc::new(triples));
    }

    for (i, &pos) in layout.human_positions.iter().enumerate() {
        let culture = first_group_cultures[rng.random_range(0..first_group_cultures.len())];
        let knowledge = cultural_knowledge_map.get(&culture).unwrap().clone();
        let entity = spawn_person(commands, ontology.clone(), pos, i, culture, knowledge, rng);
        spawned.push(entity);
    }

    let offset = layout.human_positions.len();
    for (i, &pos) in layout.second_human_positions.iter().enumerate() {
        let culture = second_group_cultures[rng.random_range(0..second_group_cultures.len())];
        let knowledge = cultural_knowledge_map.get(&culture).unwrap().clone();
        let entity = spawn_person(
            commands,
            ontology.clone(),
            pos,
            offset + i,
            culture,
            knowledge,
            rng,
        );
        spawned.push(entity);
    }

    let mut deer_index = 0;
    for herd in &layout.deer_herds {
        let members: Vec<Entity> = herd
            .iter()
            .map(|&pos| {
                let entity = spawn_deer(commands, ontology.clone(), pos, deer_index, rng);
                deer_index += 1;
                entity
            })
            .collect();
        spawned.extend(members.iter().copied());
        if members.len() > 1 {
            introduce_group_as_kin(commands, members, KIN_BASELINE_AFFECTION);
        }
    }

    let mut wolf_index = 0;
    for pack in &layout.wolf_packs {
        let members: Vec<Entity> = pack
            .iter()
            .map(|&pos| {
                let entity = spawn_wolf(commands, ontology.clone(), pos, wolf_index, rng);
                wolf_index += 1;
                entity
            })
            .collect();
        spawned.extend(members.iter().copied());
        if members.len() > 1 {
            introduce_group_as_kin(commands, members, KIN_BASELINE_AFFECTION);
        }
    }

    for &(pos, berries) in &layout.berry_bush_positions {
        spawned.push(spawn_berry_bush(commands, pos, berries));
    }

    for &(pos, apples) in &layout.apple_tree_positions {
        spawned.push(spawn_apple_tree(commands, pos, apples));
    }

    for &(pos, stones) in &layout.stone_node_positions {
        spawned.push(spawn_stone_node(commands, pos, stones));
    }

    for &(pos, wood) in &layout.wood_log_positions {
        spawned.push(spawn_wood_log(commands, pos, wood));
    }

    spawned
}

/// Establishes mutual pack bonds between all spawned wolves.
///
/// Runs once at Startup after `spawn_initial_population`. Each wolf learns every
/// other wolf as a high-trust friend — the same relationship mechanism humans use
/// for family or close community members.
fn setup_wolf_pack_bonds(
    mut wolf_query: Query<(Entity, &mut crate::agent::mind::knowledge::MindGraph), With<Wolf>>,
) {
    use crate::agent::mind::knowledge::{Concept, Metadata, Node, Predicate, Triple, Value};

    let wolves: Vec<Entity> = wolf_query.iter().map(|(e, _)| e).collect();
    if wolves.len() < 2 {
        return;
    }

    let meta = Metadata::default(); // Source::Intrinsic

    for (entity, mut mind) in wolf_query.iter_mut() {
        for &packmate in wolves.iter().filter(|&&e| e != entity) {
            mind.assert(Triple::with_meta(
                Node::Entity(packmate),
                Predicate::IsA,
                Value::Concept(Concept::Friend),
                meta.clone(),
            ));
            mind.assert(Triple::with_meta(
                Node::Entity(packmate),
                Predicate::Trust,
                Value::Float(0.9),
                meta.clone(),
            ));
            mind.assert(Triple::with_meta(
                Node::Entity(packmate),
                Predicate::Affection,
                Value::Float(0.8),
                meta.clone(),
            ));
        }
    }
}
