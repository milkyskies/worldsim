//! AgentConfig: parameters for spawning a test agent with non-default needs and knowledge.
//!
//! Reads: nothing
//! Writes: AgentConfig (test-only struct with sensible defaults)
//! Upstream: nothing
//! Downstream: testing::world::TestWorld::spawn_agent

use crate::agent::body::genetics::genome::Genome;
use crate::agent::body::metabolism::Metabolism;
use crate::agent::culture::Culture;
use crate::agent::mind::knowledge::Triple;
use bevy::math::Vec2;

/// Configuration for a test agent. All fields default to neutral values so tests
/// only need to specify what they care about.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// World position the agent spawns at.
    pub pos: Vec2,
    /// Optional display name override. When `None`, the agent is assigned a
    /// unique name from the shared `NameCounters` resource (see
    /// `crate::agent::naming`).
    pub name: Option<String>,
    /// Metabolism state at spawn. Defaults to `Metabolism::well_fed()`.
    /// Tests that need a hungry agent use `Metabolism::at_urgency(0.8)` or
    /// `Metabolism::empty()` for full-on starvation.
    pub metabolism: Metabolism,
    /// Hydration satisfaction in `0..1` (0.0 = parched, 1.0 = fresh).
    pub hydration: f32,
    /// Stamina value (0.0 = exhausted, 100.0 = fully rested).
    pub stamina: f32,
    /// Wakefulness (0.0 = must sleep, 1.0 = fully rested).
    pub wakefulness: f32,
    /// Thermal comfort (0.0 = hypothermic, 1.0 = warm). Default is 1.0
    /// so tests that don't exercise the warmth drive ignore it entirely.
    pub warmth: f32,
    /// Optional override for baseline companionship satisfaction
    /// (0.0 = desperately lonely, 1.0 = content). `None` keeps the
    /// genome-derived value. `Some(v)` inserts a `SocialDriveOverride`
    /// component that `develop_phenotype_system` applies when deriving
    /// drives from the genome.
    pub social_drive: Option<f32>,
    /// Genome the phenotype, personality, and drives are derived from.
    /// Defaults to the neutral genome (all-zero loci → species baseline).
    /// Tests that want specific trait values use
    /// `Genome::from_phenotype(&Phenotype { ... })`.
    pub genome: Genome,
    /// Cultural baseline knowledge loaded into the MindGraph with
    /// `Source::Cultural` metadata. Defaults to `Culture::Nomad`, matching
    /// the windowed game's default spawn.
    pub culture: Culture,
    /// Extra knowledge triples added to the MindGraph on top of the cultural
    /// baseline (e.g. an episodic wolf-danger memory a test pre-seeds). These
    /// carry whatever metadata the caller put on them — `Source::Experienced`,
    /// `Source::Reported`, etc.
    pub knowledge: Vec<Triple>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            pos: Vec2::ZERO,
            name: None,
            metabolism: Metabolism::well_fed(),
            hydration: 1.0,
            stamina: 100.0,
            wakefulness: 1.0,
            warmth: 1.0,
            social_drive: None,
            genome: Genome::default(),
            culture: Culture::default(),
            knowledge: Vec::new(),
        }
    }
}

impl AgentConfig {
    /// Returns a config positioned at the given location, all other fields default.
    pub fn at(pos: Vec2) -> Self {
        Self {
            pos,
            ..Self::default()
        }
    }
}
