//! Communication: parallel Bevy plugin that runs conversations as a continuous channel.
//!
//! Reads: PsychologicalDrives, Transform, ActiveActions, MindGraph
//! Writes: ConversationManager, InConversation, ActiveActions (Converse marker), MindGraph (Hearsay), GameEvent
//! Upstream: agent::mind::conversation (data types), agent::actions (channel marker)
//! Downstream: psyche::relationships (consumes SocialInteraction), psyche::emotions (ConversationAbandoned)
//!
//! # Architecture
//!
//! Conversations live as a separate **channel** from the action system:
//!
//! ```text
//! ActionSystem                  CommunicationSystem
//! ────────────                  ───────────────────
//! Atomic, finite actions        Continuous, multi-turn channel
//! Single agent                  Two agents in shared state
//! Owns one action slot          Owns the Conversation, drives turn flow
//! ```
//!
//! Turn ownership lives **only** on [`Conversation::turn`] — never on a
//! component flag. The previous design's `InConversation::my_turn` was a
//! recurring source of races; the single-owner model removes them entirely.
//!
//! Body-channel occupation works by inserting a [`ConverseAction`] marker into
//! [`ActiveActions`] for each participant. This makes Sleep / Flee / Fight
//! preempt conversations through the same path as any other action conflict —
//! the [`evaluate_conversation_continuation`] system notices the marker is
//! gone on the next tick and ends the conversation with an
//! [`ConversationAbandoned`] event.

use bevy::ecs::world::EntityWorldMut;
use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::actions::registry::{ActionState, ActiveActions};
use crate::agent::actions::types::ActionType;
use crate::agent::body::needs::PsychologicalDrives;
use crate::agent::events::{ConversationTopic, GameEvent};
use crate::agent::mind::conversation::{
    Conversation, ConversationAbandoned, ConversationManager, ConversationState, InConversation,
    Intent, Topic, Turn,
};
use crate::agent::mind::knowledge::{Metadata, MindGraph};
use crate::agent::mind::social_perception::CONVERSATION_RANGE;
use crate::core::not_paused;
use crate::core::tick::TickCount;

// ============================================================================
// Tunables
// ============================================================================

/// Both agents must have at least this much social drive for the
/// CommunicationPlugin to auto-start a conversation between them.
///
/// This is **interim** behavior — when #45 (`InitiateConversation` action)
/// lands, that action will be the canonical entry point and this auto-init
/// path will be removed.
pub const AUTO_INITIATE_SOCIAL_THRESHOLD: f32 = 0.55;

/// Number of ticks since the last turn after which a conversation is
/// considered abandoned and ended.
pub const STALE_CONVERSATION_TICKS: u64 = 300;

/// Ticks between two turns from the same conversation. Keeps the dialogue
/// readable rather than every tick producing a new turn.
pub const TURN_INTERVAL_TICKS: u64 = 30;

/// Conversations end gracefully after this many turns when neither side
/// has a competing urgency.
pub const NATURAL_END_TURN_COUNT: usize = 6;

/// Reduction in `social` drive each successful turn satisfies.
pub const SOCIAL_DRIVE_PER_TURN: f32 = 0.1;

// ============================================================================
// Plugin
// ============================================================================

/// Bevy plugin that owns the conversation lifecycle. Registered alongside
/// (not inside) the action plugin so the two systems run in parallel.
pub struct CommunicationPlugin;

impl Plugin for CommunicationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConversationManager>()
            .register_type::<InConversation>()
            .add_message::<ConversationAbandoned>()
            .add_systems(
                Update,
                (
                    auto_initiate_conversations,
                    select_turn_intent.after(auto_initiate_conversations),
                    process_received_communication.after(select_turn_intent),
                    emit_communication_events.after(process_received_communication),
                    evaluate_conversation_continuation.after(emit_communication_events),
                )
                    .run_if(not_paused),
            );
    }
}

// ============================================================================
// 1. Auto-initiation (interim entry point — replaced by #45)
// ============================================================================

