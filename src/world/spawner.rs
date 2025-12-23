//! Unified Spawner: The single source of truth for creating entities in the world.
//! Ensures consistent ECS components + Knowledge Graph assertions.
//!
//! Individual entity spawning logic is delegated to:
//! - `human.rs` - Person/Agent spawning
//! - `apple_tree.rs` - Apple Tree spawning
//! - `berry_bush.rs` - Berry Bush spawning
//! - `deer.rs` - Deer spawning

use crate::agent::mind::knowledge::Ontology;
use bevy::prelude::*;

// Re-export spawning functions for convenience
pub use super::apple_tree::{
    ResourceRegeneration, VisualApple, VisualLeaves, regenerate_resources, spawn_apple_tree,
    sync_apple_visuals,
};
pub use super::berry_bush::{VisualBerry, VisualBushLeaves, spawn_berry_bush, sync_berry_visuals};
pub use super::deer::{Deer, spawn_deer};
pub use super::human::spawn_person;

pub struct SpawnerPlugin;

impl Plugin for SpawnerPlugin {
    fn build(&self, app: &mut App) {
        // Initialize the shared Ontology resource
        app.insert_resource(crate::agent::mind::knowledge::setup_ontology());

        app.register_type::<ResourceRegeneration>()
            .register_type::<Deer>()
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
    use rand::Rng;
    use std::collections::HashMap;
    use std::sync::Arc;
    let mut rng = rand::rng();

    // Precompute cultural knowledge
    let mut cultural_knowledge_map = HashMap::new();
    let all_cultures = [
        crate::agent::culture::Culture::Nomad,
        crate::agent::culture::Culture::Farmer,
        crate::agent::culture::Culture::Hunter,
        crate::agent::culture::Culture::Gatherer,
    ];
    for culture in all_cultures {
        let triples = crate::agent::culture::create_cultural_knowledge(culture);
        cultural_knowledge_map.insert(culture, Arc::new(triples));
    }

    // Spawn human agents
    for i in 0..32 {
        // Find a valid spawn location
        let mut spawn_pos = None;
        for _ in 0..50 {
            let x = rng.random_range(0.0..(map.width as f32 * crate::world::map::TILE_SIZE));
            let y = rng.random_range(0.0..(map.height as f32 * crate::world::map::TILE_SIZE));
            let test_pos = Vec2::new(x, y);

            if map.is_walkable(test_pos) {
                spawn_pos = Some(test_pos);
                break;
            }
        }

        if let Some(pos) = spawn_pos {
            // Random Culture
            let culture = all_cultures[rng.random_range(0..all_cultures.len())];
            let knowledge = cultural_knowledge_map.get(&culture).unwrap().clone();

            spawn_person(&mut commands, ontology.clone(), pos, i, culture, knowledge);
        }
    }

    // Spawn Apple Trees
    for _ in 0..24 {
        if let Some(pos) = find_spawn_location(&map, &mut rng) {
            spawn_apple_tree(&mut commands, pos, 5);
        }
    }

    // Spawn Berry Bushes
    for _ in 0..32 {
        if let Some(pos) = find_spawn_location(&map, &mut rng) {
            spawn_berry_bush(&mut commands, pos, 4);
        }
    }

    // Spawn Deer
    for i in 0..0 {
        if let Some(pos) = find_spawn_location(&map, &mut rng) {
            spawn_deer(&mut commands, ontology.clone(), pos, i);
        }
    }
}

/// Helper function to find a valid spawn location
fn find_spawn_location(
    map: &crate::world::map::WorldMap,
    rng: &mut impl rand::Rng,
) -> Option<Vec2> {
    for _ in 0..50 {
        let x = rng.random_range(0.0..(map.width as f32 * crate::world::map::TILE_SIZE));
        let y = rng.random_range(0.0..(map.height as f32 * crate::world::map::TILE_SIZE));
        let test_pos = Vec2::new(x, y);

        if map.is_walkable(test_pos) {
            return Some(test_pos);
        }
    }
    None
}
