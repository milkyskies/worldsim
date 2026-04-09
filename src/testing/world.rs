//! TestWorld: a Bevy `App` configured with all simulation logic plugins but no rendering or input.
//!
//! Reads: AgentPlugin, agent components, knowledge ontology, world map types
//! Writes: TestWorld (App wrapper exposing spawn/tick/inspect APIs)
//! Upstream: testing::config (AgentConfig), testing::spawn (logic-only spawners)
//! Downstream: integration tests (scenario, brain, knowledge, planner, perception)

use bevy::math::IVec2;
use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::AgentPlugin;
use crate::agent::actions::{ActionRegistry, ActionType, ActiveActions};
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::mind::knowledge::{
    Concept, MindGraph, Node as MindNode, Ontology, Predicate, Value, setup_ontology,
};
use crate::core::tick::TickCount;
use crate::core::{GameLog, GameTime};
use crate::testing::config::AgentConfig;
use crate::testing::spawn::{
    spawn_test_apple_tree, spawn_test_berry_bush, spawn_test_deer, spawn_test_person,
};
use crate::world::environment::LightLevel;
use crate::world::map::{CHUNK_SIZE, Chunk, WorldMap};

/// Default test world dimensions in tiles. Large enough for typical scenarios but
/// small enough that map construction is cheap (a few KB).
const DEFAULT_MAP_TILES: u32 = 64;

/// A lightweight headless simulation harness. Wraps a Bevy `App` configured with
/// the same logic plugins as the real game (`AgentPlugin` and friends) but
/// without rendering, windowing, input, UI, or world spawn population.
///
/// The seed parameter is captured for forward compatibility with deterministic
/// RNG once the simulation is refactored to use a seeded RNG resource. Today,
/// individual tests should rely on explicit `AgentConfig` values for
/// reproducibility rather than implicit randomness.
pub struct TestWorld {
    app: App,
    seed: u64,
}

impl TestWorld {
    /// Creates a new TestWorld with seed 0.
    pub fn new() -> Self {
        Self::with_seed(0)
    }

    /// Creates a new TestWorld with the given RNG seed.
    pub fn with_seed(seed: u64) -> Self {
        let mut app = App::new();

        // MinimalPlugins gives us TaskPool, Time, ScheduleRunner — no rendering.
        app.add_plugins(MinimalPlugins);

        // Resources normally provided by plugins we deliberately exclude:
        // - SpawnerPlugin (Ontology, plus startup population we don't want)
        // - MapPlugin (WorldMap, plus tile sprite spawning)
        // - EnvironmentPlugin (LightLevel, plus ClearColor manipulation)
        // - CorePlugin (TickCount/GameLog/GameTime, plus keyboard time controls)
        app.insert_resource(setup_ontology());
        app.insert_resource(make_walkable_map(DEFAULT_MAP_TILES, DEFAULT_MAP_TILES));
        app.insert_resource(LightLevel(1.0));
        app.insert_resource(TickCount::new(60.0));
        app.insert_resource(GameLog::new(100));
        app.init_resource::<GameTime>();

        // Replace tick_system with a deterministic per-update tick advancer so
        // each `app.update()` advances exactly one logical tick regardless of
        // wall-clock delta. This is critical for reproducible tests.
        app.add_systems(Update, deterministic_tick);

        // Adds biology, brains, nervous_system, mind systems, action registry,
        // conversation manager, relationship config, etc.
        app.add_plugins(AgentPlugin);

        Self { app, seed }
    }

    /// The seed this TestWorld was created with.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    // ─── Spawning ──────────────────────────────────────────────────────────

    /// Spawns a Person agent with the given config.
    pub fn spawn_agent(&mut self, config: AgentConfig) -> Entity {
        let ontology = self.app.world().resource::<Ontology>().clone();
        spawn_test_person(self.app.world_mut(), ontology, config)
    }

    /// Spawns `n` agents in a small grid centered on `near`. Useful for crowd
    /// scenarios; returns the entities in spawn order.
    pub fn spawn_agent_cluster(&mut self, n: usize, near: Vec2) -> Vec<Entity> {
        // Lay out as a square grid with ~16 px spacing.
        let side = (n as f32).sqrt().ceil() as usize;
        let spacing = 16.0;
        let mut entities = Vec::with_capacity(n);
        for i in 0..n {
            let row = (i / side) as f32;
            let col = (i % side) as f32;
            let offset = Vec2::new(col * spacing, row * spacing);
            let center_offset = Vec2::new(side as f32 * spacing * 0.5, side as f32 * spacing * 0.5);
            let pos = near + offset - center_offset;
            entities.push(self.spawn_agent(AgentConfig::at(pos)));
        }
        entities
    }

