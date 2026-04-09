//! Decision trace logging: per-agent ring buffer of SimEvent-derived records.
//!
//! Reads: SimEvent (Decision, ActionStarted, ActionCompleted, ActionPreempted, ActionFailed, EmotionTriggered, EntityPerceived), agent Names
//! Writes: DecisionTraceBuffer resource (ring buffers indexed by agent Entity)
//! Upstream: events::SimEvent, cli::CliArgs (via HeadlessConfig)
//! Downstream: headless::run_headless (dumps trace on completion), tests

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use bevy::prelude::*;
use serde::Serialize;

use crate::agent::Agent;
use crate::agent::brains::proposal::BrainType;
use crate::agent::events::SimEvent;

// ─── Config ──────────────────────────────────────────────────────────────────

/// Which agents to include in the trace.
#[derive(Debug, Clone, Default)]
pub enum AgentFilter {
    /// Trace is disabled (no recording occurs).
    #[default]
    Disabled,
    /// Trace all agents.
    All,
    /// Trace a specific agent by name (case-insensitive).
    Named(String),
}

/// Output format used by [`dump_trace`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TraceFormat {
    /// Human-readable lines written to stderr.
    #[default]
    Text,
    /// One JSON object per line written to `output_file` (or stdout if unset).
    Jsonl,
}

/// Configuration for the decision trace system.
///
/// Insert as a resource before ticking to activate recording. The default
/// value has [`AgentFilter::Disabled`], so no recording occurs unless you
/// override it.
#[derive(Resource, Debug, Clone, Default)]
pub struct TraceConfig {
    /// Agent filter. [`AgentFilter::Disabled`] means no tracing.
    pub agent_filter: AgentFilter,
    /// Only record events within this tick range (inclusive). `None` = all ticks.
    pub tick_range: Option<(u64, u64)>,
    /// Output format used by [`dump_trace`].
    pub format: TraceFormat,
    /// File path for JSONL output. If `None` and format is JSONL, writes to stdout.
    pub output_file: Option<PathBuf>,
    /// Maximum records to keep per agent ring buffer. 0 uses the default (500).
    pub buffer_size: usize,
}

impl TraceConfig {
    /// Returns `true` if tracing is active (filter is not `Disabled`).
    pub fn is_enabled(&self) -> bool {
        !matches!(self.agent_filter, AgentFilter::Disabled)
    }

    /// Returns `true` if the given agent name should be recorded.
    pub fn matches_agent(&self, name: &str) -> bool {
        match &self.agent_filter {
            AgentFilter::Disabled => false,
            AgentFilter::All => true,
            AgentFilter::Named(n) => n.eq_ignore_ascii_case(name),
        }
    }

    /// Returns `true` if the given tick falls within the configured range.
    pub fn in_tick_range(&self, tick: u64) -> bool {
        match self.tick_range {
            None => true,
            Some((start, end)) => (start..=end).contains(&tick),
        }
    }

    /// Effective per-agent buffer capacity (defaults to 500 when 0 is set).
    pub fn effective_buffer_size(&self) -> usize {
        if self.buffer_size == 0 {
            500
        } else {
            self.buffer_size
        }
    }
}

// ─── Records ─────────────────────────────────────────────────────────────────

/// A single trace record stored in an agent's ring buffer.
///
/// Serializes to JSONL as a tagged enum (`"event"` field is the variant tag).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TraceRecord {
    ProposalMade {
        tick: u64,
        brain: String,
        action: String,
        urgency: f32,
        power: f32,
        score: f32,
        admitted: bool,
    },
    DecisionWinner {
        tick: u64,
        brain: String,
        actions: Vec<String>,
    },
    ActionStarted {
        tick: u64,
        action: String,
    },
    ActionCompleted {
        tick: u64,
        action: String,
    },
    ActionPreempted {
        tick: u64,
        preempted: String,
    },
    ActionFailed {
        tick: u64,
        action: String,
        reason: String,
    },
    EmotionTriggered {
        tick: u64,
        emotion: String,
        intensity: f32,
    },
    EntityPerceived {
        tick: u64,
        target: String,
    },
}

impl TraceRecord {
    /// Tick this record was captured on.
    pub fn tick(&self) -> u64 {
        match self {
            Self::ProposalMade { tick, .. }
            | Self::DecisionWinner { tick, .. }
            | Self::ActionStarted { tick, .. }
            | Self::ActionCompleted { tick, .. }
            | Self::ActionPreempted { tick, .. }
            | Self::ActionFailed { tick, .. }
            | Self::EmotionTriggered { tick, .. }
            | Self::EntityPerceived { tick, .. } => *tick,
        }
    }

