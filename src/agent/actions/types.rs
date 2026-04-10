//! Action type enum: the verbs agents can perform, separated from events.
//!
//! Reads: nothing (leaf type)
//! Writes: ActionType (used as value across the agent stack)
//! Upstream: none
//! Downstream: actions::registry, brains, nervous_system, ui::character_sheet

use bevy::prelude::*;

/// Defines the objective "verbs" agents can perform.
/// This separates Intent (Action) from Occurrence (Event).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default)]
pub enum ActionType {
    // Survival / Biological
    Eat,
    Sleep,
    WakeUp, // NEW: Transition from sleep to awake
    Drink,

    // Resource
    Harvest,
    Pickup,
    Drop,
    Build,
    /// Move items from the agent's own slots into a target entity's slots.
    /// Polymorphic across construction sites, chests, furnaces, etc. — the
    /// target's `ItemSlots` filters and access rules decide what's accepted.
    Deposit,
    /// Move items from a target entity's slots into the agent's own slots.
    /// Polymorphic across chests, dropped piles, furnace outputs, etc. —
    /// the target's `extract_access` decides what can leave.
    Take,

    // Movement / Positioning
    Walk,    // "Travel"
    Wander,  // Random short-distance movement
    Explore, // Directed long-distance exploration to find resources
    #[default]
    Idle,

    // Social / Combat
    Wave,
    /// Walk-to-target marker proposed by brains to start a conversation.
    /// Owned by the CommunicationPlugin — on arrival within CONVERSATION_RANGE
    /// the plugin swaps this for `Converse` and registers a Conversation.
    InitiateConversation,
    /// Body-channel marker for being in a conversation. Inserted and removed
    /// by the CommunicationPlugin — never proposed by brains directly.
    Converse,
    Attack,
    Flee,
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl ActionType {
    /// Human-readable present-participle verb ("Eating", "Fleeing from", ...)
    /// for the character sheet. Verbs ending in a preposition imply a target
    /// follows (e.g. "Walking to <place>", "Attacking <target>").
    pub fn verb(self) -> &'static str {
        match self {
            ActionType::Eat => "Eating",
            ActionType::Sleep => "Sleeping",
            ActionType::WakeUp => "Waking up",
            ActionType::Drink => "Drinking",
            ActionType::Harvest => "Harvesting",
            ActionType::Pickup => "Picking up",
            ActionType::Drop => "Dropping",
            ActionType::Build => "Building",
            ActionType::Deposit => "Depositing into",
            ActionType::Take => "Taking from",
            ActionType::Walk => "Walking to",
            ActionType::Wander => "Wandering",
            ActionType::Explore => "Exploring",
            ActionType::Idle => "Idle",
            ActionType::Wave => "Waving at",
            ActionType::InitiateConversation => "Approaching",
            ActionType::Converse => "Talking to",
            ActionType::Attack => "Attacking",
            ActionType::Flee => "Fleeing from",
        }
    }
}
