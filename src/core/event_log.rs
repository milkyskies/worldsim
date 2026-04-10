//! JSONL event logger: subscribes to the SimEvent bus and writes one JSON object per line.
//!
//! Reads: SimEvent (unified bus), agent Names
//! Writes: EventLogBuffer resource (collected JSONL lines)
//! Upstream: agent::events::SimEvent, cli::CliArgs (via HeadlessConfig)
//! Downstream: headless::run_headless (writes log on completion)

use std::path::PathBuf;

use bevy::prelude::*;
use serde_json::Value;

use crate::agent::Agent;
use crate::agent::events::SimEvent;

// ─── Config ──────────────────────────────────────────────────────────────────

/// Where to send log output.
#[derive(Debug, Clone)]
pub enum EventLogOutput {
    Stdout,
    File(PathBuf),
}

/// A single filter condition. All specified filters are ANDed together.
#[derive(Debug, Clone)]
pub enum EventLogFilter {
    /// Only log events where the primary agent name matches (case-insensitive).
    Agent(String),
    /// Only log events whose type string (e.g. "Decision", "ActionStarted") is in this set.
    Types(Vec<String>),
    /// Only log events in this tick range (inclusive).
    TickRange(u64, u64),
}

/// Configuration for the JSONL event logger.
#[derive(Resource, Debug, Clone)]
pub struct EventLogConfig {
    pub output: EventLogOutput,
    pub filters: Vec<EventLogFilter>,
}

impl EventLogConfig {
    fn tick_passes(&self, tick: u64) -> bool {
        self.filters.iter().all(|f| match f {
            EventLogFilter::TickRange(start, end) => (*start..=*end).contains(&tick),
            _ => true,
        })
    }

    fn type_passes(&self, event_type: &str) -> bool {
        self.filters.iter().all(|f| match f {
            EventLogFilter::Types(types) => {
                types.iter().any(|t| t.eq_ignore_ascii_case(event_type))
            }
            _ => true,
        })
    }

    fn agent_passes(&self, names: &[String]) -> bool {
        self.filters.iter().all(|f| match f {
            EventLogFilter::Agent(agent_name) => {
                names.is_empty() || names.iter().any(|n| n.eq_ignore_ascii_case(agent_name))
            }
            _ => true,
        })
    }
}

/// Parse a `--log-filter` string into an `EventLogFilter`.
///
/// Accepted prefixes:
/// - `agent:<name>`
/// - `type:<T1,T2,...>`
/// - `tick:<start>-<end>`
pub fn parse_log_filter(s: &str) -> Option<EventLogFilter> {
    if let Some(rest) = s.strip_prefix("agent:") {
        Some(EventLogFilter::Agent(rest.to_string()))
    } else if let Some(rest) = s.strip_prefix("type:") {
        let types = rest.split(',').map(|t| t.trim().to_string()).collect();
        Some(EventLogFilter::Types(types))
    } else if let Some(rest) = s.strip_prefix("tick:") {
        let (a, b) = rest.split_once('-')?;
        let start = a.parse::<u64>().ok()?;
        let end = b.parse::<u64>().ok()?;
        Some(EventLogFilter::TickRange(start, end))
    } else {
        None
    }
}

// ─── Buffer ──────────────────────────────────────────────────────────────────

/// Accumulates pre-serialized JSONL lines during a headless run.
#[derive(Resource, Default)]
pub struct EventLogBuffer {
    pub lines: Vec<String>,
}

// ─── System ──────────────────────────────────────────────────────────────────

/// Bevy system (Last schedule): reads SimEvents and appends JSONL lines to EventLogBuffer.
pub fn collect_event_log(
    config: Res<EventLogConfig>,
    mut buffer: ResMut<EventLogBuffer>,
    mut sim_events: MessageReader<SimEvent>,
    agent_names: Query<&Name, With<Agent>>,
    all_names: Query<&Name>,
) {
    let resolve = |entity: Entity| -> String {
        all_names
            .get(entity)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|_| format!("{entity:?}"))
    };
    let agent_resolve = |entity: Entity| -> String {
        agent_names
            .get(entity)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|_| format!("{entity:?}"))
    };

    for event in sim_events.read() {
        // Extract tick and the set of agent names involved in this event.
        let (event_type, tick, involved_agents) = event_meta(event, &agent_resolve, &resolve);

        if !config.tick_passes(tick) {
            continue;
        }
        if !config.type_passes(event_type) {
            continue;
        }
        if !config.agent_passes(&involved_agents) {
            continue;
        }

        let obj = event_to_json(event, event_type, tick, &resolve);
        if let Ok(line) = serde_json::to_string(&obj) {
            buffer.lines.push(line);
        }
    }
}

