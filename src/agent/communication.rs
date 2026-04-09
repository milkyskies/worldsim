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
use crate::agent::events::{ConversationTopic, GameEvent, SimEvent};
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

/// Maximum number of small-talk triples picked from the speaker's MindGraph
/// per `Share` turn. Keeps each turn focused rather than dumping the agent's
/// entire memory.
pub const SMALL_TALK_TRIPLES_PER_TURN: usize = 2;

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
                    process_initiate_conversation,
                    select_turn_intent.after(process_initiate_conversation),
                    process_received_communication.after(select_turn_intent),
                    emit_communication_events.after(process_received_communication),
                    evaluate_conversation_continuation.after(emit_communication_events),
                )
                    .run_if(not_paused),
            );
    }
}

// ============================================================================
// 1. InitiateConversation lifecycle (entry point owned by this plugin)
// ============================================================================

/// Watches agents with `InitiateConversation` running. Each tick:
///
/// 1. Re-syncs the agent's movement target to the partner's *current*
///    position so the agent tracks a moving partner instead of walking to a
///    stale fixed point.
/// 2. If the agent has reached `CONVERSATION_RANGE`, registers a new
///    `Conversation`, swaps `InitiateConversation` for `Converse` in both
///    agents' `ActiveActions` **in-place** (no command queue), inserts
///    `InConversation` on both, and emits `SimEvent::ConversationStarted`.
///
/// All `ActiveActions` mutations happen in-place via `Query<&mut>` rather
/// than queued commands. Otherwise [`evaluate_conversation_continuation`]
/// runs later in the same Update, sees no `Converse` marker yet (commands
/// haven't flushed), and abandons the just-created conversation.
pub fn process_initiate_conversation(
    mut commands: Commands,
    mut manager: ResMut<ConversationManager>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
    transforms: Query<&Transform, With<Agent>>,
    in_conversations: Query<&InConversation, With<Agent>>,
    mut active_actions: Query<(Entity, &mut ActiveActions), With<Agent>>,
    mut target_positions: Query<&mut crate::agent::TargetPosition>,
) {
    // Pass 1: snapshot which initiators want what partner. Doing this in a
    // separate pass releases the active_actions borrow before mutation.
    let pairs: Vec<(Entity, Option<Entity>)> = active_actions
        .iter()
        .filter_map(|(entity, active)| {
            active
                .get(ActionType::InitiateConversation)
                .map(|state| (entity, state.target_entity))
        })
        .collect();

    for (initiator, partner) in pairs {
        // No target → drop the action.
        let Some(partner) = partner else {
            if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
                active.remove(ActionType::InitiateConversation);
            }
            continue;
        };
        // Partner missing or already busy talking → drop the action.
        let Ok(partner_transform) = transforms.get(partner) else {
            if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
                active.remove(ActionType::InitiateConversation);
            }
            continue;
        };
        if in_conversations.get(partner).is_ok() {
            if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
                active.remove(ActionType::InitiateConversation);
            }
            continue;
        }
        let Ok(initiator_transform) = transforms.get(initiator) else {
            continue;
        };
        let initiator_pos = initiator_transform.translation.truncate();
        let partner_pos = partner_transform.translation.truncate();
        let distance = initiator_pos.distance(partner_pos);

        // Re-sync target_position so the movement system tracks the partner.
        if let Ok((_, mut active)) = active_actions.get_mut(initiator)
            && let Some(state) = active.get_mut(ActionType::InitiateConversation)
        {
            state.target_position = Some(partner_pos);
        }
        if let Ok(mut tp) = target_positions.get_mut(initiator) {
            tp.0 = Some(partner_pos);
        }

        if distance > CONVERSATION_RANGE {
            // Still walking — let the movement system advance position.
            continue;
        }

        // In range! Register the conversation if one doesn't exist yet.
        if manager.find_active(initiator, partner).is_some() {
            if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
                active.remove(ActionType::InitiateConversation);
            }
            continue;
        }

        let id = manager.start_conversation([initiator, partner], tick.current);
        commands.entity(initiator).insert(InConversation {
            conversation_id: id,
            partner,
        });
        commands.entity(partner).insert(InConversation {
            conversation_id: id,
            partner: initiator,
        });

        // Swap InitiateConversation -> Converse in-place on the initiator,
        // and add Converse to the partner. Both happen this tick so
        // evaluate_conversation_continuation sees the Mouth channel held.
        let now = tick.current;
        if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
            active.remove(ActionType::InitiateConversation);
            if !active.contains(ActionType::Converse) {
                active.insert(ActionState::new(ActionType::Converse, now));
            }
        }
        if let Ok((_, mut active)) = active_actions.get_mut(partner)
            && !active.contains(ActionType::Converse)
        {
            active.insert(ActionState::new(ActionType::Converse, now));
        }
        // Clear the movement target so we stop walking once paired.
        if let Ok(mut tp) = target_positions.get_mut(initiator) {
            tp.0 = None;
        }

        sim_events.write(SimEvent::ConversationStarted {
            participants: vec![initiator, partner],
            tick: tick.current,
            conversation_id: id,
        });
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
/// nuanced intents.
///
/// **Content selection** uses [`pick_small_talk_triples`] (#40) — for `Share`
/// intents the speaker offers up to `SMALL_TALK_TRIPLES_PER_TURN` triples
/// from their own MindGraph that score high on recency / salience / novelty
/// to the partner.
pub fn select_turn_intent(
    mut manager: ResMut<ConversationManager>,
    tick: Res<TickCount>,
    minds: Query<&MindGraph>,
) {
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
        let listener = conv.other_participant(speaker);
        let intent = next_intent_for(conv);
        let topic = Topic::General;

        // Only Share intents carry content; Greet/Farewell/Acknowledge are pure speech acts.
        let content = if matches!(intent, Intent::Share)
            && let (Ok(speaker_mind), Ok(listener_mind)) = (minds.get(speaker), minds.get(listener))
        {
            crate::agent::mind::small_talk::pick_small_talk_triples(
                speaker_mind,
                listener_mind,
                now,
                SMALL_TALK_TRIPLES_PER_TURN,
            )
        } else {
            Vec::new()
        };

        let turn = Turn {
            speaker,
            intent,
            topic,
            emotion: None,
            content,
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
    mut sim_events: MessageWriter<SimEvent>,
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
                sim_events.write(SimEvent::ConversationAbandoned {
                    abandoner,
                    abandoned,
                    tick: tick.current,
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
            sim_events.write(SimEvent::ConversationEnded {
                participants: vec![conv.participants[0], conv.participants[1]],
                tick: tick.current,
                conversation_id: id,
            });
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
        let conv = Conversation::new(0, [Entity::from_bits(1), Entity::from_bits(2)], 0);
        assert_eq!(next_intent_for(&conv), Intent::Greet);
    }

    #[test]
    fn next_intent_wraps_with_farewell() {
        let mut conv = Conversation::new(0, [Entity::from_bits(1), Entity::from_bits(2)], 0);
        conv.state = ConversationState::Wrapping;
        assert_eq!(next_intent_for(&conv), Intent::Farewell);
    }
}
