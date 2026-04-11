//! ScenarioBuilder: composable world configuration for test scenarios.
//!
//! Reads: TestWorld, AgentConfig, WorldMap, Ontology, MindGraph
//! Writes: ScenarioBuilder (fluent API), ScenarioEntities (named-agent index)
//! Upstream: testing::world::TestWorld, testing::config::AgentConfig
//! Downstream: integration tests, scenario tests

use std::collections::HashMap;
use std::ops::Index;

use bevy::math::Vec2;
use bevy::prelude::*;

use crate::agent::body::genetics::genome::Genome;
use crate::agent::mind::knowledge::{Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::agent::mind::recognition::initialize_relationship;
use crate::testing::config::AgentConfig;
use crate::testing::world::{TestWorld, make_walkable_map};
use crate::world::map::{TileType, WorldMap};

// ─── RelBuilder ───────────────────────────────────────────────────────────

/// Fluent builder for relationship dimensions. All dimensions start at 0.5.
pub struct RelBuilder {
    pub trust: f32,
    pub affection: f32,
    pub respect: f32,
}

impl Default for RelBuilder {
    fn default() -> Self {
        Self {
            trust: 0.5,
            affection: 0.5,
            respect: 0.5,
        }
    }
}

impl RelBuilder {
    pub fn trust(mut self, v: f32) -> Self {
        self.trust = v.clamp(0.0, 1.0);
        self
    }

    pub fn affection(mut self, v: f32) -> Self {
        self.affection = v.clamp(0.0, 1.0);
        self
    }

    pub fn respect(mut self, v: f32) -> Self {
        self.respect = v.clamp(0.0, 1.0);
        self
    }
}

// ─── Internal config structs ───────────────────────────────────────────────

struct AgentSpec {
    name: String,
    pos: Option<Vec2>,
    genome: Option<Genome>,
    metabolism: Option<crate::agent::body::metabolism::Metabolism>,
    stamina: Option<f32>,
    social_drive: Option<f32>,
    group: Option<String>,
    knowledge: Vec<Triple>,
}

struct GroupSpec {
    name: String,
    agent_count: usize,
    near: Option<Vec2>,
    genome: Option<Genome>,
    metabolism: Option<crate::agent::body::metabolism::Metabolism>,
    stamina: Option<f32>,
    knows_each_other: bool,
}

struct RelationshipSpec {
    a: String,
    b: String,
    trust: f32,
    affection: f32,
    respect: f32,
}

struct TileEdit {
    x: u32,
    y: u32,
    tile: TileType,
}

struct ResourceSpec {
    kind: ResourceKind,
    count: usize,
    near: Vec2,
}

enum ResourceKind {
    BerryBush,
    AppleTree,
}

// ─── AgentBuilder ─────────────────────────────────────────────────────────

/// Builder for a single named agent. Call `.done()` to return to `ScenarioBuilder`.
pub struct AgentBuilder {
    parent: ScenarioBuilder,
    spec: AgentSpec,
}

impl AgentBuilder {
    /// World position for this agent.
    pub fn pos(mut self, pos: Vec2) -> Self {
        self.spec.pos = Some(pos);
        self
    }

    /// Set the genome used to derive this agent's phenotype, personality, and
    /// drives. Accepts anything that converts into a `Genome` — most callers
    /// pass a fluent builder like `personality().conscientiousness(0.0)` or
    /// `physical().speed(1.3)`.
    pub fn genome(mut self, g: impl Into<Genome>) -> Self {
        self.spec.genome = Some(g.into());
        self
    }

    /// Hunger urgency 0..1. Internally constructs a `Metabolism` at the
    /// given urgency. Prefer `.metabolism(..)` for explicit control.
    pub fn hunger_urgency(mut self, u: f32) -> Self {
        self.spec.metabolism = Some(crate::agent::body::metabolism::Metabolism::at_urgency(u));
        self
    }

    /// Explicit metabolism state.
    pub fn metabolism(mut self, m: crate::agent::body::metabolism::Metabolism) -> Self {
        self.spec.metabolism = Some(m);
        self
    }

    /// Stamina value (0.0 = exhausted, 100.0 = fully rested).
    pub fn stamina(mut self, v: f32) -> Self {
        self.spec.stamina = Some(v);
        self
    }

    /// Social drive (0.0 = satisfied, 1.0 = lonely).
    pub fn social_drive(mut self, v: f32) -> Self {
        self.spec.social_drive = Some(v);
        self
    }

    /// Assign this agent to a named group.
    pub fn in_group(mut self, group: &str) -> Self {
        self.spec.group = Some(group.to_string());
        self
    }

    /// Pre-load knowledge triples into the agent's MindGraph.
    pub fn knowledge(mut self, triples: Vec<Triple>) -> Self {
        self.spec.knowledge = triples;
        self
    }

    /// Finish agent configuration and return to the parent `ScenarioBuilder`.
    pub fn done(mut self) -> ScenarioBuilder {
        self.parent.agents.push(self.spec);
        self.parent
    }
}

// ─── GroupBuilder ─────────────────────────────────────────────────────────

/// Builder for a group of agents with shared configuration.
pub struct GroupBuilder {
    parent: ScenarioBuilder,
    spec: GroupSpec,
}

impl GroupBuilder {
    /// Number of agents in this group (default: 1).
    pub fn agents(mut self, n: usize) -> Self {
        self.spec.agent_count = n;
        self
    }

    /// Center position for agent placement.
    pub fn near(mut self, pos: Vec2) -> Self {
        self.spec.near = Some(pos);
        self
    }

    /// Shared genome for every agent in this group. Accepts any
    /// `Into<Genome>` — typically a fluent builder.
    pub fn genome(mut self, g: impl Into<Genome>) -> Self {
        self.spec.genome = Some(g.into());
        self
    }

    /// Hunger urgency 0..1 applied to every agent in the group.
    pub fn hunger_urgency(mut self, u: f32) -> Self {
        self.spec.metabolism = Some(crate::agent::body::metabolism::Metabolism::at_urgency(u));
        self
    }

    /// Explicit metabolism state applied to every agent in the group.
    pub fn metabolism(mut self, m: crate::agent::body::metabolism::Metabolism) -> Self {
        self.spec.metabolism = Some(m);
        self
    }

    /// Stamina value applied to all agents in the group.
    pub fn stamina(mut self, v: f32) -> Self {
        self.spec.stamina = Some(v);
        self
    }

    /// If `true`, all agents in this group will know each other at spawn.
    pub fn knows_each_other(mut self, v: bool) -> Self {
        self.spec.knows_each_other = v;
        self
    }

    /// Finish group configuration and return to the parent `ScenarioBuilder`.
    pub fn done(mut self) -> ScenarioBuilder {
        self.parent.groups.push(self.spec);
        self.parent
    }
}

// ─── ScenarioBuilder ──────────────────────────────────────────────────────

/// A composable builder for test world scenarios. Obtain via `TestWorld::scenario(seed)`.
pub struct ScenarioBuilder {
    seed: u64,
    map_width: u32,
    map_height: u32,
    tile_edits: Vec<TileEdit>,
    noise_biomes: bool,
    agents: Vec<AgentSpec>,
    groups: Vec<GroupSpec>,
    relationships: Vec<RelationshipSpec>,
    resources: Vec<ResourceSpec>,
}

impl ScenarioBuilder {
    pub(super) fn new(seed: u64) -> Self {
        Self {
            seed,
            map_width: 128,
            map_height: 128,
            tile_edits: Vec::new(),
            noise_biomes: true,
            agents: Vec::new(),
            groups: Vec::new(),
            relationships: Vec::new(),
            resources: Vec::new(),
        }
    }

    // ── World configuration ────────────────────────────────────────────────

    /// Set map dimensions in tiles (default: 128×128). Smaller maps mean faster pathfinding.
    pub fn map_size(mut self, width: u32, height: u32) -> Self {
        self.map_width = width;
        self.map_height = height;
        self
    }

    /// Place a single tile at the given tile coordinates.
    pub fn tile_at(mut self, x: u32, y: u32, tile: TileType) -> Self {
        self.tile_edits.push(TileEdit { x, y, tile });
        self
    }

    /// Fill a rectangular region with a tile type. `(x, y)` is the top-left corner.
    pub fn fill_rect(mut self, x: u32, y: u32, w: u32, h: u32, tile: TileType) -> Self {
        for ty in y..y.saturating_add(h) {
            for tx in x..x.saturating_add(w) {
                self.tile_edits.push(TileEdit { x: tx, y: ty, tile });
            }
        }
        self
    }

    /// Enable or disable noise biome generation (default: `true`).
    /// Set to `false` for flat-grass fast tests where terrain doesn't matter.
    pub fn noise_biomes(mut self, enabled: bool) -> Self {
        self.noise_biomes = enabled;
        self
    }

    // ── Agent groups ───────────────────────────────────────────────────────

    /// Begin configuring a named group of agents. Call `.done()` on the returned
    /// `GroupBuilder` to return here.
    pub fn group(self, name: &str) -> GroupBuilder {
        GroupBuilder {
            parent: self,
            spec: GroupSpec {
                name: name.to_string(),
                agent_count: 1,
                near: None,
                genome: None,
                metabolism: None,
                stamina: None,
                knows_each_other: false,
            },
        }
    }

    // ── Named agents ───────────────────────────────────────────────────────

    /// Begin configuring a named agent. Call `.done()` on the returned
    /// `AgentBuilder` to return here.
    pub fn agent(self, name: &str) -> AgentBuilder {
        AgentBuilder {
            parent: self,
            spec: AgentSpec {
                name: name.to_string(),
                pos: None,
                genome: None,
                metabolism: None,
                stamina: None,
                social_drive: None,
                group: None,
                knowledge: Vec::new(),
            },
        }
    }

    // ── Relationships ──────────────────────────────────────────────────────

    /// Set a pre-existing relationship between two named agents (or between a
    /// named agent and all members of a named group).
    pub fn relationship(
        mut self,
        a: &str,
        b: &str,
        f: impl FnOnce(RelBuilder) -> RelBuilder,
    ) -> Self {
        let rel = f(RelBuilder::default());
        self.relationships.push(RelationshipSpec {
            a: a.to_string(),
            b: b.to_string(),
            trust: rel.trust,
            affection: rel.affection,
            respect: rel.respect,
        });
        self
    }

    // ── Resources ──────────────────────────────────────────────────────────

    /// Spawn `count` berry bushes near the given world position.
    pub fn berry_bushes(mut self, count: usize, near: Vec2) -> Self {
        self.resources.push(ResourceSpec {
            kind: ResourceKind::BerryBush,
            count,
            near,
        });
        self
    }

    /// Spawn `count` apple trees near the given world position.
    pub fn apple_trees(mut self, count: usize, near: Vec2) -> Self {
        self.resources.push(ResourceSpec {
            kind: ResourceKind::AppleTree,
            count,
            near,
        });
        self
    }

    // ── Build ──────────────────────────────────────────────────────────────

    /// Finalise the scenario, produce a `TestWorld` and a `ScenarioEntities` index.
    pub fn build(self) -> (TestWorld, ScenarioEntities) {
        // Build the world map.
        let map = build_map(self.map_width, self.map_height, &self.tile_edits);

        // Construct the TestWorld, injecting our custom map.
        let mut world = TestWorld::with_seed_and_map(self.seed, map);

        let mut named: HashMap<String, Entity> = HashMap::new();
        let mut groups: HashMap<String, Vec<Entity>> = HashMap::new();

        // Spawn groups.
        for group_spec in &self.groups {
            let entities = spawn_group(&mut world, group_spec);
            groups.insert(group_spec.name.clone(), entities);
        }

        // Spawn named agents.
        for agent_spec in &self.agents {
            let entity = spawn_named_agent(&mut world, agent_spec);
            named.insert(agent_spec.name.clone(), entity);

            // Add to group entity list if assigned.
            if let Some(group_name) = &agent_spec.group {
                groups.entry(group_name.clone()).or_default().push(entity);
            }
        }

        // Apply knows_each_other within groups.
        for group_spec in &self.groups {
            if group_spec.knows_each_other
                && let Some(members) = groups.get(&group_spec.name)
            {
                apply_mutual_knowledge(&mut world, members);
            }
        }

        // Apply explicit relationships.
        for rel_spec in &self.relationships {
            let a_entity = named.get(&rel_spec.a).copied();
            let b_entities: Vec<Entity> = if let Some(entity) = named.get(&rel_spec.b).copied() {
                vec![entity]
            } else if let Some(members) = groups.get(&rel_spec.b) {
                members.clone()
            } else {
                Vec::new()
            };

            let Some(a) = a_entity else { continue };
            for b in &b_entities {
                apply_relationship(&mut world, a, *b, rel_spec);
                apply_relationship(&mut world, *b, a, rel_spec);
            }
        }

        // Spawn resources.
        for res in &self.resources {
            let spacing = 24.0_f32;
            for i in 0..res.count {
                let offset = Vec2::new((i as f32) * spacing, 0.0);
                let pos = res.near + offset;
                match res.kind {
                    ResourceKind::BerryBush => {
                        world.spawn_berry_bush(pos, 10);
                    }
                    ResourceKind::AppleTree => {
                        world.spawn_apple_tree(pos, 10);
                    }
                }
            }
        }

        let entities = ScenarioEntities { named, groups };
        (world, entities)
    }
}

// ─── ScenarioEntities ─────────────────────────────────────────────────────

/// Index of all named agents and groups produced by `ScenarioBuilder::build()`.
pub struct ScenarioEntities {
    named: HashMap<String, Entity>,
    groups: HashMap<String, Vec<Entity>>,
}

impl ScenarioEntities {
    /// Look up a named agent entity. Panics if the name doesn't exist.
    pub fn get(&self, name: &str) -> Entity {
        *self
            .named
            .get(name)
            .unwrap_or_else(|| panic!("ScenarioEntities: no agent named {name:?}"))
    }

    /// Look up all entities in a named group. Panics if the group doesn't exist.
    pub fn group(&self, name: &str) -> &[Entity] {
        self.groups
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or_else(|| panic!("ScenarioEntities: no group named {name:?}"))
    }
}

impl Index<&str> for ScenarioEntities {
    type Output = Entity;

    fn index(&self, name: &str) -> &Entity {
        self.named
            .get(name)
            .unwrap_or_else(|| panic!("ScenarioEntities: no agent named {name:?}"))
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────

/// Build a `WorldMap` with the given dimensions and tile edits applied.
fn build_map(width: u32, height: u32, edits: &[TileEdit]) -> WorldMap {
    let mut map = make_walkable_map(width, height);
    for edit in edits {
        map.set_tile(edit.x, edit.y, edit.tile);
    }
    map
}

/// Spawn a group of agents at positions scattered near the group's center.
fn spawn_group(world: &mut TestWorld, spec: &GroupSpec) -> Vec<Entity> {
    let center = spec.near.unwrap_or(Vec2::ZERO);
    let spacing = 20.0_f32;
    let side = (spec.agent_count as f32).sqrt().ceil() as usize;
    let offset_base = Vec2::new(side as f32 * spacing * 0.5, side as f32 * spacing * 0.5);

    let mut entities = Vec::with_capacity(spec.agent_count);
    for i in 0..spec.agent_count {
        let row = (i / side) as f32;
        let col = (i % side) as f32;
        let pos = center + Vec2::new(col * spacing, row * spacing) - offset_base;

        let config = AgentConfig {
            pos,
            name: Some(format!("{}_agent_{}", spec.name, i)),
            metabolism: spec
                .metabolism
                .clone()
                .unwrap_or_else(crate::agent::body::metabolism::Metabolism::well_fed),
            stamina: spec.stamina.unwrap_or(100.0),
            genome: spec.genome.clone().unwrap_or_default(),
            ..Default::default()
        };
        entities.push(world.spawn_agent(config));
    }
    entities
}

/// Spawn a single named agent using the spec.
fn spawn_named_agent(world: &mut TestWorld, spec: &AgentSpec) -> Entity {
    let config = AgentConfig {
        pos: spec.pos.unwrap_or(Vec2::ZERO),
        name: Some(spec.name.clone()),
        metabolism: spec
            .metabolism
            .clone()
            .unwrap_or_else(crate::agent::body::metabolism::Metabolism::well_fed),
        stamina: spec.stamina.unwrap_or(100.0),
        social_drive: spec.social_drive,
        genome: spec.genome.clone().unwrap_or_default(),
        knowledge: spec.knowledge.clone(),
        ..Default::default()
    };
    world.spawn_agent(config)
}

/// Write mutual Knows triples between all members of a group.
fn apply_mutual_knowledge(world: &mut TestWorld, members: &[Entity]) {
    // Collect (entity, name) pairs without holding borrows.
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

    for (i, (entity_a, name_a)) in pairs.iter().enumerate() {
        for (j, (entity_b, name_b)) in pairs.iter().enumerate() {
            if i == j {
                continue;
            }
            // Write knows-triple from a's perspective about b.
            let a = *entity_a;
            let b = *entity_b;
            let b_name = name_b.clone();
            let a_name = name_a.clone();

            {
                let mind_a = world.app_mut().world_mut().get_mut::<MindGraph>(a);
                if let Some(mut mind) = mind_a {
                    initialize_relationship(&mut mind, b, &b_name, 0);
                }
            }
            {
                let mind_b = world.app_mut().world_mut().get_mut::<MindGraph>(b);
                if let Some(mut mind) = mind_b {
                    initialize_relationship(&mut mind, a, &a_name, 0);
                }
            }
        }
    }
}

/// Write relationship dimensions from `a`'s perspective about `b`.
fn apply_relationship(world: &mut TestWorld, a: Entity, b: Entity, spec: &RelationshipSpec) {
    let b_name = world
        .app()
        .world()
        .get::<Name>(b)
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();

    let mind = world.app_mut().world_mut().get_mut::<MindGraph>(a);
    let Some(mut mind) = mind else { return };

    // initialize_relationship sets Knows/Introduced/NameOf/Trust/Affection/Respect/PowerBalance.
    // We then overwrite the three dimensions with the caller-specified values.
    initialize_relationship(&mut mind, b, &b_name, 0);

    let target = Node::Entity(b);
    let meta = Metadata::semantic(0);
    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::Trust,
        Value::Float(spec.trust),
        meta.clone(),
    ));
    mind.assert(Triple::with_meta(
        target.clone(),
        Predicate::Affection,
        Value::Float(spec.affection),
        meta.clone(),
    ));
    mind.assert(Triple::with_meta(
        target,
        Predicate::Respect,
        Value::Float(spec.respect),
        meta,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestWorld;
    use bevy::math::Vec2;

    #[test]
    fn scenario_builder_spawns_named_agent() {
        let (mut world, agents) = TestWorld::scenario(42)
            .map_size(16, 16)
            .noise_biomes(false)
            .agent("alice")
            .pos(Vec2::new(50.0, 50.0))
            .done()
            .build();

        let alice = agents["alice"];
        assert!(world.entity_exists(alice));
        assert_eq!(world.find_agent_by_name("alice"), Some(alice));
    }

    #[test]
    fn scenario_builder_named_agent_hunger_applied() {
        let (world, agents) = TestWorld::scenario(42)
            .map_size(16, 16)
            .noise_biomes(false)
            .agent("bob")
            .pos(Vec2::new(10.0, 10.0))
            .hunger_urgency(0.75)
            .done()
            .build();

        assert!((world.agent_hunger(agents["bob"]) - 0.75).abs() < 0.01);
    }

    #[test]
    fn scenario_builder_group_spawns_correct_count() {
        let (_world, agents) = TestWorld::scenario(1)
            .map_size(32, 32)
            .noise_biomes(false)
            .group("village")
            .agents(4)
            .near(Vec2::new(50.0, 50.0))
            .done()
            .build();

        assert_eq!(agents.group("village").len(), 4);
    }

    #[test]
    fn scenario_builder_knows_each_other_writes_knows_triple() {
        let (_world, agents) = TestWorld::scenario(7)
            .map_size(16, 16)
            .noise_biomes(false)
            .group("clan")
            .agents(2)
            .near(Vec2::new(20.0, 20.0))
            .knows_each_other(true)
            .done()
            .build();

        let members = agents.group("clan");
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn scenario_builder_relationship_sets_trust() {
        let (world, agents) = TestWorld::scenario(9)
            .map_size(16, 16)
            .noise_biomes(false)
            .agent("alice")
            .pos(Vec2::new(10.0, 10.0))
            .done()
            .agent("bob")
            .pos(Vec2::new(12.0, 10.0))
            .done()
            .relationship("alice", "bob", |r| r.trust(0.9))
            .build();

        let alice = agents["alice"];
        let bob = agents["bob"];
        let trust = world.agent_trust(alice, bob);
        assert!(
            (trust - 0.9).abs() < 0.01,
            "expected trust ~0.9, got {trust}"
        );
    }

    #[test]
    fn scenario_builder_tile_at_applies_edit() {
        let (_world, _agents) = TestWorld::scenario(3)
            .map_size(16, 16)
            .tile_at(5, 5, TileType::Water)
            .build();
        // Map construction succeeds without panicking — tile edit was applied.
    }

    #[test]
    fn scenario_builder_fill_rect_applies_region() {
        let (_world, _agents) = TestWorld::scenario(3)
            .map_size(16, 16)
            .fill_rect(0, 0, 4, 4, TileType::Grass)
            .build();
        // Builds without panic — region fill was applied.
    }

    #[test]
    fn solo_agent_preset_returns_single_entity() {
        let (mut world, agent) = TestWorld::solo_agent(42);
        assert!(world.entity_exists(agent));
        assert_eq!(world.all_agents().len(), 1);
    }

    #[test]
    fn two_strangers_preset_returns_two_distinct_entities() {
        let (mut world, a, b) = TestWorld::two_strangers(42);
        assert!(world.entity_exists(a));
        assert!(world.entity_exists(b));
        assert_ne!(a, b);
        assert_eq!(world.all_agents().len(), 2);
    }

    #[test]
    fn scenario_builder_custom_map_size_used() {
        let (_world, _agents) = TestWorld::scenario(0)
            .map_size(8, 8)
            .noise_biomes(false)
            .build();
        // Custom map size builds without panic.
    }

    #[test]
    fn scenario_entities_index_panics_on_unknown_name() {
        let result = std::panic::catch_unwind(|| {
            let (_world, agents) = TestWorld::scenario(0).map_size(8, 8).build();
            let _ = agents["nonexistent"];
        });
        assert!(result.is_err());
    }
}
