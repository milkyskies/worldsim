//! `Engaged` component, `EngagementKind`, and `EngagementId`. Per-kind
//! action-ownership / conflict rules live on the enum so arbitration
//! can stay kind-agnostic.

use bevy::prelude::*;

use crate::agent::actions::types::ActionType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, serde::Serialize)]
pub struct EngagementId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, serde::Serialize)]
pub enum EngagementKind {
    Converse,
    Hunt,
    Devour,
    Harvest,
    Flee,
    Sleep,
}

impl EngagementKind {
    /// True when `action` is part of this kind's own action set —
    /// arbitration must not reject these on the engagement-commitment
    /// gate. New kinds add an arm here, not in arbitration.
    pub fn owns_action(self, action: ActionType) -> bool {
        match self {
            Self::Converse => matches!(
                action,
                ActionType::Converse | ActionType::InitiateConversation
            ),
            Self::Hunt => matches!(
                action,
                ActionType::InitiateHunt | ActionType::Bite | ActionType::Walk
            ),
            Self::Devour => matches!(
                action,
                ActionType::InitiateDevour | ActionType::Devour
            ),
            Self::Harvest => matches!(
                action,
                ActionType::InitiateHarvest | ActionType::Harvest
            ),
            Self::Flee => matches!(
                action,
                ActionType::InitiateFlee | ActionType::Flee
            ),
            Self::Sleep => matches!(
                action,
                ActionType::InitiateSleep | ActionType::Sleep | ActionType::WakeUp
            ),
        }
    }
}

/// Component attached to agents currently inside an engagement. The
/// payload (turns, participants, etc.) lives in the kind's own
/// resource (e.g. [`super::converse::ConverseRegistry`]) keyed by
/// [`Engaged::id`].
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct Engaged {
    pub kind: EngagementKind,
    pub id: EngagementId,
}

impl Engaged {
    pub fn new(kind: EngagementKind, id: EngagementId) -> Self {
        Self { kind, id }
    }
}

/// Surfaced on `EngagementEnded` so relationship updaters can
/// distinguish a graceful close from an abandonment from an
/// emotion-driven break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, serde::Serialize)]
pub enum EngagementEndReason {
    Natural,
    Stale,
    OutOfRange,
    EmotionOverride,
    Abandoned,
    Other,
}
