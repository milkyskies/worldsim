//! Agent event types: GameEvent, ActionOutcomeEvent, and SimEvent — the shared message bus for agent interactions.
//!
//! Reads: ActionType, Concept (item types), Triple (knowledge content)
//! Writes: GameEvent (Interaction, SocialInteraction, KnowledgeShared), ActionOutcomeEvent (Success/Failed), SimEvent (unified observability bus)
//! Upstream: action execution systems (emit outcomes), conversation system (emits KnowledgeShared)
//! Downstream: belief_updater (consumes ActionOutcomeEvent), relationship systems (consume SocialInteraction), SimEvent consumers (#84, #123, #124, #125)

use super::actions::ActionType;
use super::brains::proposal::{BrainPowers, BrainProposal, BrainType, Intent};
use super::psyche::emotions::EmotionType;
use crate::agent::mind::knowledge::Concept;
use bevy::prelude::*;
use std::sync::Arc;

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

/// How much a need changed when an action completed.
/// Pre-action levels are captured so joy can be scaled by urgency at the moment of relief.
#[derive(Debug, Clone, Default, Reflect)]
pub struct NeedSatisfaction {
    /// How much hunger dropped (positive = hunger went down).
    pub hunger_reduced: f32,
    /// How much thirst dropped (positive = thirst went down).
    pub thirst_reduced: f32,
    /// How much energy rose (positive = energy went up).
    pub energy_gained: f32,
    /// Hunger level just before the action completed (0–100).
    pub pre_hunger: f32,
    /// Thirst level just before the action completed (0–100).
    pub pre_thirst: f32,
}

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
        /// How much physical needs changed (if any)
        need_satisfaction: Option<NeedSatisfaction>,
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
    /// No drinkable water adjacent to agent
    NoWaterNearby,
    /// Agent lacks required crafting or building materials
    MissingMaterials,
}

/// Event for communicating action outcomes to belief update system
#[derive(Event, Message, Debug, Clone, Reflect)]
pub struct ActionOutcomeEvent {
    pub actor: Entity,
    pub outcome: ActionOutcome,
}

/// Unified event bus capturing every meaningful simulation state change.
///
/// Most variants carry `agent` and `tick` for uniform filtering.
/// Conversation variants use `participants` instead of a single `agent`.
/// Bevy events are free if unread — zero performance impact without consumers.
#[derive(Event, Message, Debug, Clone, Reflect)]
pub enum SimEvent {
    /// A brain decision was made: the arbitration system selected actions.
    Decision {
        agent: Entity,
        tick: u64,
        winner: Option<BrainType>,
        chosen_actions: Vec<ActionType>,
        powers: BrainPowers,
        #[reflect(ignore)]
        proposals: Arc<Vec<BrainProposal>>,
    },

    /// An action was admitted into the running set.
    ActionStarted {
        agent: Entity,
        tick: u64,
        action: ActionType,
        target: Option<Entity>,
    },

    /// An action completed normally.
    ActionCompleted {
        agent: Entity,
        tick: u64,
        action: ActionType,
    },

    /// An action was preempted to make room for a higher-priority action.
    ActionPreempted {
        agent: Entity,
        tick: u64,
        preempted_action: ActionType,
    },

    /// An action failed its can_start check.
    ActionFailed {
        agent: Entity,
        tick: u64,
        action: ActionType,
        reason: FailureReason,
    },

    /// An active plan was abandoned (stalled out or replaced by a better proposal).
    PlanAbandoned {
        agent: Entity,
        tick: u64,
        action: ActionType,
        intent: Intent,
    },

    /// A conversation was started between participants.
    ConversationStarted {
        participants: Vec<Entity>,
        tick: u64,
        conversation_id: u64,
    },

    /// A conversation ended.
    ConversationEnded {
        participants: Vec<Entity>,
        tick: u64,
        conversation_id: u64,
    },

    /// A conversation was abandoned rudely (no farewell).
    ConversationAbandoned {
        abandoner: Entity,
        abandoned: Entity,
        tick: u64,
    },

    /// A relationship dimension changed between two agents.
    RelationshipChanged {
        agent: Entity,
        other: Entity,
        tick: u64,
        dimension: RelationshipDimension,
        old_value: f32,
        new_value: f32,
    },

    /// An emotion was triggered or reinforced.
    EmotionTriggered {
        agent: Entity,
        tick: u64,
        emotion: EmotionType,
        intensity: f32,
    },

    /// An agent died.
    Death {
        agent: Entity,
        tick: u64,
        cause: String,
    },

    /// An agent perceived a new entity (wasn't visible last tick).
    EntityPerceived {
        agent: Entity,
        tick: u64,
        target: Entity,
    },

    /// An agent recognized a stranger (first encounter).
    StrangerDetected {
        agent: Entity,
        tick: u64,
        stranger: Entity,
    },

    /// Knowledge was shared between agents.
    KnowledgeShared {
        speaker: Entity,
        listener: Entity,
        tick: u64,
        triple_count: usize,
    },
}

/// Which relationship dimension changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum RelationshipDimension {
    Trust,
    Affection,
}
