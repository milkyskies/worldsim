//! Conversation data types: shared state between two agents talking, owned by the [`CommunicationPlugin`](crate::agent::communication::CommunicationPlugin).
//!
//! Reads: nothing (pure data)
//! Writes: nothing (pure data)
//! Upstream: knowledge::Triple (turn content), psyche::emotions::Emotion (turn coloring)
//! Downstream: agent::communication (the plugin that owns and mutates these), ui (read-only display)

use crate::agent::mind::knowledge::{Concept, Triple};
use crate::agent::psyche::emotions::Emotion;
use bevy::prelude::*;

/// An active conversation between two agents.
///
/// Owned exclusively by [`ConversationManager`] inside the
/// [`CommunicationPlugin`](crate::agent::communication::CommunicationPlugin).
/// Turn ownership is encoded in the `turn` index — no flag on participant
/// components, so there's no race condition possible.
#[derive(Debug, Clone, Reflect)]
pub struct Conversation {
    pub id: u64,
    pub participants: [Entity; 2],
    /// Index into `participants` of the agent whose turn it currently is.
    pub turn: usize,
    pub state: ConversationState,
    pub started_at: u64,
    pub last_turn_at: u64,
    pub turns: Vec<Turn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Default)]
pub enum ConversationState {
    #[default]
    Greeting,
    Active,
    Wrapping,
    Ended,
}

/// One turn in a conversation - what one agent says.
#[derive(Debug, Clone, Reflect)]
pub struct Turn {
    pub speaker: Entity,
    pub intent: Intent,
    pub topic: Topic,
    pub emotion: Option<Emotion>,
    pub content: Vec<Triple>,
    pub timestamp: u64,
    pub expects_response: bool,
}

/// What the speaker is trying to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum Intent {
    Greet,
    Ask,
    Answer,
    Share,
    Empathize,
    Agree,
    Disagree,
    Thank,
    Farewell,
    Acknowledge,
}

/// What they're talking about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum Topic {
    General,
    Location(Concept),
    State(Entity),
    Person(Entity),
    Help,
}

impl Conversation {
    pub fn new(id: u64, participants: [Entity; 2], started_at: u64) -> Self {
        Self {
            id,
            participants,
            turn: 0,
            state: ConversationState::Greeting,
            started_at,
            last_turn_at: started_at,
            turns: Vec::new(),
        }
    }

    /// Append a turn and bump `last_turn_at`. Does not advance `turn` —
    /// callers should call [`Conversation::advance_turn`] separately.
    pub fn add_turn(&mut self, turn: Turn) {
        self.last_turn_at = turn.timestamp;
        self.turns.push(turn);
    }

    /// Flip turn ownership to the other participant.
    pub fn advance_turn(&mut self) {
        self.turn = 1 - self.turn;
    }

    /// Returns the entity whose turn it currently is.
    pub fn current_speaker(&self) -> Entity {
        self.participants[self.turn]
    }

    /// Returns the entity whose turn it currently is *not*.
    pub fn current_listener(&self) -> Entity {
        self.participants[1 - self.turn]
    }

    /// True if the most recent turn was a question expecting a response.
    pub fn last_turn_expects_response(&self) -> bool {
        self.turns
            .last()
            .map(|t| t.expects_response)
            .unwrap_or(false)
    }
}

/// Resource owned by the [`CommunicationPlugin`](crate::agent::communication::CommunicationPlugin) — the single source of truth for conversation state.
#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct ConversationManager {
    pub conversations: std::collections::HashMap<u64, Conversation>,
    pub next_id: u64,
}

impl ConversationManager {
    pub fn start_conversation(&mut self, participants: [Entity; 2], tick: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.conversations
            .insert(id, Conversation::new(id, participants, tick));
        id
    }

    pub fn get(&self, id: u64) -> Option<&Conversation> {
        self.conversations.get(&id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Conversation> {
        self.conversations.get_mut(&id)
    }

    /// Find an active (non-ended) conversation containing both participants.
    pub fn find_active(&self, a: Entity, b: Entity) -> Option<&Conversation> {
        self.conversations.values().find(|c| {
            c.state != ConversationState::Ended
                && c.participants.contains(&a)
                && c.participants.contains(&b)
        })
    }

    pub fn active_conversations(&self) -> impl Iterator<Item = &Conversation> {
        self.conversations
            .values()
            .filter(|c| c.state != ConversationState::Ended)
    }
}

/// Component attached to agents currently in a conversation.
///
/// Carries only the conversation handle and partner entity. Turn ownership
/// lives on [`Conversation::turn`] — keeping it off the component eliminates
/// the dual-write race that the old `my_turn`/`owes_response` flags suffered.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct InConversation {
    pub conversation_id: u64,
    pub partner: Entity,
}

/// Emitted when an agent leaves a conversation without saying goodbye.
/// Consumed by relationship and emotion systems to apply social penalties.
#[derive(Message, Debug, Clone)]
pub struct ConversationAbandoned {
    pub abandoner: Entity,
    pub abandoned: Entity,
    pub conversation_state: ConversationState,
}
