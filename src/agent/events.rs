use super::actions::ActionType;
use crate::agent::mind::knowledge::Concept;
use bevy::prelude::*;

/// Topics for conversations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum ConversationTopic {
    Greetings, // Small talk, weather
    Knowledge, // Share belief about the world
    Feelings,  // Express emotions
    Gossip,    // Share beliefs about other agents
    Request,   // Ask for something
}

#[derive(Event, Message, Debug, Clone, Reflect)]
pub enum GameEvent {
    /// An atomic interaction happened in the world.
    /// "Bob waved at Alice", "Bob ate an Apple".
    Interaction {
        actor: Entity,
        action: ActionType,
        target: Option<Entity>,
        location: Option<Vec2>,
    },

    /// A social interaction between agents
    /// Used for relationship updates
    SocialInteraction {
        actor: Entity,
        target: Entity,
        action: ActionType,
        topic: Option<ConversationTopic>,
        /// -1.0 (hostile) to 1.0 (friendly)
        valence: f32,
    },

    /// Knowledge being shared from one agent to another
    KnowledgeShared {
        speaker: Entity,
        listener: Entity,
        /// The knowledge being shared (as Triples)
        content: Vec<crate::agent::mind::knowledge::Triple>,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// ACTION OUTCOMES — Results of actions that update beliefs
// ═══════════════════════════════════════════════════════════════════════════

/// The result of an attempted action
#[derive(Debug, Clone, Reflect)]
pub enum ActionOutcome {
    /// Action succeeded - effects should be applied to beliefs
    Success {
        action: ActionType,
        target: Option<Entity>,
        /// What was gained (item concept, quantity)
        gained: Option<(Concept, u32)>,
        /// What was consumed (item concept, quantity)
        consumed: Option<(Concept, u32)>,
    },
    /// Action failed - learn why
    Failed {
        action: ActionType,
        target: Option<Entity>,
        reason: FailureReason,
    },
}

/// Why an action failed
#[derive(Debug, Clone, Reflect, PartialEq)]
pub enum FailureReason {
    /// Target no longer exists or is invalid
    TargetGone,
    /// No target specified when one is required
    NoTarget,
    /// Target has no resources left
    ResourceDepleted,
    /// Agent doesn't have required item
    MissingItem(Concept),
    /// Agent has no edible food
    NoEdibleFood,
    /// Agent is too far from target
    TooFar,
    /// Interrupted by something else
    Interrupted,
    /// Path is blocked
    PathBlocked,
    /// Already did this (e.g., already introduced)
    AlreadyDone,
}

/// Event for communicating action outcomes to belief update system
#[derive(Event, Message, Debug, Clone, Reflect)]
pub struct ActionOutcomeEvent {
    pub actor: Entity,
    pub outcome: ActionOutcome,
}