    /// Format as a human-readable line (no agent prefix; caller adds that).
    pub fn to_text(&self) -> String {
        match self {
            Self::ProposalMade {
                brain,
                action,
                urgency,
                power,
                admitted,
                ..
            } => {
                let tag = if *admitted { " [admitted]" } else { "" };
                format!("{brain} proposed {action}, urgency={urgency:.0}, power={power:.0}{tag}")
            }
            Self::DecisionWinner { brain, actions, .. } => {
                format!("→ {brain} WINS → {}", actions.join(", "))
            }
            Self::ActionStarted { action, .. } => format!("ActionStarted {action}"),
            Self::ActionCompleted { action, .. } => format!("ActionCompleted {action}"),
            Self::ActionPreempted { preempted, .. } => format!("ActionPreempted {preempted}"),
            Self::ActionFailed { action, reason, .. } => {
                format!("ActionFailed {action}: {reason}")
            }
            Self::EmotionTriggered {
                emotion, intensity, ..
            } => format!("EmotionTriggered {emotion} intensity={intensity:.2}"),
            Self::EntityPerceived { target, .. } => format!("EntityPerceived {target}"),
        }
    }
}

// ─── Buffer ───────────────────────────────────────────────────────────────────

/// Trace data for a single agent: name and a bounded ring buffer of records.
#[derive(Debug, Default)]
pub struct AgentTrace {
    pub name: String,
    pub records: VecDeque<TraceRecord>,
}

/// Global resource holding per-agent decision trace ring buffers.
///
/// Populated by [`update_decision_trace`]. Readable after a headless run
/// via `world.app().world().resource::<DecisionTraceBuffer>()`.
#[derive(Resource, Debug, Default)]
pub struct DecisionTraceBuffer {
    pub agents: HashMap<Entity, AgentTrace>,
}

impl DecisionTraceBuffer {
    /// Push a record into an agent's ring buffer, evicting the oldest if full.
    fn push(&mut self, entity: Entity, record: TraceRecord, max_size: usize) {
        let trace = self.agents.entry(entity).or_default();
        trace.records.push_back(record);
        while trace.records.len() > max_size {
            trace.records.pop_front();
        }
    }

    /// Register or update the display name for an agent entity.
    fn set_name(&mut self, entity: Entity, name: String) {
        let trace = self.agents.entry(entity).or_default();
        if trace.name.is_empty() {
            trace.name = name;
        }
    }