    /// Spawns a deer (animal agent) at the given position.
    pub fn spawn_deer(&mut self, pos: Vec2) -> Entity {
        let ontology = self.app.world().resource::<Ontology>().clone();
        spawn_test_deer(self.app.world_mut(), ontology, pos)
    }

    /// Spawns a berry bush at the given position with the specified berry count.
    pub fn spawn_berry_bush(&mut self, pos: Vec2, berries: u32) -> Entity {
        spawn_test_berry_bush(self.app.world_mut(), pos, berries)
    }

    /// Spawns an apple tree at the given position with the specified apple count.
    pub fn spawn_apple_tree(&mut self, pos: Vec2, apples: u32) -> Entity {
        spawn_test_apple_tree(self.app.world_mut(), pos, apples)
    }

    /// Sets a tile type at the given tile coordinates.
    pub fn set_tile(&mut self, x: u32, y: u32, tile: crate::world::map::TileType) {
        self.app
            .world_mut()
            .resource_mut::<WorldMap>()
            .set_tile(x, y, tile);
    }

    // ─── Simulation ────────────────────────────────────────────────────────

    /// Advances the simulation by `n` ticks. Each tick is one full Bevy `update()`
    /// pass with all logic systems running.
    pub fn tick(&mut self, n: u64) {
        for _ in 0..n {
            self.app.update();
        }
    }

    /// Returns the current tick count.
    pub fn current_tick(&self) -> u64 {
        self.app.world().resource::<TickCount>().current
    }

    // ─── Inspection ────────────────────────────────────────────────────────

    /// Returns the underlying Bevy `App` for advanced introspection. Prefer the
    /// typed helpers below for common queries.
    pub fn app(&self) -> &App {
        &self.app
    }

    /// Returns the underlying Bevy `App` for advanced mutation. Prefer the typed
    /// helpers below for common operations.
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Borrows a component from an entity. Panics if missing — tests should know
    /// what they spawned.
    pub fn get<T: Component>(&self, entity: Entity) -> &T {
        self.app.world().get::<T>(entity).unwrap_or_else(|| {
            panic!(
                "entity {entity:?} missing component {}",
                std::any::type_name::<T>()
            )
        })
    }

    /// Mutably borrows a component from an entity. Panics if missing.
    pub fn get_mut<T: Component<Mutability = bevy::ecs::component::Mutable>>(
        &mut self,
        entity: Entity,
    ) -> Mut<'_, T> {
        let type_name = std::any::type_name::<T>();
        self.app
            .world_mut()
            .get_mut::<T>(entity)
            .unwrap_or_else(|| panic!("entity {entity:?} missing component {type_name}"))
    }

    /// Returns true if the entity still exists in the world.
    pub fn entity_exists(&self, entity: Entity) -> bool {
        self.app.world().get_entity(entity).is_ok()
    }

    /// Returns the Euclidean distance between two entities' Transforms in 2D.
    /// Panics if either entity lacks a Transform.
    pub fn distance(&self, a: Entity, b: Entity) -> f32 {
        let pos_a = self.get::<Transform>(a).translation.truncate();
        let pos_b = self.get::<Transform>(b).translation.truncate();
        pos_a.distance(pos_b)
    }

    /// Returns all agent entities currently in the world.
    pub fn all_agents(&mut self) -> Vec<Entity> {
        let world = self.app.world_mut();
        let mut query = world.query_filtered::<Entity, With<Agent>>();
        query.iter(world).collect()
    }

    // ─── Convenience queries ───────────────────────────────────────────────

    /// True if `agent` has at least one Knows triple about `other`.
    pub fn agent_knows(&self, agent: Entity, other: Entity) -> bool {
        let mind = self.get::<MindGraph>(agent);
        !mind
            .query(
                Some(&MindNode::Self_),
                Some(Predicate::Knows),
                Some(&Value::Entity(other)),
            )
            .is_empty()
            || !mind
                .query(Some(&MindNode::Entity(other)), Some(Predicate::Knows), None)
                .is_empty()
    }

    /// Returns the trust value `agent` has toward `other`, or 0.0 if no triple exists.
    pub fn agent_trust(&self, agent: Entity, other: Entity) -> f32 {
        let mind = self.get::<MindGraph>(agent);
        mind.query(Some(&MindNode::Entity(other)), Some(Predicate::Trust), None)
            .into_iter()
            .find_map(|t| match &t.object {
                Value::Float(f) => Some(*f),
                _ => None,
            })
            .unwrap_or(0.0)
    }

    /// Returns the agent's hunger value (0.0–100.0).
    pub fn agent_hunger(&self, agent: Entity) -> f32 {
        self.get::<PhysicalNeeds>(agent).hunger
    }

    /// Returns the agent's thirst value (0.0–100.0).
    pub fn agent_thirst(&self, agent: Entity) -> f32 {
        self.get::<PhysicalNeeds>(agent).thirst
    }

    /// Returns the agent's energy value (0.0–100.0).
    pub fn agent_energy(&self, agent: Entity) -> f32 {
        self.get::<PhysicalNeeds>(agent).energy
    }

    /// Returns true if the entity carries any of the given concept in its inventory.
    pub fn has_item(&self, entity: Entity, concept: Concept) -> bool {
        self.app
            .world()
            .get::<crate::agent::inventory::Inventory>(entity)
            .map(|inv| inv.has(concept))
            .unwrap_or(false)
    }

    /// Returns the count of `concept` in the entity's inventory, or 0 if missing.
    pub fn item_count(&self, entity: Entity, concept: Concept) -> u32 {
        self.app
            .world()
            .get::<crate::agent::inventory::Inventory>(entity)
            .map(|inv| inv.count(concept))
            .unwrap_or(0)
    }

    /// Returns the action type the agent is currently executing. Returns
    /// `Some(Idle)` when the agent has no active action. With parallel
    /// channels, this reports the *primary* (highest-intensity) running action.
    pub fn current_action(&self, agent: Entity) -> Option<ActionType> {
        let world = self.app.world();
        let active = world.get::<ActiveActions>(agent)?;
        let registry = world.resource::<ActionRegistry>();
        Some(
            active
                .primary(registry)
                .map(|s| s.action_type)
                .unwrap_or(ActionType::Idle),
        )
    }

    /// Returns true if the action registry contains an entry for the given action.
    /// Useful for catching test setup mistakes.
    pub fn has_registered_action(&self, action: ActionType) -> bool {
        self.app
            .world()
            .resource::<ActionRegistry>()
            .get(action)
            .is_some()
    }
}

