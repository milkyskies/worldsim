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
pub use super::wolf::{Wolf, spawn_wolf};

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
                spawn_initial_population.after(crate::world::map::setup_map),
            )
            .add_systems(
                Update,
                (regenerate_resources, sync_apple_visuals, sync_berry_visuals),
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
    use rand::Rng;
    use std::collections::HashMap;
    use std::sync::Arc;
    let mut rng = rand::rng();

    let all_cultures = [
        crate::agent::culture::Culture::Nomad,
        crate::agent::culture::Culture::Farmer,
        crate::agent::culture::Culture::Hunter,
        crate::agent::culture::Culture::Gatherer,
    ];
    let mut cultural_knowledge_map = HashMap::new();
    for culture in all_cultures {
        let triples = crate::agent::culture::create_cultural_knowledge(culture);
        cultural_knowledge_map.insert(culture, Arc::new(triples));
    }

    for (i, &pos) in layout.human_positions.iter().enumerate() {
        let culture = all_cultures[rng.random_range(0..all_cultures.len())];
        let knowledge = cultural_knowledge_map.get(&culture).unwrap().clone();
        spawn_person(commands, ontology.clone(), pos, i, culture, knowledge);
    }

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
}
