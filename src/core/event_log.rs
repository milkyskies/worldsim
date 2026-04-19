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
use crate::agent::events::{SimEvent, SimEventKind};

// ─── Config ──────────────────────────────────────────────────────────────────

/// Where to send log output.
#[derive(Debug, Clone)]
pub enum EventLogOutput {
    Stdout,
    /// JSONL file (default). One JSON object per line.
    File(PathBuf),
    /// Parquet file. Columnar format — 10-100x smaller than JSONL and
    /// DuckDB reads it instantly via `read_parquet`.
    Parquet(PathBuf),
}

/// Build an `EventLogOutput::File` or `EventLogOutput::Parquet` from a path
/// based on the file extension.
pub fn output_from_path(path: PathBuf) -> EventLogOutput {
    match path.extension().and_then(|s| s.to_str()) {
        Some("parquet") => EventLogOutput::Parquet(path),
        _ => EventLogOutput::File(path),
    }
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
    all_names: Query<(Entity, &Name)>,
) {
    // Build an owned entity→name map once per system run. The thread-local
    // resolver used by serde needs a `'static` closure, so the HashMap is
    // cloned into an Arc that the closure captures by ownership.
    let name_table: std::sync::Arc<std::collections::HashMap<Entity, String>> = std::sync::Arc::new(
        all_names
            .iter()
            .map(|(e, name)| (e, name.as_str().to_string()))
            .collect(),
    );
    let resolve = {
        let names = std::sync::Arc::clone(&name_table);
        move |entity: Entity| -> String {
            names
                .get(&entity)
                .cloned()
                .unwrap_or_else(|| entity_id_str(entity))
        }
    };

    for event in sim_events.read() {
        let (event_type, tick, involved_agents, involved_ids) = event_meta(event, &resolve);

        if !config.tick_passes(tick) {
            continue;
        }
        if !config.type_passes(event_type) {
            continue;
        }
        if !config.agent_passes(&involved_agents, &involved_ids) {
            continue;
        }

        let obj = event_to_json(event, resolve.clone());
        if let Ok(line) = serde_json::to_string(&obj) {
            buffer.lines.push(line);
        }
    }
}

/// Extract filter metadata from a SimEvent.
///
/// The new SimEvent struct carries `tick` and `agents: Vec<Entity>` at the
/// top level, so this helper is a thin pass-through that resolves each
/// entity to its (name, id) pair using the provided closure. The variant's
/// type-string comes from `strum::AsRefStr` via `event.kind.as_ref()`.
fn event_meta<'a>(
    event: &'a SimEvent,
    resolve: &impl Fn(Entity) -> String,
) -> (&'a str, u64, Vec<String>, Vec<String>) {
    let names = event.agents.iter().map(|e| resolve(*e)).collect();
    let ids = event.agents.iter().map(|e| entity_id_str(*e)).collect();
    (event.kind.as_ref(), event.tick, names, ids)
}