    /// Returns an iterator over all agent traces, sorted by agent name.
    pub fn sorted_agents(&self) -> Vec<(Entity, &AgentTrace)> {
        let mut pairs: Vec<_> = self.agents.iter().map(|(&e, t)| (e, t)).collect();
        pairs.sort_by_key(|(_, t)| t.name.as_str());
        pairs
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn brain_name(bt: BrainType) -> &'static str {
    match bt {
        BrainType::Survival => "Survival",
        BrainType::Emotional => "Emotional",
        BrainType::Rational => "Rational",
    }
}

fn power_for(brain: BrainType, powers: &crate::agent::brains::proposal::BrainPowers) -> f32 {
    match brain {
        BrainType::Survival => powers.survival,
        BrainType::Emotional => powers.emotional,
        BrainType::Rational => powers.rational,
    }
}

// ─── System ──────────────────────────────────────────────────────────────────

/// Bevy system (runs in `Last` schedule): reads SimEvents and records them in
/// the per-agent ring buffers of [`DecisionTraceBuffer`].
pub fn update_decision_trace(
    config: Res<TraceConfig>,
    mut buffer: ResMut<DecisionTraceBuffer>,
    mut sim_events: MessageReader<SimEvent>,
    agent_names: Query<&Name, With<Agent>>,
    all_names: Query<&Name>,
) {
    if !config.is_enabled() {
        return;
    }

    let buf_size = config.effective_buffer_size();
    let events: Vec<_> = sim_events.read().cloned().collect();

    for event in events {
        match event {
            SimEvent::Decision {
                agent,
                tick,
                winner,
                chosen_actions,
                powers,
                proposals,
            } => {
                if !config.in_tick_range(tick) {
                    continue;
                }
                let name = agent_names
                    .get(agent)
                    .map(|n| n.as_str())
                    .unwrap_or("?")
                    .to_string();
                if !config.matches_agent(&name) {
                    continue;
                }
                buffer.set_name(agent, name);

                for proposal in proposals.iter() {
                    let power = power_for(proposal.brain, &powers);
                    let score = proposal.urgency * power;
                    let admitted = chosen_actions.contains(&proposal.action.action_type);
                    buffer.push(
                        agent,
                        TraceRecord::ProposalMade {
                            tick,
                            brain: brain_name(proposal.brain).to_string(),
                            action: proposal.action.name.clone(),
                            urgency: proposal.urgency,
                            power,
                            score,
                            admitted,
                        },
                        buf_size,
                    );
                }

                if let Some(winning_brain) = winner {
                    let action_names: Vec<String> =
                        chosen_actions.iter().map(|a| format!("{a:?}")).collect();
                    buffer.push(
                        agent,
                        TraceRecord::DecisionWinner {
                            tick,
                            brain: brain_name(winning_brain).to_string(),
                            actions: action_names,
                        },
                        buf_size,
                    );
                }
            }

            SimEvent::ActionStarted {
                agent,
                tick,
                action,
                ..
            } => {
                if !config.in_tick_range(tick) {
                    continue;
                }
                let name = agent_names
                    .get(agent)
                    .map(|n| n.as_str())
                    .unwrap_or("?")
                    .to_string();
                if !config.matches_agent(&name) {
                    continue;
                }
                buffer.set_name(agent, name);
                buffer.push(
                    agent,
                    TraceRecord::ActionStarted {
                        tick,
                        action: format!("{action:?}"),
                    },
                    buf_size,
                );
            }

            SimEvent::ActionCompleted {
                agent,
                tick,
                action,
            } => {
                if !config.in_tick_range(tick) {
                    continue;
                }
                let name = agent_names
                    .get(agent)
                    .map(|n| n.as_str())
                    .unwrap_or("?")
                    .to_string();
                if !config.matches_agent(&name) {
                    continue;
                }
                buffer.set_name(agent, name);
                buffer.push(
                    agent,
                    TraceRecord::ActionCompleted {
                        tick,
                        action: format!("{action:?}"),
                    },
                    buf_size,
                );
            }

            SimEvent::ActionPreempted {
                agent,
                tick,
                preempted_action,
            } => {
                if !config.in_tick_range(tick) {
                    continue;
                }
                let name = agent_names
                    .get(agent)
                    .map(|n| n.as_str())
                    .unwrap_or("?")
                    .to_string();
                if !config.matches_agent(&name) {
                    continue;
                }
                buffer.set_name(agent, name);
                buffer.push(
                    agent,
                    TraceRecord::ActionPreempted {
                        tick,
                        preempted: format!("{preempted_action:?}"),
                    },
                    buf_size,
                );
            }

            SimEvent::ActionFailed {
                agent,
                tick,
                action,
                reason,
            } => {
                if !config.in_tick_range(tick) {
                    continue;
                }
                let name = agent_names
                    .get(agent)
                    .map(|n| n.as_str())
                    .unwrap_or("?")
                    .to_string();
                if !config.matches_agent(&name) {
                    continue;
                }
                buffer.set_name(agent, name);
                buffer.push(
                    agent,
                    TraceRecord::ActionFailed {
                        tick,
                        action: format!("{action:?}"),
                        reason: format!("{reason:?}"),
                    },
                    buf_size,
                );
            }

            SimEvent::EmotionTriggered {
                agent,
                tick,
                emotion,
                intensity,
            } => {
                if !config.in_tick_range(tick) {
                    continue;
                }
                let name = agent_names
                    .get(agent)
                    .map(|n| n.as_str())
                    .unwrap_or("?")
                    .to_string();
                if !config.matches_agent(&name) {
                    continue;
                }
                buffer.set_name(agent, name);
                buffer.push(
                    agent,
                    TraceRecord::EmotionTriggered {
                        tick,
                        emotion: format!("{emotion:?}"),
                        intensity,
                    },
                    buf_size,
                );
            }

            SimEvent::EntityPerceived {
                agent,
                tick,
                target,
            } => {
                if !config.in_tick_range(tick) {
                    continue;
                }
                let name = agent_names
                    .get(agent)
                    .map(|n| n.as_str())
                    .unwrap_or("?")
                    .to_string();
                if !config.matches_agent(&name) {
                    continue;
                }
                buffer.set_name(agent, name);
                let target_name = all_names
                    .get(target)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|_| format!("{target:?}"));
                buffer.push(
                    agent,
                    TraceRecord::EntityPerceived {
                        tick,
                        target: target_name,
                    },
                    buf_size,
                );
            }

            // Other SimEvent variants (conversations, relationships, etc.) are
            // not part of the decision trace and are intentionally ignored here.
            _ => {}
        }
    }
}

