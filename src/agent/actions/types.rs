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
