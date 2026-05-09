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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default, serde::Serialize)]
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
    /// Thermal comfort: warm up by a heat source, build one if none exists.
    SatisfyWarmth,
    /// Rest quality: sleep in a shelter, build one if none exists.
    SatisfyRestQuality,
    /// Food security: build a stockpile chest, check on existing ones.
    SatisfyFoodSecurity,
    /// Explore for its own sake, not to serve another drive.
    SatisfyCuriosity,
    /// Reserved for future reproduction drive.
    SatisfyReproduction,
    /// Fulfill a verbal promise made to another agent.
    FulfillCommitment,
    /// Idle, ambient, or "nothing specific" behavior.
    #[default]
    None,
}

impl Intent {
    /// Map a nervous-system urgency source to the intent that satisfies
    /// it. Reads the drive registry; unregistered sources fall back to
    /// `Intent::None`.
    pub fn from_urgency_source(source: UrgencySource) -> Self {
        crate::agent::drive_registry::by_urgency(source)
            .map(|e| e.intent)
            .unwrap_or(Intent::None)
    }
}

/// Which brain is making a proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, serde::Serialize)]
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
#[derive(Debug, Clone, Copy, Reflect, Default, serde::Serialize)]
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
    /// Fingerprint of the admitted (brain, action_name) set the last time we
    /// wrote a brain log line. Used to suppress per-tick "still doing the same
    /// thing" spam — we only emit a new log when the set of admitted actions
    /// changes. `None` forces the next tick to log (fresh-spawned agents,
    /// entities whose admitted set went empty last tick).
    #[reflect(ignore)]
    pub last_logged: Option<Vec<(BrainType, String)>>,
}

impl BrainState {
    pub fn new() -> Self {
        Self::default()
    }
}
