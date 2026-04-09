//! Headless simulation runner: spins up a TestWorld, populates it, runs N ticks at max speed, and emits a JSON report.
//!
//! Reads: testing::TestWorld, agent components (PhysicalNeeds, EmotionalState, Body, ConversationManager), DecisionTraceBuffer
//! Writes: HeadlessReport (serializable summary), spawn entities via TestWorld, trace output to stderr/file
//! Upstream: cli (CliArgs), main (binary entry point)
//! Downstream: stdout (JSON report), statistical tests, regression baselines, trace output

use std::time::{Duration, Instant};

use bevy::ecs::entity::Entity;
use serde::Serialize;

use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::brains::trace::{DecisionTraceBuffer, TraceConfig, dump_trace};
use crate::agent::mind::conversation::ConversationManager;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::core::{EventLogBuffer, EventLogConfig, collect_event_log, dump_event_log};
use crate::testing::TestWorld;
use crate::world::map::WorldMap;
use crate::world::spawn_config::{SpawnAlgorithm, WorldSpawnConfig};

/// A MindGraph text query for a specific agent.
#[derive(Debug, Clone)]
pub struct InspectQuery {
    /// Agent name (case-insensitive).
    pub agent: String,
    /// Text to search for in the MindGraph.
    pub text: String,
}

/// Configuration for post-run inspection commands.
#[derive(Debug, Clone, Default)]
pub struct InspectConfig {
    /// If set, stop the simulation at this tick (overrides `HeadlessConfig::ticks`).
    pub at_tick: Option<u64>,
    /// Print full agent state snapshots for these agent names.
    pub inspect_agents: Vec<String>,
    /// Print full MindGraph dumps for these agent names.
    pub dump_mind_agents: Vec<String>,
    /// Execute these MindGraph text queries.
    pub queries: Vec<InspectQuery>,
}

impl InspectConfig {
    /// Returns true if any inspection commands are configured.
    pub fn is_active(&self) -> bool {
        !self.inspect_agents.is_empty()
            || !self.dump_mind_agents.is_empty()
            || !self.queries.is_empty()
    }
}

/// Configuration for a headless run.
#[derive(Debug, Clone)]
pub struct HeadlessConfig {
    /// Number of logical ticks to advance.
    pub ticks: u64,
    /// Seed for the spawn-position RNG. Same seed + same population produces
    /// the same starting layout.
    pub seed: u64,
    /// Number of human agents to spawn at startup.
    pub humans: usize,
    /// Number of berry bushes to scatter across the map.
    pub berry_bushes: usize,
    /// Number of apple trees to scatter across the map.
    pub apple_trees: usize,
    /// Number of deer to scatter across the map.
    pub deer: usize,
    /// When true, use the same 128×128 map and Realistic placement algorithm as
    /// the normal game. The `humans`, `deer`, `berry_bushes`, and `apple_trees`
    /// fields still apply and override the game defaults for each entity type.
    pub game_defaults: bool,
    /// Decision trace configuration. Disabled by default (no overhead when
    /// `trace.agent_filter` is `AgentFilter::Disabled`).
    pub trace: TraceConfig,
    /// JSONL event log configuration. `None` disables the logger.
    pub event_log: Option<EventLogConfig>,
    /// Inspection commands to run after the simulation completes.
    pub inspect: InspectConfig,
}

impl Default for HeadlessConfig {
    fn default() -> Self {
        Self {
            ticks: 1_000,
            seed: 0,
            humans: 5,
            berry_bushes: 8,
            apple_trees: 4,
            deer: 3,
            game_defaults: false,
            trace: TraceConfig::default(),
            event_log: None,
            inspect: InspectConfig::default(),
        }
    }
}