// ─── Output ──────────────────────────────────────────────────────────────────

/// Dump the contents of the trace buffer to the configured output destination.
///
/// Call this after the simulation run completes (e.g. from `run_headless`).
/// Text format writes to stderr; JSONL format writes to the configured file or
/// stdout.
pub fn dump_trace(buffer: &DecisionTraceBuffer, config: &TraceConfig) {
    use std::io::Write;

    let agents = buffer.sorted_agents();

    if agents.iter().all(|(_, t)| t.records.is_empty()) {
        return;
    }

    match &config.format {
        TraceFormat::Text => {
            let stderr = std::io::stderr();
            let mut out = std::io::BufWriter::new(stderr.lock());
            for (_, trace) in &agents {
                for record in &trace.records {
                    let _ = writeln!(
                        out,
                        "[tick {}] {}: {}",
                        record.tick(),
                        trace.name,
                        record.to_text()
                    );
                }
            }
        }
        TraceFormat::Jsonl => match &config.output_file {
            Some(path) => match std::fs::File::create(path) {
                Ok(file) => {
                    let mut writer = std::io::BufWriter::new(file);
                    write_jsonl(&agents, &mut writer);
                }
                Err(e) => {
                    eprintln!(
                        "trace: could not create output file {}: {e}",
                        path.display()
                    );
                }
            },
            None => {
                let stdout = std::io::stdout();
                let mut writer = std::io::BufWriter::new(stdout.lock());
                write_jsonl(&agents, &mut writer);
            }
        },
    }
}

