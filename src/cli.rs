//! Command-line interface parsing for the worldsim binary.
//!
//! Reads: process argv (via clap)
//! Writes: CliArgs (parsed flags), HeadlessConfig (derived from flags)
//! Upstream: main (binary entry point)
//! Downstream: headless::run_headless, the main Bevy app

use std::path::PathBuf;

use clap::Parser;

use crate::agent::brains::trace::{AgentFilter, TraceConfig, TraceFormat};
use crate::core::{
    EventLogConfig, EventLogOutput, FieldLoggerConfig, FieldLoggerFormat, FieldLoggerOutput,
    expand_fields, parse_agent_selector, parse_log_filter, parse_on_change_spec,
};
use crate::headless::{HeadlessConfig, InspectConfig, InspectQuery, WhyQuery};
use crate::world::spawn_config::WorldSpawnConfig;

/// Command-line arguments accepted by the worldsim binary.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "worldsim",
    about = "Agent simulation. Run windowed by default; pass --headless for max-speed batch runs.",
    version
)]
pub struct CliArgs {
    /// Run without a window or rendering. The simulation advances by `--ticks`
    /// ticks at max speed and then exits.
    #[arg(long)]
    pub headless: bool,

    /// Number of logical ticks to advance in headless mode.
    #[arg(long, default_value_t = 1_000)]
    pub ticks: u64,

    /// Seed for the spawn-position RNG. Same seed + same population produces
    /// the same starting layout.
    #[arg(long, default_value_t = 0)]
    pub seed: u64,

    /// After a headless run finishes, print a JSON report to stdout.
    #[arg(long)]
    pub report: bool,

    /// Use the same 128x128 map and Realistic placement algorithm as the normal
    /// game. The --humans, --deer, etc. flags still override individual counts.
    /// Without this flag, headless uses a 64x64 flat map with uniform scatter.
    #[arg(long)]
    pub game_defaults: bool,

    /// Number of human agents to spawn at startup (headless mode only).
    /// Defaults to the game defaults (6) when --game-defaults is set,
    /// or the headless defaults (5) otherwise.
    #[arg(long)]
    pub humans: Option<usize>,

    /// Number of berry bushes to scatter (headless mode only).
    /// Defaults to the game defaults (32) when --game-defaults is set,
    /// or the headless defaults (8) otherwise.
    #[arg(long)]
    pub berry_bushes: Option<usize>,

    /// Number of apple trees to scatter (headless mode only).
    /// Defaults to the game defaults (24) when --game-defaults is set,
    /// or the headless defaults (4) otherwise.
    #[arg(long)]
    pub apple_trees: Option<usize>,

    /// Number of deer to scatter (headless mode only).
    /// Defaults to the game defaults (8) when --game-defaults is set,
    /// or the headless defaults (3) otherwise.
    #[arg(long)]
    pub deer: Option<usize>,

    /// Number of wolf predators to scatter (headless mode only).
    /// Defaults to 0 when not set. Use --game-defaults to spawn the full wolf pack.
    #[arg(long)]
    pub wolves: Option<usize>,

    /// Enable decision trace logging. Use "all" to trace all agents or
    /// "agent:<selector>" (e.g. "agent:alice" or "agent:0v0") to trace a
    /// specific agent. The selector accepts either a display name
    /// (case-insensitive) or a Bevy entity-id string — the latter matches
    /// the `agent_id` field in the JSONL event log so you can copy an id
    /// straight from a log line. Trace output is written to stderr (text)
    /// or the file set by --trace-file (JSONL). Only meaningful in
    /// --headless mode.
    #[arg(long)]
    pub trace: Option<String>,

    /// Restrict trace recording to a tick range (inclusive). Format: START-END,
    /// e.g. "4500-4600". Requires --trace.
    #[arg(long)]
    pub trace_ticks: Option<String>,

