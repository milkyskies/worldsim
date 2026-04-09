//! Command-line interface parsing for the worldsim binary.
//!
//! Reads: process argv (via clap)
//! Writes: CliArgs (parsed flags), HeadlessConfig (derived from flags)
//! Upstream: main (binary entry point)
//! Downstream: headless::run_headless, the main Bevy app

use std::path::PathBuf;

use clap::Parser;

use crate::agent::brains::trace::{AgentFilter, TraceConfig, TraceFormat};
use crate::headless::HeadlessConfig;

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

    /// Number of human agents to spawn at startup (headless mode only).
    #[arg(long, default_value_t = 5)]
    pub humans: usize,

    /// Number of berry bushes to scatter (headless mode only).
    #[arg(long, default_value_t = 8)]
    pub berry_bushes: usize,

    /// Number of apple trees to scatter (headless mode only).
    #[arg(long, default_value_t = 4)]
    pub apple_trees: usize,

    /// Number of deer to scatter (headless mode only).
    #[arg(long, default_value_t = 3)]
    pub deer: usize,

    /// Enable decision trace logging. Use "all" to trace all agents or
    /// "agent:<name>" (e.g. "agent:alice") to trace a specific agent.
    /// Trace output is written to stderr (text) or the file set by
    /// --trace-file (JSONL). Only meaningful in --headless mode.
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
}

impl CliArgs {
    /// Builds a HeadlessConfig from these CLI arguments.
    pub fn to_headless_config(&self) -> HeadlessConfig {
        HeadlessConfig {
            ticks: self.ticks,
            seed: self.seed,
            humans: self.humans,
            berry_bushes: self.berry_bushes,
            apple_trees: self.apple_trees,
            deer: self.deer,
            trace: self.build_trace_config(),
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
        assert_eq!(args.humans, 5);
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
}
