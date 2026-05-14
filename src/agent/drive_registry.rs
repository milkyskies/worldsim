//! Unified drive registry: one row per drive declaring every per-drive fact
//! that was previously scattered across `body::need`, `nervous_system::urgency`,
//! `brains::proposal`, and `brains::rational`.
//!
//! Reads: nothing (pure static data)
//! Writes: nothing
//! Upstream: nothing
//! Downstream: `NeedKind::satisfier`, `NeedKind::satiation_threshold`,
//!             `UrgencySource::survival_weight`, `UrgencySource::is_deprivation`,
//!             `Intent::from_urgency_source`, `goal_for_urgency`,
//!             `NervousSystemConfig::default` (drive list)
//!
//! Adding a new drive: add a row to `DRIVE_REGISTRY`. Every dispatch function
//! that reads the registry picks it up automatically.
//!
//! Drives NOT in the registry are the Maslow-tier read-only satisfaction
//! drives (Safety, Esteem, Autonomy) on `NeedKind`. They have no urgency
//! source and no satisfier action, so the registry doesn't model them —
//! `NeedKind::satisfier` / `satiation_threshold` fall back to (None, 1.0)
//! for anything the registry doesn't cover.

use crate::agent::actions::ActionType;
use crate::agent::body::need::NeedKind;
use crate::agent::brains::proposal::Intent;
use crate::agent::mind::knowledge::Predicate;
use crate::agent::nervous_system::urgency::UrgencySource;

/// How to construct the GOAP goal triple for this drive. Pattern is
/// consumed by `goal_for_urgency`; `None` means no rational-brain goal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GoalPattern {
    /// `(Self_, predicate, target)` — the seven standard drives fit this shape.
    SelfHas {
        predicate: Predicate,
        target_quantity: f32,
    },
    /// Commitment: reuse the conditions of the highest-commitment
    /// `PlanSource::VerbalCommitment` plan in PlanMemory.
    HighestCommitmentPlan,
}

/// One row in the drive registry — every per-drive fact that used to be
/// scattered across the codebase. Fields that don't apply (e.g.
/// `need_kind` for Commitment, `goal_pattern` for purely emotional
/// drives) are `None`.
#[derive(Debug)]
pub struct DriveEntry {
    pub urgency: UrgencySource,
    pub need_kind: Option<NeedKind>,
    pub intent: Intent,
    pub satisfier: Option<ActionType>,
    /// `NeedKind::satiation_threshold()` return value. Needs without a
    /// satisfier still report `1.0` via the fallback in `satiation_threshold`.
    pub satiation_threshold: f32,
    pub survival_weight: f32,
    pub is_deprivation: bool,
    pub goal_pattern: Option<GoalPattern>,
    pub display_name: &'static str,
}

