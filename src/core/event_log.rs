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

    fn agent_passes(&self, names: &[String], ids: &[String]) -> bool {
        self.filters.iter().all(|f| match f {
            EventLogFilter::Agent(query) => {
                if names.is_empty() && ids.is_empty() {
                    return true;
                }
                names.iter().any(|n| n.eq_ignore_ascii_case(query))
                    || ids.iter().any(|i| i.eq_ignore_ascii_case(query))
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

/// Stable, always-serializable id for an entity. Uses Bevy's Debug format
/// (e.g. `19v0` for index 19 / generation 0) so it survives despawn and
/// never collides across entities, unlike the Name component.
fn entity_id_str(entity: Entity) -> String {
    format!("{entity:?}")
}

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
            .unwrap_or_else(|_| entity_id_str(entity))
    };
    let agent_resolve = |entity: Entity| -> String {
        agent_names
            .get(entity)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|_| entity_id_str(entity))
    };

    for event in sim_events.read() {
        // Extract tick and the set of agent names + ids involved in this event.
        let (event_type, tick, involved_agents, involved_ids) =
            event_meta(event, &agent_resolve, &resolve);

        if !config.tick_passes(tick) {
            continue;
        }
        if !config.type_passes(event_type) {
            continue;
        }
        if !config.agent_passes(&involved_agents, &involved_ids) {
            continue;
        }

        let obj = event_to_json(event, event_type, tick, &resolve);
        if let Ok(line) = serde_json::to_string(&obj) {
            buffer.lines.push(line);
        }
    }
}

/// Returns (event_type_str, tick, involved_names, involved_ids) for filtering.
/// Names come from the resolver (may fall back to id on unnamed/despawned
/// entities); ids are always the stable Entity debug format so filters can
/// target a specific individual even after death.
fn event_meta<'a>(
    event: &SimEvent,
    agent_resolve: &impl Fn(Entity) -> String,
    resolve: &impl Fn(Entity) -> String,
) -> (&'a str, u64, Vec<String>, Vec<String>) {
    let one = |ty, tick, e: Entity, named: bool| {
        let name = if named { agent_resolve(e) } else { resolve(e) };
        (ty, tick, vec![name], vec![entity_id_str(e)])
    };
    let two = |ty, tick, a: Entity, b: Entity, named: bool| {
        let (na, nb) = if named {
            (agent_resolve(a), agent_resolve(b))
        } else {
            (resolve(a), resolve(b))
        };
        (
            ty,
            tick,
            vec![na, nb],
            vec![entity_id_str(a), entity_id_str(b)],
        )
    };
    match event {
        SimEvent::Decision { agent, tick, .. } => one("Decision", *tick, *agent, true),
        SimEvent::ActionStarted { agent, tick, .. } => one("ActionStarted", *tick, *agent, true),
        SimEvent::ActionCompleted { agent, tick, .. } => {
            one("ActionCompleted", *tick, *agent, true)
        }
        SimEvent::ActionPreempted { agent, tick, .. } => {
            one("ActionPreempted", *tick, *agent, true)
        }
        SimEvent::ActionFailed { agent, tick, .. } => one("ActionFailed", *tick, *agent, true),
        SimEvent::PlanAbandoned { agent, tick, .. } => one("PlanAbandoned", *tick, *agent, true),
        SimEvent::ConversationStarted {
            participants, tick, ..
        } => {
            let names = participants.iter().map(|e| resolve(*e)).collect();
            let ids = participants.iter().map(|e| entity_id_str(*e)).collect();
            ("ConversationStarted", *tick, names, ids)
        }
        SimEvent::ConversationEnded {
            participants, tick, ..
        } => {
            let names = participants.iter().map(|e| resolve(*e)).collect();
            let ids = participants.iter().map(|e| entity_id_str(*e)).collect();
            ("ConversationEnded", *tick, names, ids)
        }
        SimEvent::ConversationJoined { joiner, tick, .. } => {
            one("ConversationJoined", *tick, *joiner, false)
        }
        SimEvent::ConversationLeft { leaver, tick, .. } => {
            one("ConversationLeft", *tick, *leaver, false)
        }
        SimEvent::ConversationAbandoned {
            abandoner,
            abandoned,
            tick,
        } => two(
            "ConversationAbandoned",
            *tick,
            *abandoner,
            *abandoned,
            false,
        ),
        SimEvent::RelationshipChanged {
            agent, other, tick, ..
        } => two("RelationshipChanged", *tick, *agent, *other, true),
        SimEvent::EmotionTriggered { agent, tick, .. } => {
            one("EmotionTriggered", *tick, *agent, true)
        }
        SimEvent::Death { agent, tick, .. } => one("Death", *tick, *agent, true),
        SimEvent::EntityPerceived { agent, tick, .. } => {
            one("EntityPerceived", *tick, *agent, true)
        }
        SimEvent::StrangerDetected { agent, tick, .. } => {
            one("StrangerDetected", *tick, *agent, true)
        }
        SimEvent::KnowledgeShared {
            speaker,
            listener,
            tick,
            ..
        } => two("KnowledgeShared", *tick, *speaker, *listener, true),
        SimEvent::WarmthPerceived { agent, tick, .. } => {
            one("WarmthPerceived", *tick, *agent, true)
        }
        SimEvent::SoundPerceived { agent, tick, .. } => one("SoundPerceived", *tick, *agent, true),
        SimEvent::TheoryOfMindUpdated {
            agent, about, tick, ..
        } => two("TheoryOfMindUpdated", *tick, *agent, *about, true),
        SimEvent::ItemSpoiled { agent, tick, .. } => one("ItemSpoiled", *tick, *agent, true),
        SimEvent::EffectApplied { agent, tick, .. } => one("EffectApplied", *tick, *agent, true),
        SimEvent::LaborContributed { agent, tick, .. } => {
            one("LaborContributed", *tick, *agent, true)
        }
        SimEvent::SkillChanged { agent, tick, .. } => one("SkillChanged", *tick, *agent, true),
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
                "agent_id": entity_id_str(*agent),
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
                "agent_id": entity_id_str(*agent),
                "action": format!("{action:?}"),
                "target": target.map(resolve),
                "target_id": target.map(entity_id_str),
            })
        }
        SimEvent::ActionCompleted { agent, action, .. } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "agent_id": entity_id_str(*agent),
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
                "agent_id": entity_id_str(*agent),
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
                "agent_id": entity_id_str(*agent),
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
                "agent_id": entity_id_str(*agent),
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
                "participant_ids": participants.iter().map(|e| entity_id_str(*e)).collect::<Vec<_>>(),
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
                "participant_ids": participants.iter().map(|e| entity_id_str(*e)).collect::<Vec<_>>(),
                "conversation_id": conversation_id,
            })
        }
        SimEvent::ConversationJoined {
            joiner,
            conversation_id,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "joiner": resolve(*joiner),
                "joiner_id": entity_id_str(*joiner),
                "conversation_id": conversation_id,
            })
        }
        SimEvent::ConversationLeft {
            leaver,
            conversation_id,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "leaver": resolve(*leaver),
                "leaver_id": entity_id_str(*leaver),
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
                "abandoner_id": entity_id_str(*abandoner),
                "abandoned": resolve(*abandoned),
                "abandoned_id": entity_id_str(*abandoned),
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
                "agent_id": entity_id_str(*agent),
                "other": resolve(*other),
                "other_id": entity_id_str(*other),
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
                "agent_id": entity_id_str(*agent),
                "emotion": format!("{emotion:?}"),
                "intensity": intensity,
            })
        }
        SimEvent::Death { agent, cause, .. } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "agent_id": entity_id_str(*agent),
                "cause": cause,
            })
        }
        SimEvent::EntityPerceived { agent, target, .. } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "agent_id": entity_id_str(*agent),
                "target": resolve(*target),
                "target_id": entity_id_str(*target),
            })
        }
        SimEvent::StrangerDetected {
            agent, stranger, ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "agent_id": entity_id_str(*agent),
                "stranger": resolve(*stranger),
                "stranger_id": entity_id_str(*stranger),
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
                "speaker_id": entity_id_str(*speaker),
                "listener": resolve(*listener),
                "listener_id": entity_id_str(*listener),
                "triple_count": triple_count,
            })
        }
        SimEvent::WarmthPerceived { agent, source, .. } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "agent_id": entity_id_str(*agent),
                "source": resolve(*source),
                "source_id": entity_id_str(*source),
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
                "agent_id": entity_id_str(*agent),
                "source": resolve(*source),
                "source_id": entity_id_str(*source),
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
                "agent_id": entity_id_str(*agent),
                "about": resolve(*about),
                "about_id": entity_id_str(*about),
                "source": format!("{source:?}"),
                "belief_count": belief_count,
            })
        }
        SimEvent::ItemSpoiled {
            agent, from, to, ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "agent_id": entity_id_str(*agent),
                "from": format!("{from:?}"),
                "to": format!("{to:?}"),
            })
        }
        SimEvent::EffectApplied { agent, source, .. } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "agent_id": entity_id_str(*agent),
                "source": resolve(*source),
                "source_id": entity_id_str(*source),
            })
        }
        SimEvent::LaborContributed { agent, site, .. } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "agent_id": entity_id_str(*agent),
                "site": resolve(*site),
                "site_id": entity_id_str(*site),
            })
        }
        SimEvent::SkillChanged {
            agent,
            skill,
            old_value,
            new_value,
            ..
        } => {
            serde_json::json!({
                "tick": tick,
                "type": event_type,
                "agent": resolve(*agent),
                "agent_id": entity_id_str(*agent),
                "skill": format!("{skill:?}"),
                "old": old_value,
                "new": new_value,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(filter: EventLogFilter) -> EventLogConfig {
        EventLogConfig {
            output: EventLogOutput::Stdout,
            filters: vec![filter],
        }
    }

    #[test]
    fn agent_filter_matches_by_name() {
        let cfg = cfg_with(EventLogFilter::Agent("TestWolf".to_string()));
        let names = vec!["TestWolf".to_string()];
        let ids = vec!["18v0".to_string()];
        assert!(cfg.agent_passes(&names, &ids));
    }

    #[test]
    fn agent_filter_matches_by_entity_id() {
        let cfg = cfg_with(EventLogFilter::Agent("18v0".to_string()));
        let names = vec!["TestWolf".to_string()];
        let ids = vec!["18v0".to_string()];
        assert!(cfg.agent_passes(&names, &ids));
    }

    #[test]
    fn agent_filter_distinguishes_individuals_sharing_a_name() {
        // The whole point of #352-follow-up: multiple TestWolf entities need
        // to be distinguishable. Filtering by id selects a specific one even
        // when every wolf shares the TestWolf name.
        let cfg = cfg_with(EventLogFilter::Agent("19v0".to_string()));
        let wolf_a_names = vec!["TestWolf".to_string()];
        let wolf_a_ids = vec!["18v0".to_string()];
        let wolf_b_names = vec!["TestWolf".to_string()];
        let wolf_b_ids = vec!["19v0".to_string()];
        assert!(!cfg.agent_passes(&wolf_a_names, &wolf_a_ids));
        assert!(cfg.agent_passes(&wolf_b_names, &wolf_b_ids));
    }

    #[test]
    fn agent_filter_rejects_unrelated_entities() {
        let cfg = cfg_with(EventLogFilter::Agent("TestWolf".to_string()));
        let names = vec!["TestPerson".to_string()];
        let ids = vec!["3v0".to_string()];
        assert!(!cfg.agent_passes(&names, &ids));
    }

    #[test]
    fn agent_filter_passes_events_with_no_agent_refs() {
        // ConversationEnded etc. can legitimately serialize with no agent
        // identity; don't drop those for agent-targeted queries.
        let cfg = cfg_with(EventLogFilter::Agent("TestWolf".to_string()));
        assert!(cfg.agent_passes(&[], &[]));
    }
}