/// Pair up nearby agents who both want to socialize and start a conversation.
///
/// **Interim**: this is the temporary entry point until issue #45 introduces
/// the `InitiateConversation` action. The auto-init policy is intentionally
/// conservative (high social-drive threshold, in-range only, neither already
/// in a conversation) so it doesn't fire spuriously in unrelated tests.
pub fn auto_initiate_conversations(
    mut commands: Commands,
    mut manager: ResMut<ConversationManager>,
    tick: Res<TickCount>,
    agents: Query<
        (
            Entity,
            &Transform,
            &PsychologicalDrives,
            Option<&InConversation>,
            &ActiveActions,
        ),
        With<Agent>,
    >,
) {
    let mut candidates: Vec<(Entity, Vec2, f32)> = Vec::new();
    for (entity, transform, drives, in_conv, active) in agents.iter() {
        if in_conv.is_some() {
            continue;
        }
        if drives.social < AUTO_INITIATE_SOCIAL_THRESHOLD {
            continue;
        }
        // Don't yank agents out of high-priority body work just to chat.
        if active.contains(ActionType::Sleep)
            || active.contains(ActionType::Flee)
            || active.contains(ActionType::Attack)
        {
            continue;
        }
        candidates.push((entity, transform.translation.truncate(), drives.social));
    }

    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            let (a, pa, _) = candidates[i];
            let (b, pb, _) = candidates[j];
            if pa.distance(pb) > CONVERSATION_RANGE {
                continue;
            }
            // Check neither was paired up earlier in the same tick.
            if manager.find_active(a, b).is_some() {
                continue;
            }
            let id = manager.start_conversation([a, b], tick.current);
            commands.entity(a).insert(InConversation {
                conversation_id: id,
                partner: b,
            });
            commands.entity(b).insert(InConversation {
                conversation_id: id,
                partner: a,
            });
            commands
                .entity(a)
                .queue(InsertConverseMarker { tick: tick.current });
            commands
                .entity(b)
                .queue(InsertConverseMarker { tick: tick.current });
        }
    }
}

/// `EntityCommand` that adds a [`ConverseAction`] marker to an entity's
/// [`ActiveActions`]. Inserted via `commands.queue` so the change happens at
/// the next sync point — the borrow checker can't see two systems mutating
/// `ActiveActions` if we go through commands.
struct InsertConverseMarker {
    tick: u64,
}

impl EntityCommand for InsertConverseMarker {
    fn apply(self, mut entity: EntityWorldMut) {
        let tick = self.tick;
        if let Some(mut active) = entity.get_mut::<ActiveActions>() {
            if !active.contains(ActionType::Converse) {
                active.insert(ActionState::new(ActionType::Converse, tick));
            }
        }
    }
}

struct RemoveConverseMarker;

impl EntityCommand for RemoveConverseMarker {
    fn apply(self, mut entity: EntityWorldMut) {
        if let Some(mut active) = entity.get_mut::<ActiveActions>() {
            active.remove(ActionType::Converse);
        }
    }
}

// ============================================================================
// 2. Select turn intent (basic version — #46 replaces with smart selection)
// ============================================================================

/// For each active conversation whose turn cadence is up, append a new turn
/// from the current speaker. Intent selection is intentionally simple in this
/// PR — issue #46 will read agent state, goals, and relationship to pick
/// nuanced intents and content.
pub fn select_turn_intent(mut manager: ResMut<ConversationManager>, tick: Res<TickCount>) {
    let now = tick.current;
    for conv in manager.conversations.values_mut() {
        if conv.state == ConversationState::Ended {
            continue;
        }
        // Greeting turn fires immediately on tick 0; subsequent turns wait.
        if !conv.turns.is_empty() && now.saturating_sub(conv.last_turn_at) < TURN_INTERVAL_TICKS {
            continue;
        }

        let speaker = conv.current_speaker();
        let intent = next_intent_for(conv);
        let topic = Topic::General;

        let turn = Turn {
            speaker,
            intent,
            topic,
            emotion: None,
            content: Vec::new(),
            timestamp: now,
            expects_response: matches!(intent, Intent::Greet | Intent::Ask),
        };
        conv.add_turn(turn);
        conv.advance_turn();

        // State machine: Greeting -> Active -> Wrapping -> Ended.
        conv.state = match (conv.state, intent) {
            (_, Intent::Farewell) => ConversationState::Ended,
            (ConversationState::Greeting, _) => {
                if conv.turns.len() >= 2 {
                    ConversationState::Active
                } else {
                    ConversationState::Greeting
                }
            }
            (ConversationState::Active, _) if conv.turns.len() >= NATURAL_END_TURN_COUNT => {
                ConversationState::Wrapping
            }
            (state, _) => state,
        };
    }
}

