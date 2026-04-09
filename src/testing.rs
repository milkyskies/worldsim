//! Lightweight headless harness for spawning agents, ticking the simulation, and asserting outcomes.
//!
//! Reads: agent components, world map, ontology, brain/nervous-system plugins
//! Writes: TestWorld (Bevy App with logic-only plugins), AgentConfig (spawn parameters)
//! Upstream: agent::AgentPlugin, agent::biology, agent::brains, agent::nervous_system
//! Downstream: integration tests for brains, planner, perception, knowledge, scenarios

mod config;
mod spawn;
mod world;

pub use config::AgentConfig;
pub use world::{SimEventLog, TestWorld};
