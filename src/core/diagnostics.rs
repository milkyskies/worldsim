use bevy::prelude::*;
use std::collections::VecDeque;

use crate::agent::Agent;
use crate::agent::mind::knowledge::MindGraph;

// ═══════════════════════════════════════════════════════════════════════════
// DIAGNOSTICS RESOURCE — Track performance metrics over time
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Resource, Default)]
pub struct PerformanceDiagnostics {
    /// Historical MindGraph triple counts (timestamp, total_triples, agent_count)
    pub mindgraph_history: VecDeque<(u64, usize, usize)>,
    /// Max history entries to keep
    pub max_history: usize,
    /// Last time we logged detailed stats
    pub last_detailed_log: u64,
    /// Initial triple counts per agent (entity index -> initial count)
    pub initial_counts: std::collections::HashMap<u32, usize>,
    /// Track if we've captured initial state
    pub initialized: bool,
}

impl PerformanceDiagnostics {
    pub fn new() -> Self {
        Self {
            mindgraph_history: VecDeque::new(),
            max_history: 120, // ~2 minutes at 1 sample/sec
            last_detailed_log: 0,
            initial_counts: std::collections::HashMap::new(),
            initialized: false,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MINDGRAPH PROFILING SYSTEM
// ═══════════════════════════════════════════════════════════════════════════

pub fn profile_mindgraph_growth(
    agents: Query<(Entity, &Name, &MindGraph), With<Agent>>,
    tick: Res<crate::core::TickCount>,
    mut game_log: ResMut<crate::core::GameLog>,
    mut diagnostics: ResMut<PerformanceDiagnostics>,
) {
    let current_tick = tick.current;

    // Log every 5 seconds (300 ticks at 60 tps)
    let log_interval = (5.0 * tick.ticks_per_second) as u64;
    if current_tick < diagnostics.last_detailed_log + log_interval {
        return;
    }
    diagnostics.last_detailed_log = current_tick;

    let mut total_triples = 0usize;
    let mut total_by_subject = 0usize;
    let mut total_by_subject_pred = 0usize;
    let mut total_by_predicate = 0usize;
    let mut agent_count = 0usize;

    // Per-agent breakdown
    let mut agent_stats: Vec<(String, usize, usize, i64)> = Vec::new();

    for (entity, name, mind) in agents.iter() {
        let triple_count = mind.triples.len();
        let by_subject_size = mind.by_subject.len();
        let by_subject_pred_size = mind.by_subject_pred.len();
        let by_predicate_size = mind.by_predicate.len();

        total_triples += triple_count;
        total_by_subject += by_subject_size;
        total_by_subject_pred += by_subject_pred_size;
        total_by_predicate += by_predicate_size;
        agent_count += 1;

        // Calculate growth from initial
        let initial = diagnostics
            .initial_counts
            .get(&entity.index())
            .copied()
            .unwrap_or(0);
        let growth = triple_count as i64 - initial as i64;

        // Capture initial state
        if !diagnostics.initialized {
            diagnostics
                .initial_counts
                .insert(entity.index(), triple_count);
        }

        agent_stats.push((name.to_string(), triple_count, by_subject_pred_size, growth));
    }

    diagnostics.initialized = true;

    // Store history
    diagnostics
        .mindgraph_history
        .push_back((current_tick, total_triples, agent_count));
    while diagnostics.mindgraph_history.len() > diagnostics.max_history {
        diagnostics.mindgraph_history.pop_front();
    }

    // Calculate growth rate
    let growth_rate = if diagnostics.mindgraph_history.len() >= 2 {
        let oldest = &diagnostics.mindgraph_history[0];
        let newest = diagnostics.mindgraph_history.back().unwrap();
        let time_diff = (newest.0 - oldest.0) as f64 / tick.ticks_per_second as f64;
        let triple_diff = newest.1 as f64 - oldest.1 as f64;
        if time_diff > 0.0 {
            triple_diff / time_diff
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Log summary
    game_log.performance(format!("=== MINDGRAPH STATS (tick {}) ===", current_tick));
    game_log.performance(format!(
        "Total: {} triples across {} agents ({:.1}/agent)",
        total_triples,
        agent_count,
        if agent_count > 0 {
            total_triples as f64 / agent_count as f64
        } else {
            0.0
        }
    ));
    game_log.performance(format!("Growth rate: {:.2} triples/sec", growth_rate));
    game_log.performance(format!(
        "Index sizes: by_subject={}, by_subject_pred={}, by_predicate={}",
        total_by_subject, total_by_subject_pred, total_by_predicate
    ));

    // Per-agent details (if few agents)
    if agent_count <= 10 {
        for (name, triples, index_size, growth) in agent_stats {
            let growth_str = if growth > 0 {
                format!("+{}", growth)
            } else {
                format!("{}", growth)
            };
            game_log.performance(format!(
                "  {} : {} triples (idx:{}) [{}]",
                name, triples, index_size, growth_str
            ));
        }
    }

    // Breakdown by memory type
    profile_memory_types(&agents, &mut game_log);
}

fn profile_memory_types(
    agents: &Query<(Entity, &Name, &MindGraph), With<Agent>>,
    game_log: &mut ResMut<crate::core::GameLog>,
) {
    use crate::agent::mind::knowledge::MemoryType;

    let mut type_counts: std::collections::HashMap<MemoryType, usize> =
        std::collections::HashMap::new();
    let mut predicate_counts: std::collections::HashMap<crate::agent::mind::knowledge::Predicate, usize> =
        std::collections::HashMap::new();

    for (_, _, mind) in agents.iter() {
        for triple in &mind.triples {
            *type_counts.entry(triple.meta.memory_type).or_insert(0) += 1;
            *predicate_counts.entry(triple.predicate).or_insert(0) += 1;
        }
    }

    // Log memory type breakdown
    let mut type_vec: Vec<_> = type_counts.into_iter().collect();
    type_vec.sort_by(|a, b| b.1.cmp(&a.1));

    game_log.performance("Memory types:".to_string());
    for (mem_type, count) in type_vec.iter().take(5) {
        game_log.performance(format!("  {:?}: {}", mem_type, count));
    }

    // Log predicate breakdown (top 5)
    let mut pred_vec: Vec<_> = predicate_counts.into_iter().collect();
    pred_vec.sort_by(|a, b| b.1.cmp(&a.1));

    game_log.performance("Top predicates:".to_string());
    for (predicate, count) in pred_vec.iter().take(5) {
        game_log.performance(format!("  {:?}: {}", predicate, count));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// EVENT ACCUMULATION PROFILING
// ═══════════════════════════════════════════════════════════════════════════

pub fn profile_working_memory(
    agents: Query<(Entity, &Name, &crate::agent::mind::memory::WorkingMemory), With<Agent>>,
    tick: Res<crate::core::TickCount>,
    mut game_log: ResMut<crate::core::GameLog>,
    mut last_log: Local<u64>,
) {
    let current_tick = tick.current;
    let log_interval = (5.0 * tick.ticks_per_second) as u64;

    if current_tick < *last_log + log_interval {
        return;
    }
    *last_log = current_tick;

    let mut total_wm_items = 0usize;
    let mut processed_count = 0usize;
    let mut unprocessed_count = 0usize;

    for (_, _, wm) in agents.iter() {
        total_wm_items += wm.buffer.len();
        for item in &wm.buffer {
            if item.processed {
                processed_count += 1;
            } else {
                unprocessed_count += 1;
            }
        }
    }

    if total_wm_items > 0 {
        game_log.performance(format!(
            "WorkingMemory: {} items ({} processed, {} pending)",
            total_wm_items, processed_count, unprocessed_count
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PERCEPTION SYSTEM PROFILING (enhanced)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Resource, Default)]
pub struct PerceptionMetrics {
    pub total_checks: u64,
    pub total_visible: u64,
    pub samples: VecDeque<(u64, u64, u64)>, // (tick, checks, visible)
}

impl PerceptionMetrics {
    pub fn record(&mut self, tick: u64, checks: u64, visible: u64) {
        self.total_checks += checks;
        self.total_visible += visible;
        self.samples.push_back((tick, checks, visible));
        while self.samples.len() > 60 {
            self.samples.pop_front();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PLANNER METRICS (Resource to track A* stats)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Resource, Default)]
pub struct PlannerMetrics {
    pub total_plans: u64,
    pub total_iterations: u64,
    pub max_iterations_seen: usize,
    pub plans_this_second: u64,
    pub last_second_tick: u64,
}

impl PlannerMetrics {
    pub fn record_plan(&mut self, iterations: usize, tick: u64, tps: f32) {
        self.total_plans += 1;
        self.total_iterations += iterations as u64;
        self.max_iterations_seen = self.max_iterations_seen.max(iterations);

        // Reset per-second counter
        let second_boundary = (tick as f32 / tps) as u64;
        let last_second = (self.last_second_tick as f32 / tps) as u64;
        if second_boundary > last_second {
            self.plans_this_second = 0;
        }
        self.plans_this_second += 1;
        self.last_second_tick = tick;
    }
}

pub fn log_planner_metrics(
    tick: Res<crate::core::TickCount>,
    mut game_log: ResMut<crate::core::GameLog>,
    metrics: Res<PlannerMetrics>,
    mut last_log: Local<u64>,
) {
    let current_tick = tick.current;
    let log_interval = (5.0 * tick.ticks_per_second) as u64;

    if current_tick < *last_log + log_interval {
        return;
    }
    *last_log = current_tick;

    if metrics.total_plans > 0 {
        let avg_iterations = metrics.total_iterations as f64 / metrics.total_plans as f64;
        game_log.performance(format!(
            "Planner: {} total plans, {:.1} avg iterations, {} max iterations",
            metrics.total_plans, avg_iterations, metrics.max_iterations_seen
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DIAGNOSTIC PLUGIN
// ═══════════════════════════════════════════════════════════════════════════

pub struct DiagnosticsPlugin;

impl Plugin for DiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PerformanceDiagnostics::new())
            .insert_resource(PerceptionMetrics::default())
            .insert_resource(PlannerMetrics::default())
            .add_systems(
                Update,
                (
                    profile_mindgraph_growth,
                    profile_working_memory,
                    log_planner_metrics,
                )
                    .run_if(crate::core::not_paused),
            );
    }
}