/// JSON-serializable summary of a headless run. Statistical tests parse this
/// to assert macro-level properties.
#[derive(Debug, Clone, Serialize)]
pub struct HeadlessReport {
    pub ticks: u64,
    pub seed: u64,
    pub elapsed_secs: f64,
    pub ticks_per_second: f64,
    pub agents: AgentStats,
    pub conversations: ConversationStats,
    pub physical_means: PhysicalMeans,
    pub emotions: EmotionStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStats {
    pub spawned: u64,
    pub alive: u64,
    pub died: u64,
    pub unconscious: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationStats {
    pub total: u64,
    pub active: u64,
    pub ended: u64,
    pub avg_turns: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PhysicalMeans {
    pub hunger: f32,
    pub thirst: f32,
    pub energy: f32,
    pub health: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmotionStats {
    pub joy: u64,
    pub sadness: u64,
    pub fear: u64,
    pub anger: u64,
    pub disgust: u64,
    pub surprise: u64,
}

/// Builds a TestWorld with the given config, populates it, runs `config.ticks`
/// ticks at max speed, and returns the resulting report.
///
/// If `config.game_defaults` is true, uses the 128×128 noise map and Realistic
/// placement algorithm (same as the normal windowed game). Otherwise uses a
/// 64×64 flat map with Uniform random scatter.
///
/// If `config.trace.is_enabled()`, decision trace records are collected during
/// the run and dumped to stderr (text) or the configured file (JSONL) when the
/// run completes.
pub fn run_headless(config: HeadlessConfig) -> HeadlessReport {
    let mut world = if config.game_defaults {
        TestWorld::with_game_map(config.seed)
    } else {
        TestWorld::with_seed(config.seed)
    };

    // Override the default (disabled) TraceConfig if tracing was requested.
    if config.trace.is_enabled() {
        world.app_mut().insert_resource(config.trace.clone());
    }

    // Register the JSONL event logger if --log was specified.
    if let Some(log_config) = &config.event_log {
        world.app_mut().insert_resource(log_config.clone());
        world.app_mut().init_resource::<EventLogBuffer>();
        world
            .app_mut()
            .add_systems(bevy::app::Last, collect_event_log);
    }

    let spawned = populate(&mut world, &config);

    // If --at-tick is set, stop there; otherwise run the full --ticks count.
    let ticks_to_run = config
        .inspect
        .at_tick
        .unwrap_or(config.ticks)
        .min(config.ticks);

    let start = Instant::now();
    world.tick(ticks_to_run);
    let elapsed = start.elapsed();

    // Dump trace output before inspection so ordering is predictable.
    if config.trace.is_enabled() {
        let buffer = world.app().world().resource::<DecisionTraceBuffer>();
        dump_trace(buffer, &config.trace);
    }

    // Dump event log if configured.
    if let Some(log_config) = &config.event_log {
        let buffer = world.app().world().resource::<EventLogBuffer>();
        dump_event_log(buffer, log_config);
    }

    // Run inspection commands if any were specified.
    if config.inspect.is_active() {
        run_inspection(&mut world, &config.inspect);
    }

    collect_report(&mut world, &config, spawned, elapsed)
}

/// Execute all inspection commands against the current world state.
fn run_inspection(world: &mut TestWorld, inspect: &InspectConfig) {
    for agent_name in &inspect.inspect_agents {
        match world.find_agent_by_name(agent_name) {
            Some(entity) => {
                world.print_agent_state(entity);
                world.print_brain_decision(entity);
            }
            None => {
                eprintln!("inspect: no agent named {agent_name:?} found");
            }
        }
    }

    for agent_name in &inspect.dump_mind_agents {
        match world.find_agent_by_name(agent_name) {
            Some(entity) => {
                world.print_mind_graph(entity);
            }
            None => {
                eprintln!("dump-mind: no agent named {agent_name:?} found");
            }
        }
    }

    for q in &inspect.queries {
        match world.find_agent_by_name(&q.agent) {
            Some(entity) => {
                let results = world.query_knowledge(entity, &q.text);
                eprintln!(
                    "query [{agent}] \"{text}\" — {n} result(s):",
                    agent = q.agent,
                    text = q.text,
                    n = results.len()
                );
                for r in &results {
                    eprintln!("  {r}");
                }
            }
            None => {
                eprintln!("query: no agent named {:?} found", q.agent);
            }
        }
    }
}

/// Spawns the configured population into the TestWorld using `WorldSpawnConfig`.
/// Returns the number of agents spawned (humans + deer).
fn populate(world: &mut TestWorld, config: &HeadlessConfig) -> u64 {
    let spawn_config = if config.game_defaults {
        WorldSpawnConfig {
            humans: config.humans,
            deer: config.deer,
            berry_bushes: config.berry_bushes,
            apple_trees: config.apple_trees,
            seed: config.seed,
            ..WorldSpawnConfig::game_defaults()
        }
    } else {
        WorldSpawnConfig {
            humans: config.humans,
            deer: config.deer,
            berry_bushes: config.berry_bushes,
            apple_trees: config.apple_trees,
            seed: config.seed,
            spawn_algorithm: SpawnAlgorithm::Uniform,
            ..WorldSpawnConfig::game_defaults()
        }
    };

    let layout = {
        let map = world.app().world().resource::<WorldMap>();
        spawn_config.compute_layout(map)
    };
    world.apply_spawn_layout(&layout);

    (config.humans + config.deer) as u64
}

fn collect_report(
    world: &mut TestWorld,
    config: &HeadlessConfig,
    spawned: u64,
    elapsed: Duration,
) -> HeadlessReport {
    let elapsed_secs = elapsed.as_secs_f64();
    let ticks_per_second = if elapsed_secs > 0.0 {
        config.ticks as f64 / elapsed_secs
    } else {
        f64::INFINITY
    };

    let agent_entities = world.all_agents();
    HeadlessReport {
        ticks: config.ticks,
        seed: config.seed,
        elapsed_secs,
        ticks_per_second,
        agents: collect_agent_stats(world, &agent_entities, spawned),
        conversations: collect_conversation_stats(world),
        physical_means: collect_physical_means(world, &agent_entities),
        emotions: collect_emotion_stats(world, &agent_entities),
    }
}

fn collect_agent_stats(world: &TestWorld, agents: &[Entity], spawned: u64) -> AgentStats {
    let alive = agents.len() as u64;
    let died = spawned.saturating_sub(alive);

    let mut unconscious = 0u64;
    for entity in agents {
        if let Some(c) = world.app().world().get::<Consciousness>(*entity)
            && c.alertness < 0.5
        {
            unconscious += 1;
        }
    }

    AgentStats {
        spawned,
        alive,
        died,
        unconscious,
    }
}

fn collect_conversation_stats(world: &TestWorld) -> ConversationStats {
    let manager = world.app().world().resource::<ConversationManager>();
    let total = manager.conversations.len() as u64;
    let active = manager.active_conversations().count() as u64;
    let ended = total - active;
    let total_turns: u64 = manager
        .conversations
        .values()
        .map(|c| c.turns.len() as u64)
        .sum();
    let avg_turns = if total > 0 {
        total_turns as f64 / total as f64
    } else {
        0.0
    };
    ConversationStats {
        total,
        active,
        ended,
        avg_turns,
    }
}

fn collect_physical_means(world: &TestWorld, agents: &[Entity]) -> PhysicalMeans {
    let mut sum = PhysicalMeans {
        hunger: 0.0,
        thirst: 0.0,
        energy: 0.0,
        health: 0.0,
    };
    let mut count = 0.0f32;
    for entity in agents {
        if let Some(needs) = world.app().world().get::<PhysicalNeeds>(*entity) {
            sum.hunger += needs.hunger;
            sum.thirst += needs.thirst;
            sum.energy += needs.energy;
            sum.health += needs.health;
            count += 1.0;
        }
    }
    if count > 0.0 {
        sum.hunger /= count;
        sum.thirst /= count;
        sum.energy /= count;
        sum.health /= count;
    }
    sum
}

fn collect_emotion_stats(world: &TestWorld, agents: &[Entity]) -> EmotionStats {
    let mut stats = EmotionStats {
        joy: 0,
        sadness: 0,
        fear: 0,
        anger: 0,
        disgust: 0,
        surprise: 0,
    };
    for entity in agents {
        let Some(state) = world.app().world().get::<EmotionalState>(*entity) else {
            continue;
        };
        for emotion in &state.active_emotions {
            match emotion.emotion_type {
                EmotionType::Joy => stats.joy += 1,
                EmotionType::Sadness => stats.sadness += 1,
                EmotionType::Fear => stats.fear += 1,
                EmotionType::Anger => stats.anger += 1,
                EmotionType::Disgust => stats.disgust += 1,
                EmotionType::Surprise => stats.surprise += 1,
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_headless_with_default_config_completes() {
        let config = HeadlessConfig {
            ticks: 30,
            ..Default::default()
        };
        let report = run_headless(config);
        assert_eq!(report.ticks, 30);
        assert_eq!(report.agents.spawned, 8); // 5 humans + 3 deer
        assert!(report.elapsed_secs >= 0.0);
    }

    #[test]
    fn report_serializes_to_json() {
        let config = HeadlessConfig {
            ticks: 5,
            humans: 2,
            deer: 0,
            berry_bushes: 1,
            apple_trees: 1,
            ..Default::default()
        };
        let report = run_headless(config);
        let json = serde_json::to_string(&report).expect("report should serialize");
        assert!(json.contains("\"ticks\":5"));
        assert!(json.contains("\"agents\""));
        assert!(json.contains("\"conversations\""));
    }

    #[test]
    fn same_seed_produces_same_initial_population_layout() {
        let cfg = HeadlessConfig {
            ticks: 0,
            seed: 42,
            humans: 4,
            deer: 2,
            berry_bushes: 0,
            apple_trees: 0,
            ..Default::default()
        };

        let mut world_a = TestWorld::with_seed(cfg.seed);
        let mut world_b = TestWorld::with_seed(cfg.seed);
        populate(&mut world_a, &cfg);
        populate(&mut world_b, &cfg);

        let entities_a = world_a.all_agents();
        let entities_b = world_b.all_agents();
        let positions_a: Vec<_> = entities_a
            .iter()
            .map(|e| world_a.get::<bevy::prelude::Transform>(*e).translation)
            .collect();
        let positions_b: Vec<_> = entities_b
            .iter()
            .map(|e| world_b.get::<bevy::prelude::Transform>(*e).translation)
            .collect();

        assert_eq!(positions_a, positions_b);
    }

    #[test]
    fn agent_stats_count_alive_correctly() {
        let config = HeadlessConfig {
            ticks: 1,
            humans: 3,
            deer: 1,
            berry_bushes: 0,
            apple_trees: 0,
            ..Default::default()
        };
        let report = run_headless(config);
        assert_eq!(report.agents.spawned, 4);
        assert!(report.agents.alive <= report.agents.spawned);
        assert_eq!(
            report.agents.alive + report.agents.died,
            report.agents.spawned
        );
    }

    #[test]
    fn game_defaults_spawns_correct_human_count() {
        let game = crate::world::spawn_config::WorldSpawnConfig::game_defaults();
        let config = HeadlessConfig {
            ticks: 0,
            game_defaults: true,
            humans: game.humans,
            deer: game.deer,
            berry_bushes: game.berry_bushes,
            apple_trees: game.apple_trees,
            ..Default::default()
        };
        let report = run_headless(config);
        assert_eq!(report.agents.spawned, (game.humans + game.deer) as u64);
    }

    #[test]
    fn game_defaults_human_override_applies() {
        let config = HeadlessConfig {
            ticks: 0,
            game_defaults: true,
            humans: 10,
            deer: 0,
            berry_bushes: 0,
            apple_trees: 0,
            ..Default::default()
        };
        let report = run_headless(config);
        assert_eq!(report.agents.spawned, 10); // overridden to 10 humans, 0 deer
    }

    #[test]
    fn game_defaults_same_seed_same_positions() {
        let cfg = HeadlessConfig {
            ticks: 0,
            seed: 42,
            game_defaults: true,
            ..Default::default()
        };

        let mut world_a = TestWorld::with_game_map(cfg.seed);
        let mut world_b = TestWorld::with_game_map(cfg.seed);
        populate(&mut world_a, &cfg);
        populate(&mut world_b, &cfg);

        let entities_a = world_a.all_agents();
        let entities_b = world_b.all_agents();
        let positions_a: Vec<_> = entities_a
            .iter()
            .map(|e| world_a.get::<bevy::prelude::Transform>(*e).translation)
            .collect();
        let positions_b: Vec<_> = entities_b
            .iter()
            .map(|e| world_b.get::<bevy::prelude::Transform>(*e).translation)
            .collect();

        assert_eq!(positions_a, positions_b);
    }
}
