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

use crate::agent::mind::knowledge::Ontology;
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
                if let Some(mut social) =
                    world.get_mut::<crate::agent::mind::social_identity::SocialIdentity>(*entity_a)
                {
                    social.introduce(
                        *entity_b,
                        crate::agent::mind::knowledge::AgentName(name_b.clone()),
                        0,
                    );
                }
                if let Some(mut graph) =
                    world.get_resource_mut::<crate::agent::psyche::social_graph::SocialGraph>()
                {
                    crate::agent::mind::recognition::init_relationship_dimensions(
                        &mut graph, *entity_a, *entity_b, 0, affection,
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
pub use super::fish::{Fish, Minnow, Pike, spawn_minnow, spawn_pike};
pub use super::human::spawn_person;
pub use super::sapling::{Sapling, grow_saplings, spawn_sapling};
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
            .register_type::<Sapling>()
            .register_type::<Deer>()
            .register_type::<Wolf>()
            .register_type::<Fish>()
            .register_type::<Minnow>()
            .register_type::<Pike>()
            .add_systems(
                OnEnter(AppState::InSim),
                (
                    spawn_initial_population
                        .after(crate::world::map::setup_map)
                        .run_if(in_debug_mode_or_adventure),
                    setup_wolf_pack_bonds
                        .after(spawn_initial_population)
                        .run_if(in_debug_mode_or_adventure),
                    spawn_game_scaffold
                        .after(crate::world::map::setup_map)
                        .run_if(in_game_mode),
                    // Adventure mode reuses the Debug population, then
                    // possesses one of the spawned humans. Marker insertion
                    // runs strictly after spawn so the Person query has a
                    // non-empty candidate set.
                    possess_first_person_for_adventure
                        .after(spawn_initial_population)
                        .run_if(in_adventure_mode),
                ),
            )
            .add_systems(FixedUpdate, regenerate_resources)
            .add_systems(FixedUpdate, grow_saplings)
            .add_systems(
                Update,
                (
                    sync_apple_visuals,
                    sync_berry_visuals,
                    sync_stone_visuals,
                    sync_wood_visuals,
                    crate::world::construction_site::sync_construction_site_visuals,
                ),
            );
    }
}

fn spawn_initial_population(
    mut commands: Commands,
    map: Res<crate::world::map::WorldMap>,
    ontology: Res<Ontology>,
    palette: Res<crate::palette::Palette>,
    mut sim_rng: ResMut<crate::core::SimRng>,
    sim_config: Option<Res<SimConfig>>,
) {
    let seed = sim_config.map(|c| c.seed as u64).unwrap_or(0);
    let config = WorldSpawnConfig {
        seed,
        ..WorldSpawnConfig::game_defaults()
    };
    let layout = config.compute_layout(&map);
    let spawned = apply_layout(
        &mut commands,
        &ontology,
        &palette,
        &layout,
        sim_rng.inner_mut(),
    );
    for entity in spawned {
        commands
            .entity(entity)
            .insert(DespawnOnExit(AppState::InSim));
    }
}

/// Run condition: the sandbox population spawns for both Debug and
/// Adventure. Adventure reuses Debug's spawn entirely and then possesses
/// one of the resulting humans; without this combined gate the Adventure
/// run would land on an empty map.
fn in_debug_mode_or_adventure(sim_config: Option<Res<SimConfig>>) -> bool {
    sim_config
        .map(|c| matches!(c.mode, SimMode::Debug | SimMode::Adventure))
        .unwrap_or(true)
}

/// Run condition: game-mode scaffold path. Future factory-game systems will
/// reuse this same condition to gate themselves to game runs.
fn in_game_mode(sim_config: Option<Res<SimConfig>>) -> bool {
    sim_config
        .map(|c| matches!(c.mode, SimMode::Game))
        .unwrap_or(false)
}

/// Run condition: only Adventure mode possesses an agent on entry.
fn in_adventure_mode(sim_config: Option<Res<SimConfig>>) -> bool {
    sim_config
        .map(|c| matches!(c.mode, SimMode::Adventure))
        .unwrap_or(false)
}