impl Default for TestWorld {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Builds a fully walkable WorldMap of the given dimensions in tiles. Initializes
/// every chunk with grass so `is_walkable` returns true everywhere.
fn make_walkable_map(width: u32, height: u32) -> WorldMap {
    let mut map = WorldMap::new(width, height);
    let chunks_x = width.div_ceil(CHUNK_SIZE);
    let chunks_y = height.div_ceil(CHUNK_SIZE);
    for cy in 0..chunks_y as i32 {
        for cx in 0..chunks_x as i32 {
            map.chunks.insert(IVec2::new(cx, cy), Chunk::new(cx, cy));
        }
    }
    map
}

/// Replaces `core::tick::tick_system` for tests: increments TickCount.current by
/// exactly one per update, regardless of real-time delta. Also drives GameTime.
fn deterministic_tick(mut tick: ResMut<TickCount>, mut game_time: ResMut<GameTime>) {
    if tick.paused {
        return;
    }
    tick.current += 1;
    game_time.update_from_tick(tick.current);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Person;

    #[test]
    fn new_world_starts_at_tick_zero() {
        let world = TestWorld::new();
        assert_eq!(world.current_tick(), 0);
    }

    #[test]
    fn tick_advances_logical_tick_count() {
        let mut world = TestWorld::new();
        world.tick(10);
        assert_eq!(world.current_tick(), 10);
    }

    #[test]
    fn spawn_agent_creates_person_with_logic_components() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig::default());

        // Core markers
        assert!(world.app().world().get::<Person>(agent).is_some());
        assert!(world.app().world().get::<Agent>(agent).is_some());