    /// Output format for the trace dump: "text" (human-readable, stderr) or
    /// "jsonl" (one JSON object per line). Default: text.
    #[arg(long, default_value = "text")]
    pub trace_format: String,

    /// File path for JSONL trace output. If omitted and --trace-format=jsonl,
    /// writes to stdout.
    #[arg(long)]
    pub trace_file: Option<PathBuf>,

    /// Write a JSONL event log to this path, or "-" for stdout.
    /// Each line is one simulation event serialized as JSON.
    #[arg(long)]
    pub log: Option<String>,

    /// Filter events written to --log. Can be repeated; all filters must pass.
    /// Formats:
    ///   agent:<selector>   (display name or entity id, e.g. "alice", "0v0")
    ///   type:<T1,T2>
    ///   tick:<start>-<end>
    #[arg(long = "log-filter")]
    pub log_filter: Vec<String>,

    /// Print a full agent state snapshot at --at-tick. Format:
    /// `agent:<selector>` where selector is a display name or entity id
    /// (e.g. `agent:alice`, `agent:0v0`). Can be repeated to inspect
    /// multiple agents.
    #[arg(long)]
    pub inspect: Vec<String>,

    /// Print an agent's full MindGraph at --at-tick. Format:
    /// `agent:<selector>` — see `--inspect` for selector rules. Can be
    /// repeated.
    #[arg(long = "dump-mind")]
    pub dump_mind: Vec<String>,

    /// Search an agent's MindGraph by text at --at-tick.
    /// Format: "<selector> <query-text>" where selector is a display name or
    /// entity id (the first whitespace-separated token). Can be repeated.
    #[arg(long)]
    pub query: Vec<String>,

    /// Print the causal breakdown for a metric at --at-tick. Format:
    /// "<agent-selector> metric:<name>" — currently supported metrics:
    /// glucose, stamina, hydration, mood. Can be repeated.
    ///
    /// Example: --why "alice metric:glucose"
    #[arg(long)]
    pub why: Vec<String>,

    /// Print body-channel occupancy for an agent at --at-tick. Format:
    /// `agent:<selector>`. Can be repeated.
    #[arg(long = "dump-channels")]
    pub dump_channels: Vec<String>,

    /// Print the agent's current perception snapshot (every entity in
    /// their VisibleObjects with name, kind, and distance) at --at-tick.
    /// Format: `agent:<selector>`. Can be repeated.
    #[arg(long = "dump-perception")]
    pub dump_perception: Vec<String>,

    /// Shortcut for "print everything we know about this agent": full
    /// state snapshot, brain decision, full MindGraph, channels. Format:
    /// `agent:<selector>`. Can be repeated.
    #[arg(long = "dump-all")]
    pub dump_all: Vec<String>,

    /// Tick(s) at which to perform inspection. If not specified, inspects at
    /// the final tick (after --ticks). Can be repeated to inspect at multiple
    /// points in a single run (e.g. `--at-tick 500 --at-tick 5000`).
    #[arg(long)]
    pub at_tick: Vec<u64>,

    /// Generate the default-seed terrain and print it to stdout as an ASCII
    /// matrix, then exit. Useful for inspecting river carving and biome
    /// placement without launching the game window.
    #[arg(long)]
    pub dump_map: bool,

    // ─── Per-tick field logger (#490) ─────────────────────────────────────
    /// Agents to log each tick. Repeatable. Accepts `all`, `species:<X>`
    /// (Human/Deer/Wolf/Rabbit/Bird), `name:<substring>`, or a literal
    /// agent name / Bevy entity id (e.g. `alice`, `19v0`).
    #[arg(long = "log-agent")]
    pub log_agent: Vec<String>,

    /// Dotted field path(s) to capture each tick. Repeatable. Supports
    /// wildcards like `needs.*` and a `:delta` suffix (e.g.
    /// `needs.glucose:delta`) to inline the delta-since-last-emission.
    #[arg(long = "log-field")]
    pub log_field: Vec<String>,