/// Returns (event_type_str, tick, agent_name_list) for filtering.
fn event_meta<'a>(
    event: &SimEvent,
    agent_resolve: &impl Fn(Entity) -> String,
    resolve: &impl Fn(Entity) -> String,
) -> (&'a str, u64, Vec<String>) {
    match event {
        SimEvent::Decision { agent, tick, .. } => ("Decision", *tick, vec![agent_resolve(*agent)]),
        SimEvent::ActionStarted { agent, tick, .. } => {
            ("ActionStarted", *tick, vec![agent_resolve(*agent)])
        }
        SimEvent::ActionCompleted { agent, tick, .. } => {
            ("ActionCompleted", *tick, vec![agent_resolve(*agent)])
        }
        SimEvent::ActionPreempted { agent, tick, .. } => {
            ("ActionPreempted", *tick, vec![agent_resolve(*agent)])
        }
        SimEvent::ActionFailed { agent, tick, .. } => {
            ("ActionFailed", *tick, vec![agent_resolve(*agent)])
        }
        SimEvent::PlanAbandoned { agent, tick, .. } => {
            ("PlanAbandoned", *tick, vec![agent_resolve(*agent)])
        }
        SimEvent::ConversationStarted {
            participants, tick, ..
        } => {
            let names = participants.iter().map(|e| resolve(*e)).collect();
            ("ConversationStarted", *tick, names)
        }
        SimEvent::ConversationEnded {
            participants, tick, ..
        } => {
            let names = participants.iter().map(|e| resolve(*e)).collect();
            ("ConversationEnded", *tick, names)
        }
        SimEvent::ConversationAbandoned {
            abandoner,
            abandoned,
            tick,
        } => (
            "ConversationAbandoned",
            *tick,
            vec![resolve(*abandoner), resolve(*abandoned)],
        ),
        SimEvent::RelationshipChanged {
            agent, other, tick, ..
        } => (
            "RelationshipChanged",
            *tick,
            vec![agent_resolve(*agent), agent_resolve(*other)],
        ),
        SimEvent::EmotionTriggered { agent, tick, .. } => {
            ("EmotionTriggered", *tick, vec![agent_resolve(*agent)])
        }
        SimEvent::Death { agent, tick, .. } => ("Death", *tick, vec![agent_resolve(*agent)]),
        SimEvent::EntityPerceived { agent, tick, .. } => {
            ("EntityPerceived", *tick, vec![agent_resolve(*agent)])
        }
        SimEvent::StrangerDetected { agent, tick, .. } => {
            ("StrangerDetected", *tick, vec![agent_resolve(*agent)])
        }
        SimEvent::KnowledgeShared {
            speaker,
            listener,
            tick,
            ..
        } => (
            "KnowledgeShared",
            *tick,
            vec![agent_resolve(*speaker), agent_resolve(*listener)],
        ),
        SimEvent::WarmthPerceived { agent, tick, .. } => {
            ("WarmthPerceived", *tick, vec![agent_resolve(*agent)])
        }
        SimEvent::SoundPerceived { agent, tick, .. } => {
            ("SoundPerceived", *tick, vec![agent_resolve(*agent)])
        }
        SimEvent::TheoryOfMindUpdated {
            agent, about, tick, ..
        } => (
            "TheoryOfMindUpdated",
            *tick,
            vec![agent_resolve(*agent), agent_resolve(*about)],
        ),
    }
}

