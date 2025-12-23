use crate::agent::actions::ActionType;
use crate::agent::actions::registry::ActionState;
use crate::agent::mind::knowledge::{Concept, Triple};
use crate::agent::psyche::emotions::Emotion;
use crate::core::tick::TickCount;
use bevy::prelude::*;

/// An active conversation between two or more agents
#[derive(Debug, Clone, Reflect)]
pub struct Conversation {
    pub id: u64,
    pub participants: Vec<Entity>,
    pub turns: Vec<Turn>,
    pub state: ConversationState,
    pub started_at: u64,    // tick
    pub last_activity: u64, // tick - for stale detection
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Default)]
pub enum ConversationState {
    #[default]
    Greeting, // Just started
    Active,   // Mid-conversation
    Wrapping, // Saying goodbye
    Ended,    // Done
}

/// One "turn" in a conversation - what one agent says
#[derive(Debug, Clone, Reflect)]
pub struct Turn {
    pub speaker: Entity,
    pub intent: Intent,
    pub topic: Topic,
    pub emotion: Option<Emotion>, // Emotional coloring
    pub content: Vec<Triple>,     // Information being shared
    pub timestamp: u64,
    pub expects_response: bool, // Did this turn ask something?
}

/// What the speaker is trying to DO
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum Intent {
    Greet,       // "Hello!"
    Ask,         // Requesting information
    Answer,      // Responding to a question
    Share,       // Volunteering information
    Empathize,   // "That sounds hard"
    Agree,       // "You're right"
    Disagree,    // "I don't think so"
    Thank,       // "Thanks"
    Farewell,    // "Goodbye!"
    Acknowledge, // "Got it" / "I understand"
}

/// What they're talking about
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum Topic {
    General,           // Small talk
    Location(Concept), // "Where is X?"
    State(Entity),     // "How is X?" or "Is X ...?"
    Person(Entity),    // "What about Bob?"
    Help,              // "Can you help?"
}

impl Conversation {
    pub fn new(id: u64, participants: Vec<Entity>, started_at: u64) -> Self {
        Self {
            id,
            participants,
            turns: Vec::new(),
            state: ConversationState::Greeting,
            started_at,
            last_activity: started_at,
        }
    }

    pub fn add_turn(&mut self, turn: Turn) {
        self.last_activity = turn.timestamp;
        self.turns.push(turn);
    }
}

/// Resource to track all active conversations
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

        let conversation = Conversation::new(id, participants, tick);
        self.conversations.insert(id, conversation);

        id
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Conversation> {
        self.conversations.get_mut(&id)
    }

    /// Find active conversation involving these participants
    pub fn find_active(&self, participants: &[Entity]) -> Option<&Conversation> {
        self.conversations.values().find(|c| {
            c.state != ConversationState::Ended
                && participants.iter().all(|p| c.participants.contains(p))
        })
    }

    /// Get all active conversations (not Ended)
    pub fn active_conversations(&self) -> impl Iterator<Item = &Conversation> {
        self.conversations
            .values()
            .filter(|c| c.state != ConversationState::Ended)
    }
}

/// Component attached to agents currently in a conversation
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct InConversation {
    pub conversation_id: u64,
    pub partner: Entity,
    pub my_turn: bool,
    pub owes_response: bool, // Partner asked me something
}

// ============================================================================
// CONVERSATION MANAGEMENT SYSTEMS
// ============================================================================

