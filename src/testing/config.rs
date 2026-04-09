//! AgentConfig: parameters for spawning a test agent with non-default needs and knowledge.
//!
//! Reads: nothing
//! Writes: AgentConfig (test-only struct with sensible defaults)
//! Upstream: nothing
//! Downstream: testing::world::TestWorld::spawn_agent

use crate::agent::mind::knowledge::Triple;
use crate::agent::psyche::personality::Personality;
use bevy::math::Vec2;

/// Configuration for a test agent. All fields default to neutral values so tests
/// only need to specify what they care about.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// World position the agent spawns at.
    pub pos: Vec2,
    /// Optional display name for the agent. Defaults to "TestPerson" if `None`.
    pub name: Option<String>,
    /// Hunger value (0.0 = full, 100.0 = starving).
    pub hunger: f32,
    /// Energy value (0.0 = exhausted, 100.0 = fully rested).
    pub energy: f32,
    /// Social drive (0.0 = satisfied, 1.0 = lonely).
    pub social_drive: f32,
    /// Personality traits. Defaults to all 0.5 (neutral).
    pub personality: Personality,
    /// Pre-loaded knowledge triples added to the agent's MindGraph at spawn.
    pub knowledge: Vec<Triple>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            pos: Vec2::ZERO,
            name: None,
            hunger: 0.0,
            energy: 100.0,
            social_drive: 0.5,
            personality: Personality::default(),
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
