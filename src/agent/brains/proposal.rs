//! Brain proposal types: BrainProposal, BrainType, BrainPowers, and the BrainState component.
//!
//! Reads: ActionTemplate (from thinking), BrainType tag
//! Writes: BrainProposal, BrainPowers, BrainState (ECS component holding all proposals and the winner)
//! Upstream: thinking (ActionTemplate), all brain modules that create proposals
//! Downstream: arbitration (selects winner), brain_system (reads BrainState result)

use super::thinking::ActionTemplate;
use crate::agent::nervous_system::urgency::UrgencySource;
use bevy::prelude::*;

/// The drive a brain proposal is trying to satisfy.
///
/// Arbitration uses this to deduplicate proposals: if two brains both
/// propose actions targeting the same drive (e.g. Walk-to-apple-tree and
/// Explore-for-food both satisfy Hunger), only the highest-scoring one
/// survives. This prevents parallel conflicting strategies for the same need.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default)]
pub enum Intent {
    SatisfyHunger,
    SatisfyThirst,
    SatisfyStamina,
    SatisfySocial,
    /// Flee, hide, defend against threats.
    SatisfySafety,
    /// Pain relief: injury-driven behavior (e.g. can't move while hurt).
    SatisfyPainRelief,
    SatisfyTerritoriality,
    /// Sleep pressure from wakefulness decay, independent of stamina fatigue.
    SatisfySleepiness,
    /// Explore for its own sake, not to serve another drive.
    SatisfyCuriosity,
    /// Reserved for future reproduction drive.
    SatisfyReproduction,
    /// Idle, ambient, or "nothing specific" behavior.
    #[default]
    None,
}

impl Intent {
    /// Map a nervous-system urgency source to the intent that satisfies it.
    pub fn from_urgency_source(source: UrgencySource) -> Self {
        match source {
            UrgencySource::Hunger => Intent::SatisfyHunger,
            UrgencySource::Thirst => Intent::SatisfyThirst,
            UrgencySource::Stamina => Intent::SatisfyStamina,
            UrgencySource::Social => Intent::SatisfySocial,
            UrgencySource::Fear => Intent::SatisfySafety,
            UrgencySource::Pain => Intent::SatisfyPainRelief,
            UrgencySource::Territoriality => Intent::SatisfyTerritoriality,
            UrgencySource::Fun | UrgencySource::Curiosity => Intent::SatisfyCuriosity,
            UrgencySource::Sleepiness => Intent::SatisfySleepiness,
        }
    }
}

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
    /// Which drive this proposal is trying to satisfy. Arbitration
    /// deduplicates proposals with the same intent, keeping the highest-scoring.
    pub intent: Intent,
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
