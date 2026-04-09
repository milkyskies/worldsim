//! Headless simulation runner: spins up a TestWorld, populates it, runs N ticks at max speed, and emits a JSON report.
//!
//! Reads: testing::TestWorld, agent components (PhysicalNeeds, EmotionalState, Body, ConversationManager), DecisionTraceBuffer
//! Writes: HeadlessReport (serializable summary), spawn entities via TestWorld, trace output to stderr/file
//! Upstream: cli (CliArgs), main (binary entry point)
//! Downstream: stdout (JSON report), statistical tests, regression baselines, trace output

use std::time::{Duration, Instant};

use bevy::ecs::entity::Entity;
use bevy::math::Vec2;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::Serialize;

use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::brains::trace::{DecisionTraceBuffer, TraceConfig, dump_trace};
use crate::agent::mind::conversation::ConversationManager;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::testing::{AgentConfig, TestWorld};

/// Default world dimensions for headless populations. Matches TestWorld's
/// default walkable map so spawn positions never land in unwalkable tiles.
const DEFAULT_AREA_PX: f32 = 1024.0;

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
    /// Decision trace configuration. Disabled by default (no overhead when
    /// `trace.agent_filter` is `AgentFilter::Disabled`).
    pub trace: TraceConfig,
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
            trace: TraceConfig::default(),
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
/// If `config.trace.is_enabled()`, decision trace records are collected during
/// the run and dumped to stderr (text) or the configured file (JSONL) when the
/// run completes.
pub fn run_headless(config: HeadlessConfig) -> HeadlessReport {
    let mut world = TestWorld::with_seed(config.seed);

    // Override the default (disabled) TraceConfig if tracing was requested.
    if config.trace.is_enabled() {
        world.app_mut().insert_resource(config.trace.clone());
    }

    let spawned = populate(&mut world, &config);

    let start = Instant::now();
    world.tick(config.ticks);
    let elapsed = start.elapsed();

    // Dump trace output before collecting the report so any I/O goes to the
    // correct destination before the report is printed to stdout.
    if config.trace.is_enabled() {
        let buffer = world.app().world().resource::<DecisionTraceBuffer>();
        dump_trace(buffer, &config.trace);
    }

    collect_report(&mut world, &config, spawned, elapsed)
}

/// Spawns the configured population into the TestWorld using a seeded RNG for
/// positions. Returns the number of agents spawned (humans + deer).
fn populate(world: &mut TestWorld, config: &HeadlessConfig) -> u64 {
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);

    for _ in 0..config.humans {
        let pos = random_position(&mut rng);
        world.spawn_agent(AgentConfig::at(pos));
    }
    for _ in 0..config.deer {
        let pos = random_position(&mut rng);
        world.spawn_deer(pos);
    }
    for _ in 0..config.berry_bushes {
        let pos = random_position(&mut rng);
        world.spawn_berry_bush(pos, 5);
    }
    for _ in 0..config.apple_trees {
        let pos = random_position(&mut rng);
        world.spawn_apple_tree(pos, 7);
    }

    (config.humans + config.deer) as u64
}

fn random_position(rng: &mut ChaCha8Rng) -> Vec2 {
    let x = rng.random_range(0.0..DEFAULT_AREA_PX);
    let y = rng.random_range(0.0..DEFAULT_AREA_PX);
    Vec2::new(x, y)
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
}