    /// Preset field bundle(s): `vitals`, `actions`, `brain`, or `full`.
    /// Repeatable; combines with `--log-field`.
    #[arg(long = "log-preset")]
    pub log_preset: Vec<String>,

    /// Where to write field-logger output. Default is stderr. `-` sends to
    /// stdout. Anything else is treated as a file path (overwritten each run).
    #[arg(long = "log-file")]
    pub log_file: Option<String>,

    /// Heartbeat interval in ticks. `1` (default) emits every tick; larger
    /// values sample every Nth tick. Combined with `--log-on-change` via an
    /// OR rule: emit on the heartbeat OR whenever a watched field changed.
    #[arg(long = "log-every", default_value_t = 1)]
    pub log_every: u64,

    /// Only emit when the given field has changed since the last emission.
    /// Repeatable. Supports a `:<threshold>` suffix (e.g.
    /// `needs.aerobic:2.0`) to override the default change threshold for
    /// that field.
    #[arg(long = "log-on-change")]
    pub log_on_change: Vec<String>,

    /// Debounce change-driven emissions by N ticks. A change must stay
    /// different from the last emitted line for N ticks before it is
    /// written out; transient flickers that revert inside the window are
    /// dropped entirely. Applies only to `--log-on-change` — heartbeats
    /// (`--log-every`) and the default every-tick mode bypass debounce.
    /// `0` (default) disables it.
    #[arg(long = "log-debounce", default_value_t = 0)]
    pub log_debounce: u64,

    /// Output format for the field logger. `jsonl` (default) or `csv`. CSV
    /// flattens nested objects into dotted-path columns.
    #[arg(long = "log-as", default_value = "jsonl")]
    pub log_as: String,

    /// Dry-run: expand `--log-preset` / `--log-field` / wildcards and print
    /// the resolved field list to stdout, then exit without running a sim.
    #[arg(long = "log-list-fields")]
    pub log_list_fields: bool,
}

impl CliArgs {
    /// Builds a HeadlessConfig from these CLI arguments.
    ///
    /// Population counts fall back to game defaults when `--game-defaults` is set,
    /// or headless defaults otherwise.
    pub fn to_headless_config(&self) -> HeadlessConfig {
        let (
            default_humans,
            default_deer,
            default_wolves,
            default_berry_bushes,
            default_apple_trees,
        ) = if self.game_defaults {
            let g = WorldSpawnConfig::game_defaults();
            (g.humans, g.deer, g.wolves, g.berry_bushes, g.apple_trees)
        } else {
            (5, 3, 0, 8, 4)
        };

        HeadlessConfig {
            ticks: self.ticks,
            seed: self.seed,
            game_defaults: self.game_defaults,
            humans: self.humans.unwrap_or(default_humans),
            berry_bushes: self.berry_bushes.unwrap_or(default_berry_bushes),
            apple_trees: self.apple_trees.unwrap_or(default_apple_trees),
            deer: self.deer.unwrap_or(default_deer),
            wolves: self.wolves.unwrap_or(default_wolves),
            trace: self.build_trace_config(),
            event_log: self.build_event_log_config(),
            inspect: self.build_inspect_config(),
            field_logger: self.build_field_logger_config(),
        }
    }