        // Logic components needed for the brain pipeline
        assert!(world.app().world().get::<MindGraph>(agent).is_some());
        assert!(world.app().world().get::<PhysicalNeeds>(agent).is_some());
        assert!(
            world
                .app()
                .world()
                .get::<crate::agent::brains::proposal::BrainState>(agent)
                .is_some()
        );
    }

    #[test]
    fn spawn_agent_uses_config_values() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig {
            pos: Vec2::new(50.0, 75.0),
            hunger: 80.0,
            energy: 25.0,
            ..Default::default()
        });

        assert_eq!(world.agent_hunger(agent), 80.0);
        assert_eq!(world.agent_energy(agent), 25.0);
        let transform = world.get::<Transform>(agent);
        assert_eq!(transform.translation.x, 50.0);
        assert_eq!(transform.translation.y, 75.0);
    }

    #[test]
    fn spawn_agent_cluster_returns_n_agents_near_center() {
        let mut world = TestWorld::new();
        let center = Vec2::new(100.0, 100.0);
        let agents = world.spawn_agent_cluster(9, center);
        assert_eq!(agents.len(), 9);

        // All agents should be within a small radius of the center.
        for agent in &agents {
            let pos = world.get::<Transform>(*agent).translation.truncate();
            assert!(pos.distance(center) < 50.0);
        }
    }

    #[test]
    fn spawn_berry_bush_starts_with_berry_inventory() {
        let mut world = TestWorld::new();
        let bush = world.spawn_berry_bush(Vec2::new(10.0, 10.0), 5);
        assert_eq!(world.item_count(bush, Concept::Berry), 5);
    }

    #[test]
    fn spawn_apple_tree_starts_with_apple_inventory() {
        let mut world = TestWorld::new();
        let tree = world.spawn_apple_tree(Vec2::new(20.0, 20.0), 7);
        assert_eq!(world.item_count(tree, Concept::Apple), 7);
    }

    #[test]
    fn spawn_deer_creates_agent_with_dangerous_person_belief() {
        let mut world = TestWorld::new();
        let deer = world.spawn_deer(Vec2::new(40.0, 40.0));
        assert!(world.app().world().get::<Agent>(deer).is_some());
        assert!(
            world
                .app()
                .world()
                .get::<crate::world::deer::Deer>(deer)
                .is_some()
        );

        // The deer should know persons are dangerous (loaded at spawn).
        let mind = world.get::<MindGraph>(deer);
        let dangerous = mind.query(
            Some(&MindNode::Concept(Concept::Person)),
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Dangerous)),
        );
        assert!(
            !dangerous.is_empty(),
            "deer should believe Person is Dangerous"
        );
    }

    #[test]
    fn distance_returns_euclidean_distance_between_entities() {
        let mut world = TestWorld::new();
        let a = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
        let b = world.spawn_agent(AgentConfig::at(Vec2::new(3.0, 4.0)));
        assert_eq!(world.distance(a, b), 5.0);
    }

    #[test]
    fn entity_exists_reflects_world_state() {
        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig::default());
        assert!(world.entity_exists(agent));

        world.app_mut().world_mut().despawn(agent);
        assert!(!world.entity_exists(agent));
    }

    #[test]
    fn all_agents_returns_only_agent_marker_entities() {
        let mut world = TestWorld::new();
        let person = world.spawn_agent(AgentConfig::default());
        let deer = world.spawn_deer(Vec2::new(20.0, 20.0));
        let _bush = world.spawn_berry_bush(Vec2::new(30.0, 30.0), 3);

        let agents = world.all_agents();
        assert_eq!(agents.len(), 2);
        assert!(agents.contains(&person));
        assert!(agents.contains(&deer));
    }

    #[test]
    fn registered_actions_include_core_action_set() {
        let world = TestWorld::new();
        for action in [
            ActionType::Eat,
            ActionType::Sleep,
            ActionType::Walk,
            ActionType::Harvest,
            ActionType::Wander,
            ActionType::Talk,
        ] {
            assert!(
                world.has_registered_action(action),
                "expected {action:?} to be registered"
            );
        }
    }

    #[test]
    fn config_with_pre_loaded_knowledge_is_applied_to_mind() {
        use crate::agent::mind::knowledge::{Metadata, Triple};

        let mut world = TestWorld::new();
        let agent = world.spawn_agent(AgentConfig {
            knowledge: vec![Triple::with_meta(
                MindNode::Concept(Concept::AppleTree),
                Predicate::Produces,
                Value::Item(Concept::Apple, 1),
                Metadata::semantic(0),
            )],
            ..Default::default()
        });

        let mind = world.get::<MindGraph>(agent);
        let triples = mind.query(
            Some(&MindNode::Concept(Concept::AppleTree)),
            Some(Predicate::Produces),
            None,
        );
        assert!(
            !triples.is_empty(),
            "pre-loaded AppleTree-produces-Apple knowledge should be present"
        );
    }

    #[test]
    fn ticking_runs_brain_pipeline_without_panicking() {
        // This is the smoke test that proves the full system stack is wired up.
        // A bare agent with default needs should be tickable for many frames
        // without any system panicking on missing resources or components.
        let mut world = TestWorld::new();
        let _ = world.spawn_agent(AgentConfig {
            hunger: 50.0,
            ..Default::default()
        });
        world.spawn_apple_tree(Vec2::new(20.0, 20.0), 10);
        world.tick(30);
        assert_eq!(world.current_tick(), 30);
    }
}