/// System to sync InConversation components when Talk actions complete
pub fn sync_conversation_state(
    mut commands: Commands,
    mut conv_manager: ResMut<ConversationManager>,
    agents: Query<(Entity, &ActionState)>,
    mut in_conversation: Query<&mut InConversation>,
) {
    // Find agents that just completed a Talk action
    for (entity, action_state) in agents.iter() {
        // Check if this is a completed Talk action (action just switched to Idle)
        if action_state.action_type == ActionType::Idle {
            // Check if they have InConversation - if so, update it
            if let Ok(in_conv) = in_conversation.get(entity) {
                let conversation_id = in_conv.conversation_id;
                let partner = in_conv.partner;

                // Get the conversation to check its state
                if let Some(conv) = conv_manager.get_mut(conversation_id) {
                    // If conversation ended, remove InConversation from both participants
                    if conv.state == ConversationState::Ended {
                        for participant in &conv.participants {
                            commands.entity(*participant).remove::<InConversation>();
                        }
                    } else {
                        // Flip turns - if it was my turn, now it's partner's turn
                        let expects_response = conv
                            .turns
                            .last()
                            .map(|t| t.expects_response)
                            .unwrap_or(false);

                        // Update both participants
                        if let Ok(mut in_conv) = in_conversation.get_mut(entity) {
                            in_conv.my_turn = false;
                        }

                        if let Ok(mut partner_conv) = in_conversation.get_mut(partner) {
                            partner_conv.my_turn = true;
                            partner_conv.owes_response = expects_response;
                        }
                    }
                }
            }
        }

        // Check if this is a newly started Talk action with a target
        if action_state.action_type == ActionType::Talk {
            if let Some(target) = action_state.target_entity {
                // Check if we need to create InConversation components
                if in_conversation.get(entity).is_err() {
                    // Find or create conversation
                    let participants = vec![entity, target];
                    let conversation_id = if let Some(c) = conv_manager.find_active(&participants) {
                        c.id
                    } else {
                        // This will be created by the action's on_complete,
                        // but we need the component now
                        continue;
                    };

                    // Add InConversation components
                    commands.entity(entity).insert(InConversation {
                        conversation_id,
                        partner: target,
                        my_turn: true,
                        owes_response: false,
                    });

                    commands.entity(target).insert(InConversation {
                        conversation_id,
                        partner: entity,
                        my_turn: false,
                        owes_response: false,
                    });
                }
            }
        }
    }
}

/// System to clean up stale conversations
pub fn cleanup_stale_conversations(
    mut commands: Commands,
    mut conv_manager: ResMut<ConversationManager>,
    in_conversation: Query<(Entity, &InConversation)>,
    tick: Res<TickCount>,
) {
    const STALE_THRESHOLD: u64 = 300; // 5 seconds at 60 ticks/sec

    let mut conversations_to_end = Vec::new();

    // Find stale conversations
    for conv in conv_manager.active_conversations() {
        if tick.current.saturating_sub(conv.last_activity) > STALE_THRESHOLD {
            conversations_to_end.push(conv.id);
        }
    }

    // Mark them as ended and remove InConversation components
    for conv_id in conversations_to_end {
        if let Some(conv) = conv_manager.get_mut(conv_id) {
            conv.state = ConversationState::Ended;

            // Remove InConversation from all participants
            for (entity, in_conv) in in_conversation.iter() {
                if in_conv.conversation_id == conv_id {
                    commands.entity(entity).remove::<InConversation>();
                }
            }
        }
    }
}

/// Event for when someone abandons a conversation rudely
#[derive(Message, Debug, Clone)]
pub struct ConversationAbandoned {
    pub abandoner: Entity,
    pub abandoned: Entity,
    pub conversation_state: ConversationState,
}

/// System to detect rude conversation exits and apply relationship penalties
pub fn handle_conversation_exits(
    mut commands: Commands,
    mut conv_manager: ResMut<ConversationManager>,
    in_conversation: Query<(Entity, &InConversation)>,
    agents: Query<&ActionState>,
    mut abandoned_events: MessageWriter<ConversationAbandoned>,
) {
    // Check for agents who left a conversation without saying farewell
    for (entity, in_conv) in in_conversation.iter() {
        if let Ok(action_state) = agents.get(entity) {
            // If they're not doing Talk anymore and it's their turn
            if action_state.action_type != ActionType::Talk && in_conv.my_turn {
                // Get the conversation
                if let Some(conv) = conv_manager.get_mut(in_conv.conversation_id) {
                    // Check if the last turn was a farewell
                    let last_was_farewell = conv
                        .turns
                        .last()
                        .map(|t| t.intent == Intent::Farewell)
                        .unwrap_or(false);

                    if !last_was_farewell {
                        // They abandoned the conversation!
                        let state = conv.state;

                        // Emit event for relationship penalty system
                        abandoned_events.write(ConversationAbandoned {
                            abandoner: entity,
                            abandoned: in_conv.partner,
                            conversation_state: state,
                        });

                        // Mark conversation as ended
                        conv.state = ConversationState::Ended;

                        // Remove InConversation from both
                        commands.entity(entity).remove::<InConversation>();
                        commands.entity(in_conv.partner).remove::<InConversation>();
                    }
                }
            }
        }
    }
}