/// Pick the next intent for the current speaker.
///
/// Trivial state machine for now: greet, then share, then wrap with farewell.
/// Issue #46 replaces this with goal-aware selection.
fn next_intent_for(conv: &Conversation) -> Intent {
    match conv.state {
        ConversationState::Greeting if conv.turns.is_empty() => Intent::Greet,
        ConversationState::Wrapping => Intent::Farewell,
        ConversationState::Ended => Intent::Farewell,
        _ => {
            if conv.last_turn_expects_response() {
                Intent::Acknowledge
            } else {
                Intent::Share
            }
        }
    }
}

// ============================================================================
// 3. Process received communication (write hearsay into listener's mind)
// ============================================================================

/// Apply the most recent turn's `content` triples to the listener's MindGraph
/// as Hearsay knowledge. Listeners only learn from turns that have *not yet*
/// been processed — we use the participant's turn index to detect new turns.
pub fn process_received_communication(
    manager: Res<ConversationManager>,
    mut minds: Query<&mut MindGraph>,
    tick: Res<TickCount>,
) {
    for conv in manager.conversations.values() {
        let Some(turn) = conv.turns.last() else {
            continue;
        };
        if turn.timestamp != tick.current {
            // Only just-emitted turns get processed — anything older was
            // handled in a prior tick.
            continue;
        }
        if turn.content.is_empty() {
            continue;
        }
        let listener = conv.other_participant(turn.speaker);
        let Ok(mut mind) = minds.get_mut(listener) else {
            continue;
        };
        for triple in &turn.content {
            let mut hearsay = triple.clone();
            hearsay.meta = Metadata::hearsay(tick.current, turn.speaker);
            mind.assert(hearsay);
        }
    }
}

// ============================================================================
// 4. Emit communication events (downstream feeds: relationships, emotions)
// ============================================================================

/// Re-emit the conversation's freshest turn as a [`GameEvent::SocialInteraction`]
/// (and optionally [`GameEvent::KnowledgeShared`]) so existing downstream
/// systems (relationship updater, memory consolidator) keep working without
/// changes.
pub fn emit_communication_events(
    manager: Res<ConversationManager>,
    tick: Res<TickCount>,
    mut events: MessageWriter<GameEvent>,
) {
    for conv in manager.conversations.values() {
        let Some(turn) = conv.turns.last() else {
            continue;
        };
        if turn.timestamp != tick.current {
            continue;
        }
        let listener = conv.other_participant(turn.speaker);
        events.write(GameEvent::SocialInteraction {
            actor: turn.speaker,
            target: listener,
            action: ActionType::Converse,
            topic: Some(map_topic(turn.topic)),
            valence: 0.5,
        });
        if !turn.content.is_empty() {
            events.write(GameEvent::KnowledgeShared {
                speaker: turn.speaker,
                listener,
                content: turn.content.clone(),
            });
        }
    }
}

fn map_topic(topic: Topic) -> ConversationTopic {
    match topic {
        Topic::General => ConversationTopic::Greetings,
        Topic::Location(_) => ConversationTopic::Request,
        Topic::State(_) => ConversationTopic::Knowledge,
        Topic::Person(_) => ConversationTopic::Gossip,
        Topic::Help => ConversationTopic::Request,
    }
}