fn event_to_json(
    event: &SimEvent,
    event_type: &str,
    tick: u64,
    resolve: &impl Fn(Entity) -> String,
) -> Value {
    match event {
        SimEvent::Decision {
            agent,
            winner,
            chosen_actions,
            powers,
            proposals,
            ..
        } => {
            let proposals_json: Vec<Value> = proposals
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "brain": format!("{:?}", p.brain),
                        "action": p.action.name,
                        "urgency": p.urgency,
                        "reasoning": p.reasoning,
                    })
                })
                .collect();
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "winner": winner.map(|w| format!("{:?}", w)),
                "actions": chosen_actions.iter().map(|a| format!("{a:?}")).collect::<Vec<_>>(),
                "powers": {
                    "survival": powers.survival,
                    "emotional": powers.emotional,
                    "rational": powers.rational,
                },
                "proposals": proposals_json,
            })
        }
        SimEvent::ActionStarted {
            agent,
            action,
            target,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "action": format!("{action:?}"),
                "target": target.map(resolve),
            })
        }
        SimEvent::ActionCompleted { agent, action, .. } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "action": format!("{action:?}"),
            })
        }
        SimEvent::ActionPreempted {
            agent,
            preempted_action,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "preempted": format!("{preempted_action:?}"),
            })
        }
        SimEvent::ActionFailed {
            agent,
            action,
            reason,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "action": format!("{action:?}"),
                "reason": format!("{reason:?}"),
            })
        }
        SimEvent::PlanAbandoned {
            agent,
            action,
            intent,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "action": format!("{action:?}"),
                "intent": format!("{intent:?}"),
            })
        }
        SimEvent::ConversationStarted {
            participants,
            conversation_id,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "participants": participants.iter().map(|e| resolve(*e)).collect::<Vec<_>>(),
                "conversation_id": conversation_id,
            })
        }
        SimEvent::ConversationEnded {
            participants,
            conversation_id,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "participants": participants.iter().map(|e| resolve(*e)).collect::<Vec<_>>(),
                "conversation_id": conversation_id,
            })
        }
        SimEvent::ConversationAbandoned {
            abandoner,
            abandoned,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "abandoner": resolve(*abandoner),
                "abandoned": resolve(*abandoned),
            })
        }
        SimEvent::RelationshipChanged {
            agent,
            other,
            dimension,
            old_value,
            new_value,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "other": resolve(*other),
                "dimension": format!("{dimension:?}"),
                "old": old_value,
                "new": new_value,
            })
        }
        SimEvent::EmotionTriggered {
            agent,
            emotion,
            intensity,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "emotion": format!("{emotion:?}"),
                "intensity": intensity,
            })
        }
        SimEvent::Death { agent, cause, .. } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "cause": cause,
            })
        }
        SimEvent::EntityPerceived { agent, target, .. } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "target": resolve(*target),
            })
        }
        SimEvent::StrangerDetected {
            agent, stranger, ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "stranger": resolve(*stranger),
            })
        }
        SimEvent::KnowledgeShared {
            speaker,
            listener,
            triple_count,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "speaker": resolve(*speaker),
                "listener": resolve(*listener),
                "triple_count": triple_count,
            })
        }
        SimEvent::WarmthPerceived { agent, source, .. } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "source": resolve(*source),
            })
        }
        SimEvent::SoundPerceived {
            agent,
            source,
            kind,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "source": resolve(*source),
                "kind": format!("{kind:?}"),
            })
        }
        SimEvent::TheoryOfMindUpdated {
            agent,
            about,
            source,
            belief_count,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "about": resolve(*about),
                "source": format!("{source:?}"),
                "belief_count": belief_count,
            })
        }
    }
}

// ─── Output ──────────────────────────────────────────────────────────────────

/// Write the collected JSONL lines to the configured output.
/// Files are opened in append mode so existing content is preserved.
pub fn dump_event_log(buffer: &EventLogBuffer, config: &EventLogConfig) {
    use std::io::Write;

    if buffer.lines.is_empty() {
        return;
    }

    match &config.output {
        EventLogOutput::Stdout => {
            let stdout = std::io::stdout();
            let mut writer = std::io::BufWriter::new(stdout.lock());
            for line in &buffer.lines {
                let _ = writeln!(writer, "{line}");
            }
        }
        EventLogOutput::File(path) => {
            let file = match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("event-log: could not open {}: {e}", path.display());
                    return;
                }
            };
            let mut writer = std::io::BufWriter::new(file);
            for line in &buffer.lines {
                let _ = writeln!(writer, "{line}");
            }
        }
    }
}
