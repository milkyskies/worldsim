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
    /// Jaws-as-weapon attack. Requires `Channel::Bite`, so only species
    /// whose anatomy provides it (wolves, future crocodiles, snakes) can
    /// perform it. Distinct from `Attack`, which needs `Manipulation` and
    /// covers hands / weapons / grapples.
    Bite,
    Flee,
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