// ============================================================================
// 5. Continuation / cleanup
// ============================================================================

/// End conversations whose participants moved out of range, lost the Mouth
/// channel (e.g. Sleep / Flee preempted Converse), reached natural end, or
/// went stale. Emits [`ConversationAbandoned`] for ungraceful exits.
pub fn evaluate_conversation_continuation(
    mut commands: Commands,
    mut manager: ResMut<ConversationManager>,
    mut events: MessageWriter<ConversationAbandoned>,
    tick: Res<TickCount>,
    transforms: Query<&Transform>,
    actives: Query<&ActiveActions>,
) {
    let mut to_finalize: Vec<u64> = Vec::new();

    for (id, conv) in manager.conversations.iter_mut() {
        if conv.state == ConversationState::Ended {
            // Already ended last tick — schedule cleanup.
            to_finalize.push(*id);
            continue;
        }

        let [a, b] = conv.participants;

        // Distance check.
        let in_range = match (transforms.get(a), transforms.get(b)) {
            (Ok(ta), Ok(tb)) => {
                ta.translation
                    .truncate()
                    .distance(tb.translation.truncate())
                    <= CONVERSATION_RANGE
            }
            _ => false,
        };

        // Channel check — Converse marker preempted by Sleep / Flee / Fight
        // means the body got pulled into something more urgent.
        let channels_held = matches!(
            (actives.get(a), actives.get(b)),
            (Ok(aa), Ok(ab))
                if aa.contains(ActionType::Converse) && ab.contains(ActionType::Converse)
        );

        let stale = tick.current.saturating_sub(conv.last_turn_at) > STALE_CONVERSATION_TICKS;

        if !in_range || !channels_held || stale {
            // Hard interrupt — not graceful unless the last turn was a Farewell.
            let graceful = conv.turns.last().map(|t| t.intent) == Some(Intent::Farewell);
            if !graceful {
                let abandoner = if !channels_held {
                    // Whichever participant lost the Converse marker first
                    // is the one whose body went elsewhere.
                    pick_abandoner(&actives, [a, b]).unwrap_or(conv.current_speaker())
                } else {
                    conv.current_speaker()
                };
                let abandoned = if abandoner == a { b } else { a };
                events.write(ConversationAbandoned {
                    abandoner,
                    abandoned,
                    conversation_state: conv.state,
                });
            }
            conv.state = ConversationState::Ended;
            to_finalize.push(*id);
        }
    }

    // Finalize: drop InConversation, remove the Converse marker, drop the
    // entry from the manager so it doesn't grow unbounded.
    for id in to_finalize {
        if let Some(conv) = manager.conversations.get(&id) {
            for entity in conv.participants {
                commands.entity(entity).remove::<InConversation>();
                commands.entity(entity).queue(RemoveConverseMarker);
            }
        }
        manager.conversations.remove(&id);
    }
}

fn pick_abandoner(actives: &Query<&ActiveActions>, participants: [Entity; 2]) -> Option<Entity> {
    for entity in participants {
        if let Ok(active) = actives.get(entity) {
            if !active.contains(ActionType::Converse) {
                return Some(entity);
            }
        } else {
            return Some(entity);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_topic_general_is_greetings() {
        assert_eq!(map_topic(Topic::General), ConversationTopic::Greetings);
    }

    #[test]
    fn map_topic_help_is_request() {
        assert_eq!(map_topic(Topic::Help), ConversationTopic::Request);
    }

    #[test]
    fn next_intent_starts_with_greet() {
        let conv = Conversation::new(0, [Entity::from_raw(1), Entity::from_raw(2)], 0);
        assert_eq!(next_intent_for(&conv), Intent::Greet);
    }

    #[test]
    fn next_intent_wraps_with_farewell() {
        let mut conv = Conversation::new(0, [Entity::from_raw(1), Entity::from_raw(2)], 0);
        conv.state = ConversationState::Wrapping;
        assert_eq!(next_intent_for(&conv), Intent::Farewell);
    }
}
