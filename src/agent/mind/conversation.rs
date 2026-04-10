//! Conversation data types: shared state between agents talking, owned by the [`CommunicationPlugin`](crate::agent::communication::CommunicationPlugin).
//!
//! Reads: nothing (pure data)
//! Writes: nothing (pure data)
//! Upstream: knowledge::Triple (turn content), psyche::emotions::Emotion (turn coloring)
//! Downstream: agent::communication (the plugin that owns and mutates these), ui (read-only display)

use crate::agent::mind::knowledge::{Concept, Triple};
use crate::agent::psyche::emotions::Emotion;
use bevy::prelude::*;
use std::collections::HashSet;

/// Maximum number of participants in a single group conversation. Groups
/// that hit this cap refuse new joiners (attention limit — beyond ~6 people
/// you stop tracking who said what and the conversation splinters).
pub const MAX_GROUP_SIZE: usize = 6;

/// An active conversation between two or more agents.
///
/// Owned exclusively by [`ConversationManager`] inside the
/// [`CommunicationPlugin`](crate::agent::communication::CommunicationPlugin).
/// Turn ownership is encoded in the `turn` index — no flag on participant
/// components, so there's no race condition possible.
#[derive(Debug, Clone, Reflect)]
pub struct Conversation {
    pub id: u64,
    pub participants: Vec<Entity>,
    /// Index into `participants` of the agent whose turn it currently is.
    pub turn: usize,
    pub state: ConversationState,
    pub started_at: u64,
    pub last_turn_at: u64,
    pub turns: Vec<Turn>,
    /// Listeners who signalled they want to speak next. Consumed by the
    /// personality-weighted speaker selection when the current turn advances.
    pub wants_to_speak: HashSet<Entity>,
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
    pub fn new(id: u64, participants: Vec<Entity>, started_at: u64) -> Self {
        Self {
            id,
            participants,
            turn: 0,
            state: ConversationState::Greeting,
            started_at,
            last_turn_at: started_at,
            turns: Vec::new(),
            wants_to_speak: HashSet::default(),
        }
    }

    /// Append a turn and bump `last_turn_at`. Does not advance `turn` —
    /// callers should call [`Conversation::set_speaker`] separately once the
    /// next speaker has been chosen.
    pub fn add_turn(&mut self, turn: Turn) {
        self.last_turn_at = turn.timestamp;
        self.turns.push(turn);
    }

    /// Set the next speaker by entity. The entity must already be a
    /// participant — otherwise the call is ignored.
    pub fn set_speaker(&mut self, speaker: Entity) {
        if let Some(idx) = self.participants.iter().position(|e| *e == speaker) {
            self.turn = idx;
        }
    }

    /// Returns the entity whose turn it currently is.
    ///
    /// Invariant: `self.turn < self.participants.len()` whenever
    /// `participants` is non-empty. `remove_participant` re-clamps `turn`
    /// on every removal, `set_speaker` only accepts existing participants,
    /// and conversations with fewer than two participants are marked
    /// `Ended` by `evaluate_conversation_continuation` before the next
    /// turn is selected.
    pub fn current_speaker(&self) -> Entity {
        self.participants[self.turn]
    }