/// Render a SimEvent to a JSON object. Uses serde's derived `Serialize`
/// on `SimEventKind`, wrapped in a thread-local resolver so every Entity
/// field (top-level and nested) serializes as `{"name": ..., "id": ...}`.
///
/// Output shape:
/// ```json
/// {"tick": 123, "type": "Decision", <...payload fields flattened...>}
/// ```
/// The kind's externally-tagged `{"Decision": {...}}` wrapping is unwrapped
/// so callers see a flat object with `tick` and `type` hoisted to the top.
fn event_to_json(event: &SimEvent, resolve: impl Fn(Entity) -> String + 'static) -> Value {
    use serde_json::Map;
    let kind_value = crate::core::entity_serde::with_resolver(resolve, || {
        serde_json::to_value(&event.kind).unwrap_or(Value::Null)
    });
    // `serde` emits externally-tagged enums as `{"VariantName": {...fields}}`.
    // Unwrap to just the fields so the output schema is flat.
    let payload_fields = match kind_value {
        Value::Object(mut outer) => outer
            .remove(event.kind.as_ref())
            .and_then(|v| {
                if let Value::Object(m) = v {
                    Some(m)
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        _ => Map::new(),
    };
    let mut out = Map::new();
    out.insert("tick".into(), Value::from(event.tick));
    out.insert("type".into(), Value::from(event.kind.as_ref()));
    out.extend(payload_fields);
    Value::Object(out)
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
        EventLogOutput::Parquet(path) => {
            if let Err(e) = write_parquet(&buffer.lines, path) {
                eprintln!(
                    "event-log: parquet write failed for {}: {e}",
                    path.display()
                );
            }
        }
    }
}

/// Write a list of JSONL event lines to a Parquet file with a minimal schema
/// (tick INT64, event_type STRING, agent STRING, payload STRING).
///
/// Each line is parsed to extract `tick`/`type`/`agent` for fast columnar
/// filtering; the full JSON is also stored in `payload` so no data is lost.
pub fn write_parquet(lines: &[String], path: &std::path::Path) -> std::io::Result<()> {
    use parquet::basic::{Compression, Type as PhysicalType};
    use parquet::data_type::{ByteArray, ByteArrayType, Int64Type};
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::types::Type as SchemaType;
    use std::sync::Arc;

    let schema = Arc::new(
        SchemaType::group_type_builder("events")
            .with_fields(vec![
                Arc::new(
                    SchemaType::primitive_type_builder("tick", PhysicalType::INT64)
                        .with_repetition(parquet::basic::Repetition::REQUIRED)
                        .build()
                        .map_err(|e| std::io::Error::other(e.to_string()))?,
                ),
                Arc::new(
                    SchemaType::primitive_type_builder("event_type", PhysicalType::BYTE_ARRAY)
                        .with_repetition(parquet::basic::Repetition::REQUIRED)
                        .with_logical_type(Some(parquet::basic::LogicalType::String))
                        .build()
                        .map_err(|e| std::io::Error::other(e.to_string()))?,
                ),
                Arc::new(
                    SchemaType::primitive_type_builder("agent", PhysicalType::BYTE_ARRAY)
                        .with_repetition(parquet::basic::Repetition::OPTIONAL)
                        .with_logical_type(Some(parquet::basic::LogicalType::String))
                        .build()
                        .map_err(|e| std::io::Error::other(e.to_string()))?,
                ),
                Arc::new(
                    SchemaType::primitive_type_builder("payload", PhysicalType::BYTE_ARRAY)
                        .with_repetition(parquet::basic::Repetition::REQUIRED)
                        .with_logical_type(Some(parquet::basic::LogicalType::String))
                        .build()
                        .map_err(|e| std::io::Error::other(e.to_string()))?,
                ),
            ])
            .build()
            .map_err(|e| std::io::Error::other(e.to_string()))?,
    );

    let props = Arc::new(
        WriterProperties::builder()
            .set_compression(Compression::UNCOMPRESSED)
            .build(),
    );

    let file = std::fs::File::create(path)?;
    let mut writer = SerializedFileWriter::new(file, schema, props)
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let mut ticks: Vec<i64> = Vec::with_capacity(lines.len());
    let mut types: Vec<ByteArray> = Vec::with_capacity(lines.len());
    let mut agents: Vec<ByteArray> = Vec::with_capacity(lines.len());
    let mut agent_defs: Vec<i16> = Vec::with_capacity(lines.len());
    let mut payloads: Vec<ByteArray> = Vec::with_capacity(lines.len());

    for line in lines {
        let json: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let tick = json.get("tick").and_then(|v| v.as_i64()).unwrap_or(0);
        let event_type = json
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let agent = json
            .get("agent")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        ticks.push(tick);
        types.push(ByteArray::from(event_type.into_bytes()));
        match agent {
            Some(a) => {
                agents.push(ByteArray::from(a.into_bytes()));
                agent_defs.push(1);
            }
            None => {
                agent_defs.push(0);
            }
        }
        payloads.push(ByteArray::from(line.clone().into_bytes()));
    }

    let mut row_group = writer
        .next_row_group()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    if let Some(mut col) = row_group
        .next_column()
        .map_err(|e| std::io::Error::other(e.to_string()))?
    {
        col.typed::<Int64Type>()
            .write_batch(&ticks, None, None)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        col.close()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    if let Some(mut col) = row_group
        .next_column()
        .map_err(|e| std::io::Error::other(e.to_string()))?
    {
        col.typed::<ByteArrayType>()
            .write_batch(&types, None, None)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        col.close()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    if let Some(mut col) = row_group
        .next_column()
        .map_err(|e| std::io::Error::other(e.to_string()))?
    {
        col.typed::<ByteArrayType>()
            .write_batch(&agents, Some(&agent_defs), None)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        col.close()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    if let Some(mut col) = row_group
        .next_column()
        .map_err(|e| std::io::Error::other(e.to_string()))?
    {
        col.typed::<ByteArrayType>()
            .write_batch(&payloads, None, None)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        col.close()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    row_group
        .close()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    writer
        .close()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    Ok(())
}

/// Emit a DuckDB setup script that pre-attaches the run directory's logs
/// as views. The script creates one view per log file found (events,
/// trace, fields, mutations) and a handful of canned joined views.
///
/// Usage:
///   worldsim --debug path/to/run > setup.sql
///   duckdb -init setup.sql run.db
pub fn generate_duckdb_setup_script(run_dir: &std::path::Path) -> std::io::Result<String> {
    use std::fmt::Write;

    if !run_dir.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("run directory not found: {}", run_dir.display()),
        ));
    }

    let mut out = String::new();
    writeln!(
        &mut out,
        "-- worldsim debug views for {}",
        run_dir.display()
    )
    .ok();
    writeln!(&mut out).ok();

    let pairs: [(&str, &str); 4] = [
        ("events", "events"),
        ("trace", "trace"),
        ("fields", "fields"),
        ("mutations", "mutations"),
    ];

    for (view_name, file_stem) in pairs {
        let jsonl = run_dir.join(format!("{file_stem}.jsonl"));
        let parquet = run_dir.join(format!("{file_stem}.parquet"));
        if parquet.is_file() {
            writeln!(
                &mut out,
                "CREATE OR REPLACE VIEW {view_name} AS SELECT * FROM read_parquet('{}');",
                parquet.display()
            )
            .ok();
        } else if jsonl.is_file() {
            writeln!(
                &mut out,
                "CREATE OR REPLACE VIEW {view_name} AS SELECT * FROM read_json_auto('{}');",
                jsonl.display()
            )
            .ok();
        }
    }

    writeln!(&mut out).ok();
    writeln!(&mut out, "-- Canned joined views").ok();
    writeln!(
        &mut out,
        "CREATE OR REPLACE VIEW decisions_with_plans AS
  SELECT d.tick, d.agent, d.winner, d.actions, p.plan_id, p.goal, p.driving_urgency
  FROM events d LEFT JOIN events p
    ON p.type = 'PlanGenerated' AND p.agent = d.agent AND p.tick = d.tick
  WHERE d.type = 'Decision';"
    )
    .ok();

    Ok(out)
}

/// Read `payload` column from a Parquet file — the raw JSONL lines
/// stored by `write_parquet`. Used by round-trip tests and by
/// `worldsim debug` to stream events back out without DuckDB.
pub fn read_parquet_payloads(path: &std::path::Path) -> std::io::Result<Vec<String>> {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use parquet::record::reader::RowIter;

    let file = std::fs::File::open(path)?;
    let reader =
        SerializedFileReader::new(file).map_err(|e| std::io::Error::other(e.to_string()))?;
    let iter: RowIter = reader
        .get_row_iter(None)
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let mut out = Vec::new();
    for row_result in iter {
        let row = row_result.map_err(|e| std::io::Error::other(e.to_string()))?;
        // Column layout: tick (i64), event_type (string), agent (string?), payload (string)
        for (name, field) in row.get_column_iter() {
            if name == "payload" {
                let s = match field {
                    parquet::record::Field::Str(s) => s.clone(),
                    parquet::record::Field::Bytes(b) => {
                        String::from_utf8_lossy(b.data()).into_owned()
                    }
                    _ => String::new(),
                };
                out.push(s);
            }
        }
    }
    Ok(out)
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

    #[test]
    fn event_to_json_emits_flat_schema_with_hoisted_tick_and_type() {
        use crate::agent::brains::proposal::{BrainPowers, BrainType};
        use crate::agent::events::{SimEvent, SimEventKind};
        let e = Entity::from_raw_u32(7).unwrap();
        let event = SimEvent::single(
            42,
            e,
            SimEventKind::Decision {
                agent: e,
                winner: Some(BrainType::Rational),
                chosen_actions: vec![],
                powers: BrainPowers::default(),
                proposals: std::sync::Arc::new(vec![]),
                urgencies: vec![],
            },
        );
        let resolve = move |entity: Entity| {
            if entity == e {
                "Alice".to_string()
            } else {
                format!("{entity:?}")
            }
        };
        let json = event_to_json(&event, resolve);
        assert_eq!(json["tick"], 42);
        assert_eq!(json["type"], "Decision");
        // Entity fields serialize as {name, id} objects.
        assert_eq!(json["agent"]["name"], "Alice");
        assert_eq!(json["agent"]["id"], "7v0");
        // Payload fields are flat, not nested under a "Decision" key.
        assert!(json["winner"].is_string());
        assert!(!json.get("Decision").is_some());
    }
}
