//! Unified Spawner: The single source of truth for creating entities in the world.
//! Ensures consistent ECS components + Knowledge Graph assertions.
//!
//! Reads: WorldMap, Ontology, WorldSpawnConfig (layout computation)
//! Writes: Person, Deer, Wolf, BerryBush, AppleTree entities (initial population)
//! Upstream: world::map (terrain), world::spawn_config (placement layout)
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

use crate::agent::mind::knowledge::Ontology;
use crate::world::spawn_config::{SpawnLayout, WorldSpawnConfig};
use bevy::prelude::*;

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
                Startup,
                (
                    spawn_initial_population.after(crate::world::map::setup_map),
                    setup_wolf_pack_bonds.after(spawn_initial_population),
                ),
            )
            .add_systems(
                Update,
                (
                    regenerate_resources,
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
) {
    let config = WorldSpawnConfig::game_defaults();
    let layout = config.compute_layout(&map);
    apply_layout(&mut commands, &ontology, &layout);
}

/// Spawns all entities described by `layout` into the Bevy world using full
/// visual spawners. Used by the windowed game path.
pub fn apply_layout(commands: &mut Commands, ontology: &Ontology, layout: &SpawnLayout) {
    use crate::agent::culture::Culture;
    use rand::Rng;
    use std::collections::HashMap;
    use std::sync::Arc;
    let mut rng = rand::rng();

    // First group stays on the original settlement side; second group spawns
    // across the river. Cultures are split so the two groups have different
    // starting knowledge and drift further apart behaviorally over time.
    let first_group_cultures = [Culture::Nomad, Culture::Farmer];
    let second_group_cultures = [Culture::Hunter, Culture::Gatherer];

    let mut cultural_knowledge_map = HashMap::new();
    for culture in first_group_cultures
        .iter()
        .chain(second_group_cultures.iter())
    {
        let triples = crate::agent::culture::create_cultural_knowledge(*culture);
        cultural_knowledge_map.insert(*culture, Arc::new(triples));
    }

    let mut spawn_group = |positions: &[Vec2], pool: &[Culture], index_offset: usize| {
        for (i, &pos) in positions.iter().enumerate() {
            let culture = pool[rng.random_range(0..pool.len())];
            let knowledge = cultural_knowledge_map.get(&culture).unwrap().clone();
            spawn_person(
                commands,
                ontology.clone(),
                pos,
                index_offset + i,
                culture,
                knowledge,
            );
        }
    };

    spawn_group(&layout.human_positions, &first_group_cultures, 0);
    spawn_group(
        &layout.second_human_positions,
        &second_group_cultures,
        layout.human_positions.len(),
    );

    for (i, &pos) in layout.deer_positions.iter().enumerate() {
        spawn_deer(commands, ontology.clone(), pos, i);
    }

    for (i, &pos) in layout.wolf_positions.iter().enumerate() {
        spawn_wolf(commands, ontology.clone(), pos, i);
    }

    for &(pos, berries) in &layout.berry_bush_positions {
        spawn_berry_bush(commands, pos, berries);
    }

    for &(pos, apples) in &layout.apple_tree_positions {
        spawn_apple_tree(commands, pos, apples);
    }

    for &(pos, stones) in &layout.stone_node_positions {
        spawn_stone_node(commands, pos, stones);
    }

    for &(pos, wood) in &layout.wood_log_positions {
        spawn_wood_log(commands, pos, wood);
    }
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