fn write_jsonl(agents: &[(Entity, &AgentTrace)], writer: &mut impl std::io::Write) {
    for (entity, trace) in agents {
        for record in &trace.records {
            let obj = serde_json::json!({
                "agent": trace.name,
                "agent_id": format!("{entity:?}"),
                "record": record,
            });
            let _ = writeln!(writer, "{obj}");
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{AgentConfig, TestWorld};

    #[test]
    fn trace_buffer_captures_decisions_for_all_agents() {
        use crate::agent::inventory::Inventory;
        use crate::agent::mind::knowledge::Concept;
        use bevy::math::Vec2;

        let mut world = TestWorld::new();
        world.app_mut().insert_resource(TraceConfig {
            agent_filter: AgentFilter::All,
            buffer_size: 500,
            ..Default::default()
        });

        // Hungry agent with berries pre-loaded so the brain decides to eat
        // without needing to walk or harvest first. Brain thinking_interval is
        // 60 ticks; 300 ticks gives ~5 brain cycles regardless of entity ID
        // offset (matches existing test_sim_events pattern).
        let agent = world.spawn_agent(AgentConfig {
            pos: Vec2::new(20.0, 20.0),
            hunger: 90.0,
            ..Default::default()
        });
        world
            .app_mut()
            .world_mut()
            .get_mut::<Inventory>(agent)
            .unwrap()
            .add(Concept::Berry, 5);

        world.tick(300);

        let buffer = world.app().world().resource::<DecisionTraceBuffer>();
        assert!(
            !buffer.agents.is_empty(),
            "at least one agent should have trace records after 300 ticks"
        );
        let total_records: usize = buffer.agents.values().map(|t| t.records.len()).sum();
        assert!(
            total_records > 0,
            "expected trace records after 300 ticks, got 0"
        );
        // Verify we captured at least one Decision winner entry
        let has_winner = buffer
            .agents
            .values()
            .flat_map(|t| t.records.iter())
            .any(|r| matches!(r, TraceRecord::DecisionWinner { .. }));
        assert!(has_winner, "expected at least one DecisionWinner record");
    }

    #[test]
    fn trace_buffer_disabled_by_default() {
        let mut world = TestWorld::new();
        // Default TraceConfig is already inserted by BrainPlugin (disabled)
        let _agent = world.spawn_agent(AgentConfig::default());
        world.tick(5); // Quick check: disabled means no records regardless of tick count

        let buffer = world.app().world().resource::<DecisionTraceBuffer>();
        assert!(
            buffer.agents.is_empty(),
            "no records should be collected when trace is disabled"
        );
    }

    #[test]
    fn trace_filter_by_agent_name() {
        use crate::agent::inventory::Inventory;
        use crate::agent::mind::knowledge::Concept;
        use bevy::math::Vec2;

        let mut world = TestWorld::new();

        // Alice is close to a berry bush and hungry — guarantees perception events
        // and eventually action events within 300 ticks.
        let alice = world.spawn_agent(AgentConfig {
            name: Some("Alice".to_string()),
            pos: Vec2::new(20.0, 20.0),
            hunger: 90.0,
            ..Default::default()
        });
        // Pre-load food so Alice acts quickly without a walk phase
        world
            .app_mut()
            .world_mut()
            .get_mut::<Inventory>(alice)
            .unwrap()
            .add(Concept::Berry, 5);

        world.spawn_agent(AgentConfig {
            name: Some("Bob".to_string()),
            pos: Vec2::new(500.0, 500.0), // Far away — no interaction
            ..Default::default()
        });

        world.app_mut().insert_resource(TraceConfig {
            agent_filter: AgentFilter::Named("Alice".to_string()),
            buffer_size: 500,
            ..Default::default()
        });

        world.tick(300);

        let buffer = world.app().world().resource::<DecisionTraceBuffer>();

        // Only Alice's entries should be in the buffer
        for trace in buffer.agents.values() {
            assert_eq!(
                trace.name.to_lowercase(),
                "alice",
                "only Alice's records should be in the buffer when filter is Named(Alice)"
            );
        }
    }

    #[test]
    fn trace_buffer_respects_tick_range() {
        use bevy::math::Vec2;

        let mut world = TestWorld::new();
        // EntityPerceived fires every tick once in range. Place agent near a
        // bush; even perception events respect the tick range filter.
        world.spawn_agent(AgentConfig {
            pos: Vec2::new(10.0, 10.0),
            ..Default::default()
        });
        world.spawn_berry_bush(Vec2::new(15.0, 10.0), 3);

        world.app_mut().insert_resource(TraceConfig {
            agent_filter: AgentFilter::All,
            tick_range: Some((3, 5)),
            buffer_size: 500,
            ..Default::default()
        });

        world.tick(10);

        let buffer = world.app().world().resource::<DecisionTraceBuffer>();

        for trace in buffer.agents.values() {
            for record in &trace.records {
                let tick = record.tick();
                assert!(
                    (3..=5).contains(&tick),
                    "record at tick {tick} is outside the configured range [3, 5]"
                );
            }
        }
    }

    #[test]
    fn ring_buffer_evicts_oldest_when_full() {
        let max = 5usize;
        let mut buffer = DecisionTraceBuffer::default();
        let entity = Entity::from_bits(1);

        for i in 0..10u64 {
            buffer.push(
                entity,
                TraceRecord::ActionStarted {
                    tick: i,
                    action: "Walk".to_string(),
                },
                max,
            );
        }

        let trace = &buffer.agents[&entity];
        assert_eq!(trace.records.len(), max);
        assert_eq!(trace.records.front().unwrap().tick(), 5);
        assert_eq!(trace.records.back().unwrap().tick(), 9);
    }

    #[test]
    fn trace_record_serializes_to_json() {
        let record = TraceRecord::DecisionWinner {
            tick: 42,
            brain: "Rational".to_string(),
            actions: vec!["Walk".to_string()],
        };
        let json = serde_json::to_string(&record).expect("should serialize");
        assert!(json.contains("\"event\":\"decision_winner\""));
        assert!(json.contains("\"tick\":42"));
        assert!(json.contains("Rational"));
    }

    #[test]
    fn trace_config_agent_filter_all_matches_any_name() {
        let config = TraceConfig {
            agent_filter: AgentFilter::All,
            ..Default::default()
        };
        assert!(config.matches_agent("Alice"));
        assert!(config.matches_agent("Bob"));
        assert!(config.matches_agent("unknown"));
    }

    #[test]
    fn trace_config_named_filter_is_case_insensitive() {
        let config = TraceConfig {
            agent_filter: AgentFilter::Named("alice".to_string()),
            ..Default::default()
        };
        assert!(config.matches_agent("Alice"));
        assert!(config.matches_agent("ALICE"));
        assert!(!config.matches_agent("Bob"));
    }

    #[test]
    fn trace_config_tick_range_is_inclusive() {
        let config = TraceConfig {
            agent_filter: AgentFilter::All,
            tick_range: Some((10, 20)),
            ..Default::default()
        };
        assert!(config.in_tick_range(10));
        assert!(config.in_tick_range(15));
        assert!(config.in_tick_range(20));
        assert!(!config.in_tick_range(9));
        assert!(!config.in_tick_range(21));
    }
}