/// Mark the first spawned human as `PlayerControlled` and pin the
/// character sheet to them so the player sees their own stats from
/// turn one without having to left-click themselves.
///
/// The selection is arbitrary — Adventure-mode spawn placement currently
/// makes every human equally suitable as the starting body. A later
/// "pick which agent to possess" UI can replace this with explicit
/// player choice.
fn possess_first_person_for_adventure(
    mut commands: Commands,
    candidates: Query<Entity, With<crate::agent::Person>>,
    // `Option` so headless / TestWorld runs that don't init UiState
    // still take the possess path without panicking.
    ui_state: Option<ResMut<crate::ui::UiState>>,
) {
    let Some(entity) = candidates.iter().next() else {
        // Empty population would silently leave Adventure mode without
        // a body — surface it loudly so we notice the miswire.
        warn!("Adventure mode entered but no Person was spawned to possess");
        return;
    };
    crate::agent::player::possess(&mut commands, entity);
    if let Some(mut ui_state) = ui_state {
        // `add = false` replaces the selection rather than appending,
        // so any stale entity from a previous run gets cleared.
        ui_state.selected_entities.select_maybe_add(entity, false);
    }
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
    palette: &crate::palette::Palette,
    layout: &SpawnLayout,
    rng: &mut impl rand::Rng,
) -> Vec<Entity> {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::agent::culture::Culture;

    let mut spawned: Vec<Entity> = Vec::new();

    // First group stays on the original settlement side; second group spawns
    // across the river. Cultures are split so the two groups have different
    // starting knowledge and drift further apart behaviorally over time: the
    // first group wanders and farms, the second hunts deer for meat.
    let first_group_cultures = [Culture::Nomad, Culture::Farmer];
    let second_group_cultures = [Culture::Hunter];

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
                let entity = spawn_wolf(commands, ontology.clone(), palette, pos, wolf_index, rng);
                wolf_index += 1;
                entity
            })
            .collect();
        spawned.extend(members.iter().copied());
        if members.len() > 1 {
            introduce_group_as_kin(commands, members, KIN_BASELINE_AFFECTION);
        }
    }

    let mut minnow_index = 0;
    for school in &layout.minnow_schools {
        let members: Vec<Entity> = school
            .iter()
            .map(|&pos| {
                let entity = spawn_minnow(commands, ontology.clone(), pos, minnow_index, rng);
                minnow_index += 1;
                entity
            })
            .collect();
        spawned.extend(members.iter().copied());
        if members.len() > 1 {
            introduce_group_as_kin(commands, members, KIN_BASELINE_AFFECTION);
        }
    }

    for (i, &pos) in layout.pike_positions.iter().enumerate() {
        spawned.push(spawn_pike(commands, ontology.clone(), pos, i, rng));
    }

    for &(pos, berries) in &layout.berry_bush_positions {
        spawned.push(spawn_berry_bush(commands, palette, pos, berries));
    }

    for &(pos, apples) in &layout.apple_tree_positions {
        spawned.push(spawn_apple_tree(commands, palette, pos, apples));
    }

    for &(pos, stones) in &layout.stone_node_positions {
        spawned.push(spawn_stone_node(commands, palette, pos, stones));
    }

    for &(pos, wood) in &layout.wood_log_positions {
        spawned.push(spawn_wood_log(commands, palette, pos, wood));
    }

    spawned
}

/// Establishes mutual pack bonds between all spawned wolves.
///
/// Runs once at Startup after `spawn_initial_population`. Each wolf learns every
/// other wolf as a high-trust friend — the same relationship mechanism humans use
/// for family or close community members.
fn setup_wolf_pack_bonds(
    mut social_graph: ResMut<crate::agent::psyche::social_graph::SocialGraph>,
    mut wolf_query: Query<(Entity, &mut crate::agent::mind::knowledge::MindGraph), With<Wolf>>,
) {
    use crate::agent::mind::knowledge::{Concept, Metadata, Node, Predicate, Triple, Value};
    use crate::agent::psyche::social_graph::RelationshipEdge;

    let wolves: Vec<Entity> = wolf_query.iter().map(|(e, _)| e).collect();
    if wolves.len() < 2 {
        return;
    }

    let meta = Metadata::default(); // Source::Intrinsic

    for (entity, mut mind) in wolf_query.iter_mut() {
        for &packmate in wolves.iter().filter(|&&e| e != entity) {
            // Friend classification stays in the mind graph — it's
            // categorical and used by recognition-based code paths.
            mind.assert(Triple::with_meta(
                Node::Entity(packmate),
                Predicate::IsA,
                Value::Concept(Concept::Friend),
                meta.clone(),
            ));
            // Quantitative trust/affection live on the canonical edge.
            social_graph.set(
                entity,
                packmate,
                RelationshipEdge {
                    affection: 0.8,
                    trust: 0.9,
                    ..Default::default()
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Person;
    use crate::agent::player::PlayerControlled;

    /// Drives `possess_first_person_for_adventure` against a tiny world
    /// holding a Person marker. Asserts the system inserts
    /// `PlayerControlled` on it. Without this guarantee Adventure mode
    /// would land in a sandbox where every agent is still AI-driven.
    #[test]
    fn possess_marker_lands_on_a_person_when_one_exists() {
        let mut app = App::new();
        let person = app.world_mut().spawn(Person).id();
        let _bystander_with_no_person = app.world_mut().spawn_empty().id();

        let sys_id = app.register_system(possess_first_person_for_adventure);
        app.world_mut().run_system(sys_id).unwrap();
        app.update();

        assert!(
            app.world().get::<PlayerControlled>(person).is_some(),
            "the spawned Person should now carry PlayerControlled"
        );
    }

    /// Empty population must not silently leave Adventure mode without a
    /// body. The system warns and returns; no entity is marked. We assert
    /// the negative side of the invariant: no `PlayerControlled` exists.
    #[test]
    fn possess_is_a_no_op_when_no_person_was_spawned() {
        let mut app = App::new();
        let stranger = app.world_mut().spawn_empty().id();

        let sys_id = app.register_system(possess_first_person_for_adventure);
        app.world_mut().run_system(sys_id).unwrap();
        app.update();

        assert!(
            app.world().get::<PlayerControlled>(stranger).is_none(),
            "no PlayerControlled should be inserted when no Person exists"
        );
    }
}
