//! Brain proposal types: BrainProposal, BrainType, BrainPowers, and the BrainState component.
//!
//! Reads: ActionTemplate (from thinking), BrainType tag
//! Writes: BrainProposal, BrainPowers, BrainState (ECS component holding all proposals and the winner)
//! Upstream: thinking (ActionTemplate), all brain modules that create proposals
//! Downstream: arbitration (selects winner), brain_system (reads BrainState result)

use super::thinking::ActionTemplate;
use bevy::prelude::*;

/// Which brain is making a proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum BrainType {
    Survival,  // Reactive, immediate responses
    Emotional, // Association-driven behavior
    Rational,  // Planning, multi-step reasoning
}

impl BrainType {
    /// Returns the display name for this brain type (title-case).
    pub fn display_name(self) -> &'static str {
        match self {
            BrainType::Survival => "Survival",
            BrainType::Emotional => "Emotional",
            BrainType::Rational => "Rational",
        }
    }

    /// Returns the power level for this brain from a [`BrainPowers`] snapshot.
    pub fn power(self, powers: &BrainPowers) -> f32 {
        match self {
            BrainType::Survival => powers.survival,
            BrainType::Emotional => powers.emotional,
            BrainType::Rational => powers.rational,
        }
    }
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

/// Component tracking the current brain decision state.
///
/// With the action channel system, multiple proposals can be admitted in
/// parallel as long as their body channels don't hard-conflict. The "winner"
/// is the highest-scoring brain whose proposal made it into `chosen_actions`.
#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct BrainState {
    /// Proposals from each brain this frame
    #[reflect(ignore)]
    pub proposals: Vec<BrainProposal>,
    /// Current brain power levels
    pub powers: BrainPowers,
    /// Which brain produced the highest-scoring admitted proposal (if any)
    pub winner: Option<BrainType>,
    /// All actions admitted this tick - parallel runs if channels are compatible.
    #[reflect(ignore)]
    pub chosen_actions: Vec<ActionTemplate>,
}

impl BrainState {
    pub fn new() -> Self {
        Self::default()
    }
}