    /// Iterate over every participant except `speaker`. Used by systems
    /// that process a specific turn (where the speaker is `turn.speaker`,
    /// which may differ from the current `current_speaker()` after the
    /// floor advances).
    pub fn listeners_for(&self, speaker: Entity) -> impl Iterator<Item = Entity> + '_ {
        self.participants
            .iter()
            .copied()
            .filter(move |e| *e != speaker)
    }

    /// Iterate over all non-speaker participants (speaker = current turn).
    pub fn listeners(&self) -> impl Iterator<Item = Entity> + '_ {
        self.listeners_for(self.current_speaker())
    }

    /// True if the most recent turn was a question expecting a response.
    pub fn last_turn_expects_response(&self) -> bool {
        self.turns
            .last()
            .map(|t| t.expects_response)
            .unwrap_or(false)
    }

    /// Add a new participant to an ongoing conversation. Returns false if
    /// the group is already at capacity or the entity is already a member.
    pub fn add_participant(&mut self, entity: Entity) -> bool {
        if self.participants.contains(&entity) {
            return false;
        }
        if self.participants.len() >= MAX_GROUP_SIZE {
            return false;
        }
        self.participants.push(entity);
        true
    }

    /// Remove a participant. Keeps the current speaker stable when possible
    /// by shifting `turn` to track the same entity across the removal.
    pub fn remove_participant(&mut self, entity: Entity) {
        let speaker_before = self.participants.get(self.turn).copied();
        self.participants.retain(|e| *e != entity);
        self.wants_to_speak.remove(&entity);

        if let Some(prev) = speaker_before
            && prev != entity
            && let Some(idx) = self.participants.iter().position(|e| *e == prev)
        {
            self.turn = idx;
        } else if !self.participants.is_empty() {
            self.turn %= self.participants.len();
        } else {
            self.turn = 0;
        }
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
    pub fn start_conversation(&mut self, participants: Vec<Entity>, tick: u64) -> u64 {
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

    /// Find any active conversation containing the given entity.
    pub fn find_active_for(&self, entity: Entity) -> Option<&Conversation> {
        self.conversations
            .values()
            .find(|c| c.state != ConversationState::Ended && c.participants.contains(&entity))
    }

    /// Find any active conversation containing the given entity (mutable).
    pub fn find_active_for_mut(&mut self, entity: Entity) -> Option<&mut Conversation> {
        self.conversations
            .values_mut()
            .find(|c| c.state != ConversationState::Ended && c.participants.contains(&entity))
    }

    pub fn active_conversations(&self) -> impl Iterator<Item = &Conversation> {
        self.conversations
            .values()
            .filter(|c| c.state != ConversationState::Ended)
    }
}

/// Component attached to agents currently in a conversation.
///
/// Carries only the conversation handle. Turn ownership lives on
/// [`Conversation::turn`]; the list of other participants lives on
/// [`Conversation::participants`] — looking it up via the manager keeps the
/// two places from drifting.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct InConversation {
    pub conversation_id: u64,
}

/// Emitted when an agent leaves a conversation without saying goodbye.
/// Consumed by relationship and emotion systems to apply social penalties.
/// In a group conversation, one event fires per remaining counterparty.
#[derive(Message, Debug, Clone)]
pub struct ConversationAbandoned {
    pub abandoner: Entity,
    pub abandoned: Entity,
    pub conversation_state: ConversationState,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: u64) -> Entity {
        Entity::from_bits(id)
    }

    #[test]
    fn add_participant_respects_capacity() {
        let mut conv = Conversation::new(0, vec![e(1), e(2)], 0);
        for i in 3..=(MAX_GROUP_SIZE as u64) {
            assert!(conv.add_participant(e(i)));
        }
        assert!(!conv.add_participant(e(100)));
        assert_eq!(conv.participants.len(), MAX_GROUP_SIZE);
    }

    #[test]
    fn add_participant_rejects_duplicates() {
        let mut conv = Conversation::new(0, vec![e(1), e(2)], 0);
        assert!(!conv.add_participant(e(1)));
    }

    #[test]
    fn remove_participant_keeps_current_speaker_stable() {
        let mut conv = Conversation::new(0, vec![e(1), e(2), e(3)], 0);
        conv.set_speaker(e(3));
        assert_eq!(conv.current_speaker(), e(3));
        conv.remove_participant(e(1));
        assert_eq!(
            conv.current_speaker(),
            e(3),
            "removing a non-speaker should keep the speaker fixed"
        );
    }

    #[test]
    fn remove_participant_reclamps_when_speaker_leaves() {
        let mut conv = Conversation::new(0, vec![e(1), e(2), e(3)], 0);
        conv.set_speaker(e(3));
        conv.remove_participant(e(3));
        assert!(conv.turn < conv.participants.len());
    }

    #[test]
    fn listeners_excludes_current_speaker() {
        let mut conv = Conversation::new(0, vec![e(1), e(2), e(3)], 0);
        conv.set_speaker(e(2));
        let listeners: Vec<Entity> = conv.listeners().collect();
        assert_eq!(listeners.len(), 2);
        assert!(listeners.contains(&e(1)));
        assert!(listeners.contains(&e(3)));
        assert!(!listeners.contains(&e(2)));
    }
}