/// The canonical drive table. Order is irrelevant — lookup helpers scan
/// the slice linearly (12 entries, a handful of ns per call).
pub const DRIVE_REGISTRY: &[DriveEntry] = &[
    DriveEntry {
        urgency: UrgencySource::Hunger,
        need_kind: Some(NeedKind::Hunger),
        intent: Intent::SatisfyHunger,
        satisfier: Some(ActionType::Eat),
        satiation_threshold: 0.8,
        survival_weight: 100.0,
        is_deprivation: true,
        goal_pattern: Some(GoalPattern::SelfHas {
            predicate: Predicate::Hunger,
            target_quantity: 0.0,
        }),
        display_name: "Hunger",
    },
    DriveEntry {
        urgency: UrgencySource::Thirst,
        need_kind: Some(NeedKind::Thirst),
        intent: Intent::SatisfyThirst,
        satisfier: Some(ActionType::Drink),
        satiation_threshold: 0.95,
        survival_weight: 100.0,
        is_deprivation: true,
        goal_pattern: Some(GoalPattern::SelfHas {
            predicate: Predicate::Thirst,
            target_quantity: 0.0,
        }),
        display_name: "Thirst",
    },
    DriveEntry {
        urgency: UrgencySource::Pain,
        need_kind: Some(NeedKind::Pain),
        intent: Intent::SatisfyPainRelief,
        satisfier: None,
        satiation_threshold: 1.0,
        survival_weight: 100.0,
        is_deprivation: true,
        goal_pattern: Some(GoalPattern::SelfHas {
            predicate: Predicate::Pain,
            target_quantity: 0.0,
        }),
        display_name: "Pain",
    },
    DriveEntry {
        urgency: UrgencySource::Warmth,
        need_kind: Some(NeedKind::Warmth),
        intent: Intent::SatisfyWarmth,
        satisfier: Some(ActionType::WarmUp),
        satiation_threshold: 0.95,
        survival_weight: 90.0,
        is_deprivation: true,
        goal_pattern: Some(GoalPattern::SelfHas {
            predicate: Predicate::Warmth,
            target_quantity: 100.0,
        }),
        display_name: "Warmth",
    },
    DriveEntry {
        urgency: UrgencySource::RestQuality,
        need_kind: Some(NeedKind::RestQuality),
        intent: Intent::SatisfyRestQuality,
        satisfier: Some(ActionType::RestInShelter),
        satiation_threshold: 0.95,
        survival_weight: 70.0,
        is_deprivation: true,
        goal_pattern: Some(GoalPattern::SelfHas {
            predicate: Predicate::RestQuality,
            target_quantity: 100.0,
        }),
        display_name: "Rest Quality",
    },
    DriveEntry {
        urgency: UrgencySource::FoodSecurity,
        need_kind: Some(NeedKind::FoodSecurity),
        intent: Intent::SatisfyFoodSecurity,
        satisfier: Some(ActionType::StockChest),
        satiation_threshold: 0.95,
        // Lower than acute drives: food-security is a tomorrow-concern
        // and should lose arbitration to live hunger / thirst / pain.
        survival_weight: 30.0,
        is_deprivation: false,
        goal_pattern: Some(GoalPattern::SelfHas {
            predicate: Predicate::FoodSecurity,
            target_quantity: 100.0,
        }),
        display_name: "Food Security",
    },
    DriveEntry {
        urgency: UrgencySource::Stamina,
        need_kind: Some(NeedKind::Stamina),
        intent: Intent::SatisfyStamina,
        satisfier: Some(ActionType::Rest),
        satiation_threshold: 0.95,
        survival_weight: 80.0,
        is_deprivation: false,
        goal_pattern: Some(GoalPattern::SelfHas {
            predicate: Predicate::Stamina,
            target_quantity: 100.0,
        }),
        display_name: "Stamina",
    },
    DriveEntry {
        urgency: UrgencySource::Sleepiness,
        need_kind: Some(NeedKind::Sleep),
        intent: Intent::SatisfySleepiness,
        // The Sleep beat is engagement-owned (#746); the proposable
        // entry point is InitiateSleep.
        satisfier: Some(ActionType::InitiateSleep),
        satiation_threshold: 0.95,
        survival_weight: 80.0,
        is_deprivation: false,
        goal_pattern: None,
        display_name: "Sleepiness",
    },
    DriveEntry {
        urgency: UrgencySource::Fear,
        need_kind: Some(NeedKind::Fear),
        intent: Intent::SatisfySafety,
        satisfier: None,
        satiation_threshold: 1.0,
        survival_weight: 50.0,
        is_deprivation: false,
        goal_pattern: None,
        display_name: "Fear",
    },
    DriveEntry {
        urgency: UrgencySource::Social,
        need_kind: Some(NeedKind::Social),
        intent: Intent::SatisfySocial,
        satisfier: Some(ActionType::InitiateConversation),
        satiation_threshold: 0.9,
        survival_weight: 0.0,
        is_deprivation: false,
        goal_pattern: Some(GoalPattern::SelfHas {
            predicate: Predicate::SocialDrive,
            target_quantity: 0.0,
        }),
        display_name: "Social",
    },
    DriveEntry {
        urgency: UrgencySource::Fun,
        need_kind: Some(NeedKind::Fun),
        intent: Intent::SatisfyCuriosity,
        satisfier: Some(ActionType::Explore),
        satiation_threshold: 0.9,
        survival_weight: 0.0,
        is_deprivation: false,
        goal_pattern: None,
        display_name: "Fun",
    },
    DriveEntry {
        urgency: UrgencySource::Curiosity,
        need_kind: Some(NeedKind::Curiosity),
        intent: Intent::SatisfyCuriosity,
        satisfier: Some(ActionType::Explore),
        satiation_threshold: 0.9,
        survival_weight: 0.0,
        is_deprivation: false,
        goal_pattern: None,
        display_name: "Curiosity",
    },
    DriveEntry {
        urgency: UrgencySource::Territoriality,
        need_kind: Some(NeedKind::Territory),
        intent: Intent::SatisfyTerritoriality,
        satisfier: None,
        satiation_threshold: 1.0,
        survival_weight: 0.0,
        is_deprivation: false,
        goal_pattern: None,
        display_name: "Territoriality",
    },
    DriveEntry {
        urgency: UrgencySource::Commitment,
        need_kind: None,
        intent: Intent::FulfillCommitment,
        satisfier: None,
        satiation_threshold: 1.0,
        survival_weight: 0.0,
        is_deprivation: false,
        goal_pattern: Some(GoalPattern::HighestCommitmentPlan),
        display_name: "Commitment",
    },
    DriveEntry {
        urgency: UrgencySource::Compassion,
        need_kind: None,
        intent: Intent::SatisfyCompassion,
        // Satisfier actions (TendWounds, ShareFood, Comfort) are wired
        // in follow-ups once perceived-injury / -hunger channels land.
        satisfier: None,
        satiation_threshold: 1.0,
        // Below acute self-deprivation but above pure social drives —
        // a peer's distress matters, but a starving observer can't help
        // anyone if they don't eat first.
        survival_weight: 40.0,
        is_deprivation: false,
        goal_pattern: None,
        display_name: "Compassion",
    },
];

pub fn by_urgency(source: UrgencySource) -> Option<&'static DriveEntry> {
    DRIVE_REGISTRY.iter().find(|e| e.urgency == source)
}

pub fn by_need(kind: NeedKind) -> Option<&'static DriveEntry> {
    DRIVE_REGISTRY.iter().find(|e| e.need_kind == Some(kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_urgency_source_has_a_registry_entry() {
        for source in [
            UrgencySource::Hunger,
            UrgencySource::Thirst,
            UrgencySource::Pain,
            UrgencySource::Warmth,
            UrgencySource::RestQuality,
            UrgencySource::FoodSecurity,
            UrgencySource::Stamina,
            UrgencySource::Sleepiness,
            UrgencySource::Fear,
            UrgencySource::Social,
            UrgencySource::Fun,
            UrgencySource::Curiosity,
            UrgencySource::Territoriality,
            UrgencySource::Commitment,
            UrgencySource::Compassion,
        ] {
            assert!(
                by_urgency(source).is_some(),
                "missing drive registry entry for {source:?}"
            );
        }
    }

    #[test]
    fn every_registry_entry_has_distinct_urgency() {
        for (i, entry) in DRIVE_REGISTRY.iter().enumerate() {
            for other in &DRIVE_REGISTRY[i + 1..] {
                assert_ne!(
                    entry.urgency, other.urgency,
                    "duplicate UrgencySource in DRIVE_REGISTRY: {:?}",
                    entry.urgency
                );
            }
        }
    }
}