    /// Build the per-tick field-logger config. Returns `None` when no
    /// `--log-field` / `--log-preset` / `--log-agent` flags were given.
    pub fn build_field_logger_config(&self) -> Option<FieldLoggerConfig> {
        if self.log_field.is_empty() && self.log_preset.is_empty() && self.log_agent.is_empty() {
            return None;
        }

        let fields = match expand_fields(&self.log_field, &self.log_preset) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("--log-field/--log-preset: {e}");
                return None;
            }
        };
        if fields.is_empty() {
            return None;
        }

        let agents = if self.log_agent.is_empty() {
            eprintln!("--log-field requires at least one --log-agent selector");
            return None;
        } else {
            self.log_agent
                .iter()
                .map(|s| parse_agent_selector(s))
                .collect()
        };

        let output = match self.log_file.as_deref() {
            None => FieldLoggerOutput::Stderr,
            Some("-") => FieldLoggerOutput::Stdout,
            Some(path) => FieldLoggerOutput::File(PathBuf::from(path)),
        };

        let format = match self.log_as.as_str() {
            "csv" => FieldLoggerFormat::Csv,
            _ => FieldLoggerFormat::Jsonl,
        };

        let on_change: Vec<_> = self
            .log_on_change
            .iter()
            .map(|s| parse_on_change_spec(s))
            .collect();

        Some(FieldLoggerConfig {
            agents,
            fields,
            output,
            format,
            every: self.log_every.max(1),
            on_change,
            debounce: self.log_debounce,
        })
    }

    fn build_event_log_config(&self) -> Option<EventLogConfig> {
        let log_path = self.log.as_deref()?;
        let output = if log_path == "-" {
            EventLogOutput::Stdout
        } else {
            EventLogOutput::File(PathBuf::from(log_path))
        };
        let filters = self
            .log_filter
            .iter()
            .filter_map(|s| parse_log_filter(s))
            .collect();
        Some(EventLogConfig { output, filters })
    }

    fn build_inspect_config(&self) -> InspectConfig {
        let mut at_ticks = self.at_tick.clone();
        at_ticks.sort_unstable();
        at_ticks.dedup();

        let inspect_agents: Vec<String> = self
            .inspect
            .iter()
            .filter_map(|s| s.strip_prefix("agent:").map(|n| n.to_string()))
            .collect();

        let dump_mind_agents: Vec<String> = self
            .dump_mind
            .iter()
            .filter_map(|s| s.strip_prefix("agent:").map(|n| n.to_string()))
            .collect();

        let queries: Vec<InspectQuery> = self
            .query
            .iter()
            .filter_map(|s| {
                let (agent, query) = s.split_once(' ')?;
                Some(InspectQuery {
                    agent: agent.to_string(),
                    text: query.to_string(),
                })
            })
            .collect();

        let why_queries: Vec<WhyQuery> = self
            .why
            .iter()
            .filter_map(|s| {
                let (agent, rest) = s.split_once(' ')?;
                let metric = rest.strip_prefix("metric:")?.trim().to_string();
                Some(WhyQuery {
                    agent: agent.to_string(),
                    metric,
                })
            })
            .collect();

        let dump_channels_agents: Vec<String> = self
            .dump_channels
            .iter()
            .filter_map(|s| s.strip_prefix("agent:").map(|n| n.to_string()))
            .collect();

        let dump_perception_agents: Vec<String> = self
            .dump_perception
            .iter()
            .filter_map(|s| s.strip_prefix("agent:").map(|n| n.to_string()))
            .collect();

        let dump_all_agents: Vec<String> = self
            .dump_all
            .iter()
            .filter_map(|s| s.strip_prefix("agent:").map(|n| n.to_string()))
            .collect();

        InspectConfig {
            at_ticks,
            inspect_agents,
            dump_mind_agents,
            queries,
            why_queries,
            dump_channels_agents,
            dump_perception_agents,
            dump_all_agents,
        }
    }

    fn build_trace_config(&self) -> TraceConfig {
        let agent_filter = match self.trace.as_deref() {
            None => AgentFilter::Disabled,
            Some("all") => AgentFilter::All,
            Some(s) if s.starts_with("agent:") => {
                AgentFilter::Named(s["agent:".len()..].to_string())
            }
            Some(_) => AgentFilter::All,
        };

        let tick_range = self.trace_ticks.as_deref().and_then(parse_tick_range);

        let format = match self.trace_format.as_str() {
            "jsonl" => TraceFormat::Jsonl,
            _ => TraceFormat::Text,
        };

        TraceConfig {
            agent_filter,
            tick_range,
            format,
            output_file: self.trace_file.clone(),
            buffer_size: 500,
        }
    }
}

