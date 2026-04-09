//! Command-line interface parsing for the worldsim binary.
//!
//! Reads: process argv (via clap)
//! Writes: CliArgs (parsed flags), HeadlessConfig (derived from flags)
//! Upstream: main (binary entry point)
//! Downstream: headless::run_headless, the main Bevy app

use clap::Parser;

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
        }
    }
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
