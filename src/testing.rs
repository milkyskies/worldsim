//! Lightweight headless harness for spawning agents, ticking the simulation, and asserting outcomes.
//!
//! Reads: agent components, world map, ontology, brain/nervous-system plugins
//! Writes: TestWorld (Bevy App with logic-only plugins), AgentConfig (spawn parameters), SimEventLog (auto-collected event history)
//! Upstream: agent::AgentPlugin, agent::biology, agent::brains, agent::nervous_system
//! Downstream: integration tests for brains, planner, perception, knowledge, scenarios

mod config;
pub(crate) mod scenario;
mod spawn;
mod world;

// Re-export the fluent genome builders from their real home so test files
// can `use worldsim::testing::{personality, physical, genome};` without
// knowing where they live in the genetics module.
pub use crate::agent::body::genetics::builder::{
    GenomeBuilder, PersonalityBuilder, PhysicalBuilder, genome, personality, physical,
};
pub use config::AgentConfig;
pub use scenario::{RelBuilder, ScenarioBuilder, ScenarioEntities};
pub use world::{SimEventLog, TestWorld};
