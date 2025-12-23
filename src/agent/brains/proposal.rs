use super::thinking::ActionTemplate;
use bevy::prelude::*;

/// Which brain is making a proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum BrainType {
    Survival,   // Reactive, immediate responses
    Emotional,  // Association-driven behavior
    Rational,   // Planning, multi-step reasoning
}

/// A proposal from one of the three brains
#[derive(Debug, Clone, Reflect)]
pub struct BrainProposal {
    /// Which brain is proposing this
    pub brain: BrainType,
    /// What action to take
    pub action: ActionTemplate,
    /// How urgently this brain wants to do this (0-100+)
    pub urgency: f32,
    /// Debug string explaining why this brain wants this
    pub reasoning: String,
}

/// Brain power calculations for arbitration
#[derive(Debug, Clone, Copy, Reflect, Default)]
pub struct BrainPowers {
    pub survival: f32,
    pub emotional: f32,
    pub rational: f32,
}

/// Component tracking the current brain decision state
/// Stores proposals from all brains and which one won
#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct BrainState {
    /// Proposals from each brain this frame
    #[reflect(ignore)]
    pub proposals: Vec<BrainProposal>,
    /// Current brain power levels
    pub powers: BrainPowers,
    /// Which brain won arbitration (if any)
    pub winner: Option<BrainType>,
    /// The action that was chosen (if any)
    #[reflect(ignore)]
    pub chosen_action: Option<ActionTemplate>,
}

impl BrainState {
    pub fn new() -> Self {
        Self::default()
    }
}