/// Parses a tick range string of the form "START-END" into `(start, end)`.
/// Returns `None` if the format is invalid.
fn parse_tick_range(s: &str) -> Option<(u64, u64)> {
    let (start_str, end_str) = s.split_once('-')?;
    let start = start_str.parse::<u64>().ok()?;
    let end = end_str.parse::<u64>().ok()?;
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headless_with_ticks_and_seed() {
        let args =
            CliArgs::try_parse_from(["worldsim", "--headless", "--ticks", "500", "--seed", "7"])
                .expect("should parse");
        assert!(args.headless);
        assert_eq!(args.ticks, 500);
        assert_eq!(args.seed, 7);
        assert!(!args.report);
    }

    #[test]
    fn report_flag_sets_report_to_true() {
        let args =
            CliArgs::try_parse_from(["worldsim", "--headless", "--report"]).expect("should parse");
        assert!(args.report);
    }

    #[test]
    fn defaults_match_expectations() {
        let args = CliArgs::try_parse_from(["worldsim"]).expect("should parse");
        assert!(!args.headless);
        assert_eq!(args.ticks, 1_000);
        assert_eq!(args.seed, 0);
        assert!(args.humans.is_none());
    }

    #[test]
    fn to_headless_config_copies_population_fields() {
        let args = CliArgs::try_parse_from([
            "worldsim",
            "--headless",
            "--humans",
            "10",
            "--deer",
            "2",
            "--berry-bushes",
            "1",
            "--apple-trees",
            "0",
        ])
        .expect("should parse");
        let config = args.to_headless_config();
        assert_eq!(config.humans, 10);
        assert_eq!(config.deer, 2);
        assert_eq!(config.berry_bushes, 1);
        assert_eq!(config.apple_trees, 0);
    }

    #[test]
    fn game_defaults_flag_sets_game_counts_when_no_overrides() {
        let args = CliArgs::try_parse_from(["worldsim", "--headless", "--game-defaults"])
            .expect("should parse");
        let config = args.to_headless_config();
        assert!(config.game_defaults);
        let game = WorldSpawnConfig::game_defaults();
        assert_eq!(config.humans, game.humans);
        assert_eq!(config.deer, game.deer);
        assert_eq!(config.berry_bushes, game.berry_bushes);
        assert_eq!(config.apple_trees, game.apple_trees);
    }

    #[test]
    fn game_defaults_with_humans_override() {
        let args = CliArgs::try_parse_from([
            "worldsim",
            "--headless",
            "--game-defaults",
            "--humans",
            "10",
        ])
        .expect("should parse");
        let config = args.to_headless_config();
        assert!(config.game_defaults);
        assert_eq!(config.humans, 10);
        // Other counts use game defaults
        let game = WorldSpawnConfig::game_defaults();
        assert_eq!(config.deer, game.deer);
        assert_eq!(config.berry_bushes, game.berry_bushes);
        assert_eq!(config.apple_trees, game.apple_trees);
    }

    #[test]
    fn parse_tick_range_parses_valid_range() {
        assert_eq!(parse_tick_range("10-20"), Some((10, 20)));
        assert_eq!(parse_tick_range("0-0"), Some((0, 0)));
        assert_eq!(parse_tick_range("4500-4600"), Some((4500, 4600)));
    }

    #[test]
    fn parse_tick_range_returns_none_for_invalid_input() {
        assert_eq!(parse_tick_range("abc-def"), None);
        assert_eq!(parse_tick_range("10_20"), None);
        assert_eq!(parse_tick_range("10"), None);
        assert_eq!(parse_tick_range(""), None);
        assert_eq!(parse_tick_range("-10"), None); // empty start
    }
}
