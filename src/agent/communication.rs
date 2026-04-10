//! Communication: parallel Bevy plugin that runs conversations as a continuous channel.
//!
//! Reads: PsychologicalDrives, Transform, ActiveActions, MindGraph, TheoryOfMind, EmotionalState, Personality, RationalBrain
//! Writes: ConversationManager, InConversation, ActiveActions (Converse marker), MindGraph (Hearsay), TheoryOfMind, GameEvent, SimEvent
//! Upstream: agent::mind::conversation (data types), agent::actions (channel marker), agent::mind::theory_of_mind
//! Downstream: psyche::relationships (consumes SocialInteraction)
//!
//! # Architecture
//!
//! Conversations live as a separate **channel** from the action system and
//! support two or more participants (group conversations — the campfire
//! scenario from #65):
//!
//! ```text
//! ActionSystem                  CommunicationSystem
//! ────────────                  ───────────────────
//! Atomic, finite actions        Continuous, multi-turn channel
//! Single agent                  2..=MAX_GROUP_SIZE agents in shared state
//! Owns one action slot          Owns the Conversation, drives turn flow
//! ```
//!
//! Turn ownership lives **only** on [`Conversation::turn`] — never on a
//! component flag. The previous design's `InConversation::my_turn` was a
//! recurring source of races; the single-owner model removes them entirely.
//!
//! In group conversations the speaker broadcasts each turn's shared
//! knowledge to every listener simultaneously, and the next speaker is
//! picked by a personality-weighted selector ([`pick_next_speaker`]) that
//! favors extraverts and listeners who've queued a response. See
//! [`speak_desire`] for the scoring formula.
//!
//! Body-channel occupation works by inserting a [`ConverseAction`] marker into
//! [`ActiveActions`] for each participant. Per-participant range + channel
//! checks let an individual agent leave the group without collapsing the
//! conversation — Sleep / Flee / Fight on one participant just removes
//! them, and the rest keep talking until fewer than two remain.

use bevy::ecs::world::EntityWorldMut;
use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::actions::registry::{ActionState, ActiveActions};
use crate::agent::actions::types::ActionType;
use crate::agent::brains::rational::RationalBrain;
use crate::agent::brains::thinking::Goal;
use crate::agent::events::{ConversationTopic, GameEvent, SimEvent};
use crate::agent::mind::conversation::{
    Conversation, ConversationAbandoned, ConversationManager, ConversationState, InConversation,
    Intent, Topic, Turn,
};
use crate::agent::mind::knowledge::{Concept, Metadata, MindGraph, Node, Predicate, Value};
use crate::agent::mind::social_perception::CONVERSATION_RANGE;
use crate::agent::mind::theory_of_mind::{self, TheoryOfMind};
use crate::agent::psyche::emotions::EmotionalState;
use crate::agent::psyche::personality::Personality;
use crate::core::not_paused;
use crate::core::tick::TickCount;

// ============================================================================
// Tunables
// ============================================================================

/// Number of ticks since the last turn after which a conversation is
/// considered abandoned and ended.
pub const STALE_CONVERSATION_TICKS: u64 = 300;

/// Ticks between casual chitchat turns (Share / Acknowledge). Relaxed cadence.
pub const CHITCHAT_INTERVAL_TICKS: u64 = 60;

/// Ticks between urgent turns (Ask / Answer). Faster cadence for information exchange.
pub const URGENT_INTERVAL_TICKS: u64 = 30;

/// Ticks before a Farewell fires. Short — the conversation is already wrapping.
pub const FAREWELL_INTERVAL_TICKS: u64 = 20;

/// Conversations end gracefully after this many turns when neither side
/// has a competing urgency.
pub const NATURAL_END_TURN_COUNT: usize = 6;

/// Reduction in `social` drive each successful turn satisfies.
pub const SOCIAL_DRIVE_PER_TURN: f32 = 0.1;

/// Maximum number of triples shared per `Share` turn.
pub const SMALL_TALK_TRIPLES_PER_TURN: usize = 2;

/// Base salience threshold for danger warnings. Neurotic agents warn at lower salience.
pub const DANGER_WARN_SALIENCE: f32 = 0.7;

/// Danger observations older than this many ticks are not worth warning about.
pub const DANGER_RECENCY_TICKS: u64 = 600;

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
                    // process_initiate_conversation must run after start_actions so that
                    // InitiateConversation is already in ActiveActions before proximity is
                    // checked. Without this ordering, start_actions may not have inserted
                    // the action yet, causing the conversation to never register.
                    process_initiate_conversation
                        .after(crate::agent::nervous_system::execution::start_actions),
                    select_turn_intent.after(process_initiate_conversation),
                    update_speaker_theory_of_mind.after(select_turn_intent),
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
/// 2. If the agent has reached `CONVERSATION_RANGE`, either:
///    - **Starts a new conversation** with the partner (if the partner is
///      not already talking), or
///    - **Joins the partner's existing conversation** if one is active and
///      has capacity (supports the campfire scenario — three agents chat
///      as a group instead of two disjoint pair chats).
/// 3. Swaps `InitiateConversation` for `Converse` in `ActiveActions`
///    **in-place** (no command queue), inserts `InConversation`, and emits
///    `SimEvent::ConversationStarted` (or `ConversationJoined` for a join).
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
        // Initiator already in a conversation — drop the stale action.
        // Check both the component query (for prior-tick state) *and* the
        // manager (for same-tick state written earlier in this very system
        // loop — commands to insert `InConversation` haven't flushed yet,
        // so the query would miss a conversation we just created). Without
        // this dedupe two symmetric initiators would each spawn a
        // duplicate conversation the same tick.
        if in_conversations.get(initiator).is_ok() || manager.find_active_for(initiator).is_some() {
            if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
                active.remove(ActionType::InitiateConversation);
            }
            continue;
        }
        // No target → drop the action.
        let Some(partner) = partner else {
            if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
                active.remove(ActionType::InitiateConversation);
            }
            continue;
        };
        let Ok(partner_transform) = transforms.get(partner) else {
            if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
                active.remove(ActionType::InitiateConversation);
            }
            continue;
        };
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

        // In range. Two cases: (1) partner is already in a conversation →
        // join it as a third+ participant, (2) partner is free → start a
        // new 2-agent conversation. Consult both the query (prior-tick
        // state) and the manager (same-tick state just written above).
        let now = tick.current;
        let join_target: Option<u64> = in_conversations
            .get(partner)
            .ok()
            .map(|ic| ic.conversation_id)
            .or_else(|| manager.find_active_for(partner).map(|c| c.id));

        let (conversation_id, is_new) = if let Some(existing_id) = join_target {
            let Some(conv) = manager.conversations.get_mut(&existing_id) else {
                if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
                    active.remove(ActionType::InitiateConversation);
                }
                continue;
            };
            if conv.state == ConversationState::Ended {
                if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
                    active.remove(ActionType::InitiateConversation);
                }
                continue;
            }
            if !conv.add_participant(initiator) {
                // Group full or already in — drop the action either way.
                if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
                    active.remove(ActionType::InitiateConversation);
                }
                continue;
            }
            (existing_id, false)
        } else {
            let id = manager.start_conversation(vec![initiator, partner], now);
            (id, true)
        };

        commands
            .entity(initiator)
            .insert(InConversation { conversation_id });
        if is_new {
            commands
                .entity(partner)
                .insert(InConversation { conversation_id });
        }

        // Swap InitiateConversation -> Converse in-place on the initiator.
        // For new 2-agent conversations, also add Converse to the partner.
        if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
            active.remove(ActionType::InitiateConversation);
            if !active.contains(ActionType::Converse) {
                active.insert(ActionState::new(ActionType::Converse, now));
            }
        }
        if is_new
            && let Ok((_, mut active)) = active_actions.get_mut(partner)
            && !active.contains(ActionType::Converse)
        {
            active.insert(ActionState::new(ActionType::Converse, now));
        }
        // Clear the movement target so we stop walking once paired.
        if let Ok(mut tp) = target_positions.get_mut(initiator) {
            tp.0 = None;
        }

        if is_new {
            sim_events.write(SimEvent::ConversationStarted {
                participants: vec![initiator, partner],
                tick: now,
                conversation_id,
            });
        } else {
            sim_events.write(SimEvent::ConversationJoined {
                joiner: initiator,
                tick: now,
                conversation_id,
            });
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
// 2. Select turn intent
// ============================================================================

/// For each active conversation whose turn cadence is up, append a new turn
/// from the current speaker and advance the floor to the next speaker.
///
/// In group conversations the speaker talks to *all* listeners at once —
/// content selection picks a primary listener (the one who most recently
/// asked a question, or the first listener otherwise) for novelty scoring,
/// but the resulting triples are broadcast to every listener downstream.
///
/// After the turn is recorded, the next speaker is picked via
/// [`pick_next_speaker`] using personality traits (extraverts dominate,
/// agreeable agents yield) and the `wants_to_speak` queue.
pub fn select_turn_intent(
    mut manager: ResMut<ConversationManager>,
    tick: Res<TickCount>,
    minds: Query<&MindGraph>,
    toms: Query<&TheoryOfMind>,
    rational_brains: Query<&RationalBrain>,
    personalities: Query<&Personality>,
) {
    let now = tick.current;
    for conv in manager.conversations.values_mut() {
        if conv.state == ConversationState::Ended {
            continue;
        }
        if conv.participants.len() < 2 {
            continue;
        }

        let speaker = conv.current_speaker();
        // Primary listener for content-novelty scoring: whoever just asked a
        // question (if any), else the first non-speaker participant.
        let primary_listener = conv
            .turns
            .last()
            .filter(|t| t.expects_response && t.speaker != speaker)
            .map(|t| t.speaker)
            .or_else(|| conv.listeners().next());
        let Some(primary_listener) = primary_listener else {
            continue;
        };

        let Ok(speaker_mind) = minds.get(speaker) else {
            continue;
        };
        let speaker_tom = toms.get(speaker).ok();
        let goal = rational_brains
            .get(speaker)
            .ok()
            .and_then(|b| b.current_goal.as_ref());
        let personality = personalities.get(speaker).ok();

        let has_deliberate = !crate::agent::mind::deliberate_talk::pick_deliberate_content(
            speaker_mind,
            goal,
            speaker_tom,
            primary_listener,
            now,
            1,
        )
        .0
        .is_empty();
        let has_casual = !crate::agent::mind::small_talk::pick_small_talk_triples(
            speaker_mind,
            speaker_tom,
            primary_listener,
            now,
            1,
        )
        .is_empty();

        let intent = select_intent(
            conv,
            speaker_mind,
            speaker_tom,
            primary_listener,
            goal,
            personality,
            now,
            has_deliberate,
            has_casual,
        );

        let min_interval = intent_interval(intent);
        if min_interval > 0
            && !conv.turns.is_empty()
            && now.saturating_sub(conv.last_turn_at) < min_interval
        {
            continue;
        }

        let (content, topic) = if matches!(intent, Intent::Share | Intent::Answer) {
            let deliberate = crate::agent::mind::deliberate_talk::pick_deliberate_content(
                speaker_mind,
                goal,
                speaker_tom,
                primary_listener,
                now,
                SMALL_TALK_TRIPLES_PER_TURN,
            );
            if !deliberate.0.is_empty() {
                deliberate
            } else {
                let casual = crate::agent::mind::small_talk::pick_small_talk_triples(
                    speaker_mind,
                    speaker_tom,
                    primary_listener,
                    now,
                    SMALL_TALK_TRIPLES_PER_TURN,
                );
                (casual, Topic::General)
            }
        } else {
            (Vec::new(), Topic::General)
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
        // Speaker just spoke — clear their "wants to speak" flag.
        conv.wants_to_speak.remove(&speaker);

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

        // Advance the floor: personality-weighted pick among non-speakers.
        let next = pick_next_speaker(conv, &personalities);
        conv.set_speaker(next);
    }
}

/// Score a participant's desire to take the next turn based on personality
/// and the "queued response" flag. Higher score → more likely to speak.
///
/// ```text
/// score = 1.0
///       + extraversion * 2.0       // extraverts seize the floor
///       - agreeableness * 0.6      // agreeable yields
///       + 2.5 if wants_to_speak    // just had a question asked / emotional spike
/// ```
///
/// Returns `1.0` for participants with no `Personality` component so tests
/// using bare entities still work.
pub(crate) fn speak_desire(personality: Option<&Personality>, wants_to_speak: bool) -> f32 {
    let extraversion = personality.map(|p| p.traits.extraversion).unwrap_or(0.5);
    let agreeableness = personality.map(|p| p.traits.agreeableness).unwrap_or(0.5);
    let base = 1.0 + extraversion * 2.0 - agreeableness * 0.6;
    if wants_to_speak { base + 2.5 } else { base }
}

/// Pick the next speaker from among the non-speaker participants using a
/// deterministic weighted pseudo-random selection seeded by the conversation
/// id and turn count.
///
/// The current speaker is excluded from the pool (rotation property). If the
/// only non-speaker is also missing a `Personality` component, they speak by
/// default.
pub(crate) fn pick_next_speaker(
    conv: &Conversation,
    personalities: &Query<&Personality>,
) -> Entity {
    let speaker = conv.current_speaker();
    let candidates: Vec<Entity> = conv
        .participants
        .iter()
        .copied()
        .filter(|e| *e != speaker)
        .collect();
    if candidates.is_empty() {
        return speaker;
    }
    if candidates.len() == 1 {
        return candidates[0];
    }

    let scores: Vec<f32> = candidates
        .iter()
        .map(|e| {
            let p = personalities.get(*e).ok();
            let wants = conv.wants_to_speak.contains(e);
            speak_desire(p, wants).max(0.01)
        })
        .collect();
    let total: f32 = scores.iter().sum();
    // Deterministic selector: hash conv id + turn count into [0, total).
    let seed = conv.id.wrapping_mul(2_654_435_761) ^ (conv.turns.len() as u64);
    let frac = ((seed % 1_000_000) as f32) / 1_000_000.0;
    let mut target = frac * total;
    for (i, s) in scores.iter().enumerate() {
        target -= *s;
        if target <= 0.0 {
            return candidates[i];
        }
    }
    *candidates.last().unwrap()
}

/// Select the intent for the speaker's next turn based on their knowledge,
/// active goal, and personality traits.
///
/// `has_deliberate` and `has_casual` are pre-computed by the caller to avoid
/// redundant triple scans — content availability is checked once outside and
/// the result passed in.
///
/// Priority order:
/// 1. **Greet** — first turn in a new conversation
/// 2. **Farewell** — conversation is wrapping up
/// 3. **Share (Warn)** — recent high-salience danger the listener doesn't know.
///    Neuroticism lowers the salience threshold (anxious agents warn more readily).
/// 4. **Ask** — active goal needs location information the listener might have
/// 5. **Answer** — listener asked something last turn
/// 6. **Share** — has salient, novel world knowledge to pass on
/// 7. **Share (ChitChat)** — extraverted agents keep talking via small talk
/// 8. **Acknowledge** — default; nothing compelling to say
pub(crate) fn select_intent(
    conv: &Conversation,
    speaker_mind: &MindGraph,
    speaker_tom: Option<&TheoryOfMind>,
    listener: Entity,
    goal: Option<&Goal>,
    personality: Option<&Personality>,
    now: u64,
    has_deliberate: bool,
    has_casual: bool,
) -> Intent {
    let neuroticism = personality.map(|p| p.traits.neuroticism).unwrap_or(0.5);
    let extraversion = personality.map(|p| p.traits.extraversion).unwrap_or(0.5);

    // 1. Greet on first turn.
    if conv.state == ConversationState::Greeting && conv.turns.is_empty() {
        return Intent::Greet;
    }

    // 2. Farewell when wrapping.
    if matches!(
        conv.state,
        ConversationState::Wrapping | ConversationState::Ended
    ) {
        return Intent::Farewell;
    }

    // 3. Warn: recent high-salience danger the listener doesn't know yet.
    //    Neuroticism lowers the threshold — anxious agents warn more readily.
    let warn_threshold = (DANGER_WARN_SALIENCE - (neuroticism - 0.5) * 0.2).clamp(0.3, 1.0);
    if has_danger_to_warn(speaker_mind, speaker_tom, listener, warn_threshold, now) {
        return Intent::Share;
    }

    // 4. Ask: active goal that requires finding something (location/containment).
    if let Some(g) = goal
        && goal_needs_location(g)
    {
        return Intent::Ask;
    }

    // 5. Answer: listener asked something last turn.
    if conv.last_turn_expects_response() {
        return Intent::Answer;
    }

    // 6. Share: has deliberate world knowledge scoring above the minimum threshold.
    if has_deliberate {
        return Intent::Share;
    }

    // 7. ChitChat: extraverted agents share casual observations even when nothing
    //    urgent is on their mind.
    if extraversion > 0.55 && has_casual {
        return Intent::Share;
    }

    // 8. Default: pure social acknowledgement.
    Intent::Acknowledge
}

/// Returns the minimum ticks to wait before the next turn of this intent type.
///
/// Greet fires immediately (no wait); urgent intents (Ask/Answer) use the
/// shorter [`URGENT_INTERVAL_TICKS`]; casual sharing uses [`CHITCHAT_INTERVAL_TICKS`].
fn intent_interval(intent: Intent) -> u64 {
    match intent {
        Intent::Greet => 0,
        Intent::Farewell => FAREWELL_INTERVAL_TICKS,
        Intent::Ask | Intent::Answer => URGENT_INTERVAL_TICKS,
        _ => CHITCHAT_INTERVAL_TICKS,
    }
}

/// True if the speaker has recent high-salience danger knowledge the listener
/// likely doesn't know. Uses the speaker's [`TheoryOfMind`] to estimate what
/// the listener already knows, falling back to "assume they don't know" for
/// strangers. Uses the [`DANGER_RECENCY_TICKS`] window to exclude stale
/// observations.
fn has_danger_to_warn(
    speaker_mind: &MindGraph,
    speaker_tom: Option<&TheoryOfMind>,
    listener: Entity,
    salience_threshold: f32,
    now: u64,
) -> bool {
    speaker_mind.iter().any(|t| {
        t.predicate == Predicate::HasTrait
            && t.object == Value::Concept(Concept::Dangerous)
            && t.meta.salience >= salience_threshold
            && now.saturating_sub(t.meta.timestamp) <= DANGER_RECENCY_TICKS
            && speaker_tom
                .map(|tom| !tom.believes_target_knows_danger(listener, &t.subject, 0.5))
                .unwrap_or(true) // No ToM model = assume they don't know
    })
}

/// True if the goal has at least one condition that requires location information
/// (i.e. the agent needs to find *where* something is).
fn goal_needs_location(goal: &Goal) -> bool {
    goal.conditions.iter().any(|cond| {
        cond.predicate
            .map(|p| matches!(p, Predicate::LocatedAt | Predicate::Contains))
            .unwrap_or(false)
    })
}

// ============================================================================
// 2b. Update speaker's theory of mind after sharing content
// ============================================================================

/// When a speaker shares triples, update the speaker's [`TheoryOfMind`] to
/// record that every listener in the group now probably knows those facts.
///
/// This runs after [`select_turn_intent`] so the latest turn's content is
/// available. Only processes turns emitted this tick.
pub fn update_speaker_theory_of_mind(
    manager: Res<ConversationManager>,
    tick: Res<TickCount>,
    mut toms: Query<&mut TheoryOfMind>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    for conv in manager.conversations.values() {
        let Some(turn) = conv.turns.last() else {
            continue;
        };
        if turn.timestamp != tick.current || turn.content.is_empty() {
            continue;
        }
        let listeners: Vec<Entity> = conv
            .participants
            .iter()
            .copied()
            .filter(|e| *e != turn.speaker)
            .collect();
        let Ok(mut speaker_tom) = toms.get_mut(turn.speaker) else {
            continue;
        };
        let count = turn.content.len();
        for listener in listeners {
            speaker_tom.record_shared_triples(
                listener,
                &turn.content,
                theory_of_mind::COMMUNICATED_BELIEF_CONFIDENCE,
                tick.current,
            );
            sim_events.write(SimEvent::TheoryOfMindUpdated {
                agent: turn.speaker,
                about: listener,
                tick: tick.current,
                source: crate::agent::events::TheoryOfMindSource::Communicated,
                belief_count: count,
            });
        }
    }
}

// ============================================================================
// 3. Process received communication (write hearsay into listener's mind)
// ============================================================================

/// Apply the most recent turn's `content` triples to every listener's
/// MindGraph as Hearsay knowledge. In group conversations this broadcasts
/// shared knowledge to all participants simultaneously — the "the whole
/// group hears it" property required by #65.
pub fn process_received_communication(
    manager: Res<ConversationManager>,
    mut minds: Query<&mut MindGraph>,
    mut toms: Query<&mut TheoryOfMind>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    for conv in manager.conversations.values() {
        let Some(turn) = conv.turns.last() else {
            continue;
        };
        if turn.timestamp != tick.current {
            continue;
        }
        if turn.content.is_empty() {
            continue;
        }
        let listeners: Vec<Entity> = conv
            .participants
            .iter()
            .copied()
            .filter(|e| *e != turn.speaker)
            .collect();
        for listener in listeners {
            let Ok(mut mind) = minds.get_mut(listener) else {
                continue;
            };
            for triple in &turn.content {
                let mut hearsay = triple.clone();
                hearsay.meta = Metadata::hearsay(tick.current, turn.speaker);
                mind.assert(hearsay);
            }

            if let Ok(mut listener_tom) = toms.get_mut(listener) {
                let count = turn.content.len();
                listener_tom.record_shared_triples(
                    turn.speaker,
                    &turn.content,
                    theory_of_mind::COMMUNICATED_BELIEF_CONFIDENCE,
                    tick.current,
                );
                sim_events.write(SimEvent::TheoryOfMindUpdated {
                    agent: listener,
                    about: turn.speaker,
                    tick: tick.current,
                    source: crate::agent::events::TheoryOfMindSource::Received,
                    belief_count: count,
                });
            }
        }
    }
}

// ============================================================================
// 4. Emit communication events (downstream feeds: relationships, emotions)
// ============================================================================

/// Re-emit the conversation's freshest turn as per-listener
/// [`GameEvent::SocialInteraction`] (and optionally
/// [`GameEvent::KnowledgeShared`]) so existing downstream systems
/// (relationship updater, memory consolidator) keep working without
/// changes. In group conversations this produces one event per listener,
/// so each pairwise relationship updates independently from the shared turn.
///
/// Valence is computed from intent, affection, mood, and personality rather
/// than being hardcoded to 0.5.
pub fn emit_communication_events(
    manager: Res<ConversationManager>,
    tick: Res<TickCount>,
    mut events: MessageWriter<GameEvent>,
    agents: Query<(&MindGraph, &EmotionalState, &Personality)>,
) {
    for conv in manager.conversations.values() {
        let Some(turn) = conv.turns.last() else {
            continue;
        };
        if turn.timestamp != tick.current {
            continue;
        }
        let speaker = turn.speaker;
        for listener in conv.participants.iter().copied() {
            if listener == speaker {
                continue;
            }
            let valence = compute_interaction_valence(turn, speaker, listener, &agents);
            events.write(GameEvent::SocialInteraction {
                actor: speaker,
                target: listener,
                action: ActionType::Converse,
                topic: Some(map_topic(turn.topic)),
                valence,
            });
            if !turn.content.is_empty() {
                events.write(GameEvent::KnowledgeShared {
                    speaker,
                    listener,
                    content: turn.content.clone(),
                });
            }
        }
    }
}

fn compute_interaction_valence(
    turn: &Turn,
    speaker: Entity,
    listener: Entity,
    agents: &Query<(&MindGraph, &EmotionalState, &Personality)>,
) -> f32 {
    let base = valence_base(turn.intent);

    let listener_affection = agents
        .get(listener)
        .ok()
        .and_then(|(mind, _, _)| mind.get(&Node::Entity(speaker), Predicate::Affection))
        .and_then(|v| {
            if let Value::Float(f) = v {
                Some(*f)
            } else {
                None
            }
        })
        .unwrap_or(0.5);

    let (speaker_mood, speaker_agreeableness) = agents
        .get(speaker)
        .map(|(_, e, p)| (e.current_mood, p.traits.agreeableness))
        .unwrap_or((0.0, 0.5));
    let listener_mood = agents
        .get(listener)
        .map(|(_, e, _)| e.current_mood)
        .unwrap_or(0.0);

    valence_from_parts(
        base,
        listener_affection,
        speaker_mood,
        listener_mood,
        speaker_agreeableness,
    )
}

/// Base valence by conversational intent, before contextual modifiers are applied.
pub(crate) fn valence_base(intent: Intent) -> f32 {
    match intent {
        Intent::Greet => 0.3,
        Intent::Ask => 0.2,
        Intent::Disagree => 0.1,
        Intent::Farewell => 0.3,
        Intent::Answer
        | Intent::Share
        | Intent::Empathize
        | Intent::Agree
        | Intent::Thank
        | Intent::Acknowledge => 0.5,
    }
}

/// Pure valence math kernel. Testable without Bevy queries.
///
/// ```text
/// valence = base
///         + (listener_affection - 0.5) * 0.4   // like them → warmer
///         + (speaker_mood + listener_mood) / 2.0 * 0.2
///         + (agreeableness - 0.5) * 0.1
/// ```
/// Clamped to [-1.0, 1.0].
pub(crate) fn valence_from_parts(
    base: f32,
    listener_affection: f32,
    speaker_mood: f32,
    listener_mood: f32,
    speaker_agreeableness: f32,
) -> f32 {
    let affection_modifier = (listener_affection - 0.5) * 0.4;
    let mood_modifier = (speaker_mood + listener_mood) / 2.0 * 0.2;
    let personality_modifier = (speaker_agreeableness - 0.5) * 0.1;
    (base + affection_modifier + mood_modifier + personality_modifier).clamp(-1.0, 1.0)
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

/// Per-participant continuation check. Each tick:
///
/// - If an individual participant lost their `Converse` marker (preempted
///   by Sleep / Flee / Fight), moved out of range of the group, or left the
///   world, they're removed from the conversation. A
///   [`SimEvent::ConversationLeft`] fires for graceful exits and
///   [`SimEvent::ConversationAbandoned`] for rude ones (one event per
///   remaining counterparty, since each pair relationship reacts
///   independently).
/// - If fewer than two participants remain, or the group went stale, the
///   whole conversation is finalized and [`SimEvent::ConversationEnded`]
///   fires with the final participant list.
///
/// "In range" is defined as within [`CONVERSATION_RANGE`] of at least one
/// other participant — a scatter-and-cluster group (everyone drifts but
/// stays near someone) keeps talking.
pub fn evaluate_conversation_continuation(
    mut commands: Commands,
    mut manager: ResMut<ConversationManager>,
    mut events: MessageWriter<ConversationAbandoned>,
    mut sim_events: MessageWriter<SimEvent>,
    mut game_events: MessageWriter<GameEvent>,
    tick: Res<TickCount>,
    transforms: Query<&Transform>,
    actives: Query<&ActiveActions>,
) {
    let mut to_finalize: Vec<u64> = Vec::new();

    for (id, conv) in manager.conversations.iter_mut() {
        if conv.state == ConversationState::Ended {
            to_finalize.push(*id);
            continue;
        }

        let stale = tick.current.saturating_sub(conv.last_turn_at) > STALE_CONVERSATION_TICKS;
        if stale {
            conv.state = ConversationState::Ended;
            to_finalize.push(*id);
            continue;
        }

        // Snapshot positions so we can run per-pair range checks.
        let positions: Vec<(Entity, Option<Vec2>)> = conv
            .participants
            .iter()
            .map(|e| {
                (
                    *e,
                    transforms.get(*e).ok().map(|t| t.translation.truncate()),
                )
            })
            .collect();

        // Decide who's still part of the conversation and why each leaver left.
        let graceful_state = conv.turns.last().map(|t| t.intent) == Some(Intent::Farewell);
        let mut leavers: Vec<(Entity, bool)> = Vec::new(); // (entity, graceful)
        for (entity, pos) in &positions {
            let Some(my_pos) = pos else {
                // Entity has no transform (despawned) — treat as rude leaver.
                leavers.push((*entity, false));
                continue;
            };
            let channel_ok = actives
                .get(*entity)
                .map(|a| a.contains(ActionType::Converse))
                .unwrap_or(false);
            if !channel_ok {
                leavers.push((*entity, graceful_state));
                continue;
            }
            // In range of at least one other participant?
            let near_someone = positions.iter().any(|(other, other_pos)| {
                if other == entity {
                    return false;
                }
                other_pos
                    .map(|p| p.distance(*my_pos) <= CONVERSATION_RANGE)
                    .unwrap_or(false)
            });
            if !near_someone {
                leavers.push((*entity, graceful_state));
            }
        }

        for (leaver, graceful) in &leavers {
            // For every counterparty still in the group, emit the relationship
            // impact (one event per pair).
            let counterparties: Vec<Entity> = conv
                .participants
                .iter()
                .copied()
                .filter(|e| e != leaver && !leavers.iter().any(|(l, _)| l == e))
                .collect();

            if *graceful {
                sim_events.write(SimEvent::ConversationLeft {
                    leaver: *leaver,
                    tick: tick.current,
                    conversation_id: *id,
                });
            } else {
                for other in &counterparties {
                    events.write(ConversationAbandoned {
                        abandoner: *leaver,
                        abandoned: *other,
                        conversation_state: conv.state,
                    });
                    sim_events.write(SimEvent::ConversationAbandoned {
                        abandoner: *leaver,
                        abandoned: *other,
                        tick: tick.current,
                    });
                    game_events.write(GameEvent::SocialInteraction {
                        actor: *leaver,
                        target: *other,
                        action: ActionType::Converse,
                        topic: None,
                        valence: -0.4,
                    });
                }
            }

            commands.entity(*leaver).remove::<InConversation>();
            commands.entity(*leaver).queue(RemoveConverseMarker);
            conv.remove_participant(*leaver);
        }

        if conv.participants.len() < 2 {
            conv.state = ConversationState::Ended;
            to_finalize.push(*id);
        }
    }

    // Finalize: drop InConversation, remove the Converse marker, drop the
    // entry from the manager so it doesn't grow unbounded.
    for id in to_finalize {
        if let Some(conv) = manager.conversations.get(&id) {
            sim_events.write(SimEvent::ConversationEnded {
                participants: conv.participants.clone(),
                tick: tick.current,
                conversation_id: id,
            });
            for entity in &conv.participants {
                commands.entity(*entity).remove::<InConversation>();
                commands.entity(*entity).queue(RemoveConverseMarker);
            }
        }
        manager.conversations.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_intent_produces_lower_base_valence_than_share() {
        assert!(valence_base(Intent::Ask) < valence_base(Intent::Share));
    }

    #[test]
    fn disagree_intent_produces_lowest_base_valence() {
        let all_intents = [
            Intent::Greet,
            Intent::Ask,
            Intent::Answer,
            Intent::Share,
            Intent::Empathize,
            Intent::Agree,
            Intent::Disagree,
            Intent::Thank,
            Intent::Farewell,
            Intent::Acknowledge,
        ];
        let disagree = valence_base(Intent::Disagree);
        for intent in all_intents {
            assert!(
                valence_base(intent) >= disagree,
                "{intent:?} base ({}) is less than Disagree ({})",
                valence_base(intent),
                disagree
            );
        }
    }

    #[test]
    fn high_affection_raises_valence_compared_to_neutral() {
        let base = valence_base(Intent::Share);
        let neutral = valence_from_parts(base, 0.5, 0.0, 0.0, 0.5);
        let friend = valence_from_parts(base, 1.0, 0.0, 0.0, 0.5);
        let enemy = valence_from_parts(base, 0.0, 0.0, 0.0, 0.5);
        assert!(
            friend > neutral,
            "friend valence {friend} should exceed neutral {neutral}"
        );
        assert!(
            enemy < neutral,
            "enemy valence {enemy} should be below neutral {neutral}"
        );
    }

    #[test]
    fn positive_mood_raises_valence() {
        let base = valence_base(Intent::Share);
        let neutral_mood = valence_from_parts(base, 0.5, 0.0, 0.0, 0.5);
        let good_mood = valence_from_parts(base, 0.5, 1.0, 1.0, 0.5);
        assert!(good_mood > neutral_mood);
    }

    #[test]
    fn valence_is_clamped_to_minus_one_to_one() {
        let v_max = valence_from_parts(1.0, 1.0, 1.0, 1.0, 1.0);
        let v_min = valence_from_parts(-1.0, 0.0, -1.0, -1.0, 0.0);
        assert!(v_max <= 1.0);
        assert!(v_min >= -1.0);
    }

    #[test]
    fn agreeable_speaker_produces_higher_valence() {
        let base = valence_base(Intent::Share);
        let low_agree = valence_from_parts(base, 0.5, 0.0, 0.0, 0.0);
        let high_agree = valence_from_parts(base, 0.5, 0.0, 0.0, 1.0);
        assert!(high_agree > low_agree);
    }

    #[test]
    fn map_topic_general_is_greetings() {
        assert_eq!(map_topic(Topic::General), ConversationTopic::Greetings);
    }

    #[test]
    fn map_topic_help_is_request() {
        assert_eq!(map_topic(Topic::Help), ConversationTopic::Request);
    }

    // ── select_intent tests ─────────────────────────────────────────────────

    use crate::agent::mind::knowledge::{MemoryType, Metadata, Source, Triple};
    use crate::agent::mind::theory_of_mind::TheoryOfMind;

    fn empty_mind() -> MindGraph {
        MindGraph::default()
    }

    fn danger_triple(subject: Node, salience: f32, timestamp: u64) -> Triple {
        Triple::with_meta(
            subject,
            Predicate::HasTrait,
            Value::Concept(Concept::Dangerous),
            Metadata {
                source: Source::Experienced,
                memory_type: MemoryType::Episodic,
                timestamp,
                confidence: 1.0,
                informant: None,
                evidence: Vec::new(),
                salience,
            },
        )
    }

    #[test]
    fn select_intent_greets_on_first_turn() {
        let conv = Conversation::new(0, vec![Entity::from_bits(1), Entity::from_bits(2)], 0);
        let mind = empty_mind();
        let listener = Entity::from_bits(2);
        assert_eq!(
            select_intent(&conv, &mind, None, listener, None, None, 0, false, false),
            Intent::Greet
        );
    }

    #[test]
    fn select_intent_farewell_when_wrapping() {
        let mut conv = Conversation::new(0, vec![Entity::from_bits(1), Entity::from_bits(2)], 0);
        conv.state = ConversationState::Wrapping;
        let mind = empty_mind();
        let listener = Entity::from_bits(2);
        assert_eq!(
            select_intent(&conv, &mind, None, listener, None, None, 0, false, false),
            Intent::Farewell
        );
    }

    #[test]
    fn select_intent_warns_when_danger_known_and_partner_unaware() {
        let mut conv = Conversation::new(0, vec![Entity::from_bits(1), Entity::from_bits(2)], 0);
        conv.state = ConversationState::Active;
        // Inject a dummy turn so we're not in first-turn state.
        conv.add_turn(Turn {
            speaker: Entity::from_bits(1),
            intent: Intent::Greet,
            topic: crate::agent::mind::conversation::Topic::General,
            emotion: None,
            content: Vec::new(),
            timestamp: 0,
            expects_response: false,
        });

        let mut speaker = empty_mind();
        speaker.assert(danger_triple(
            Node::Concept(Concept::Wolf),
            0.9, // high salience
            0,   // at tick 0, queried at tick 100 → within DANGER_RECENCY_TICKS
        ));
        // No ToM = stranger model → assume listener doesn't know
        let listener = Entity::from_bits(2);

        assert_eq!(
            select_intent(
                &conv, &speaker, None, listener, None, None, 100, false, false
            ),
            Intent::Share,
            "should warn about danger the listener doesn't know"
        );
    }

    #[test]
    fn select_intent_does_not_warn_if_speaker_believes_listener_knows() {
        let mut conv = Conversation::new(0, vec![Entity::from_bits(1), Entity::from_bits(2)], 0);
        conv.state = ConversationState::Active;
        conv.add_turn(Turn {
            speaker: Entity::from_bits(1),
            intent: Intent::Greet,
            topic: crate::agent::mind::conversation::Topic::General,
            emotion: None,
            content: Vec::new(),
            timestamp: 0,
            expects_response: false,
        });

        let mut speaker = empty_mind();
        speaker.assert(danger_triple(Node::Concept(Concept::Wolf), 0.9, 0));

        // Speaker believes listener already knows about the wolf danger.
        let listener = Entity::from_bits(2);
        let mut tom = TheoryOfMind::default();
        tom.record_belief(
            listener,
            Node::Concept(Concept::Wolf),
            Predicate::HasTrait,
            Value::Concept(Concept::Dangerous),
            1.0,
            0,
        );

        // With nothing else to share and no content available, falls through to Acknowledge.
        assert_eq!(
            select_intent(
                &conv,
                &speaker,
                Some(&tom),
                listener,
                None,
                None,
                100,
                false,
                false
            ),
            Intent::Acknowledge,
            "should not warn about danger the speaker believes the listener already knows"
        );
    }

    #[test]
    fn select_intent_asks_when_goal_needs_location() {
        use crate::agent::brains::thinking::{Goal, TriplePattern};

        let mut conv = Conversation::new(0, vec![Entity::from_bits(1), Entity::from_bits(2)], 0);
        conv.state = ConversationState::Active;
        conv.add_turn(Turn {
            speaker: Entity::from_bits(1),
            intent: Intent::Greet,
            topic: crate::agent::mind::conversation::Topic::General,
            emotion: None,
            content: Vec::new(),
            timestamp: 0,
            expects_response: false,
        });

        let mind = empty_mind();
        let listener = Entity::from_bits(2);
        let goal = Goal {
            conditions: vec![TriplePattern::new(None, Some(Predicate::LocatedAt), None)],
            priority: 1.0,
        };

        assert_eq!(
            select_intent(
                &conv,
                &mind,
                None,
                listener,
                Some(&goal),
                None,
                0,
                false,
                false
            ),
            Intent::Ask,
            "agent with location-seeking goal should Ask"
        );
    }

    #[test]
    fn select_intent_answers_when_partner_asked() {
        let mut conv = Conversation::new(0, vec![Entity::from_bits(1), Entity::from_bits(2)], 0);
        conv.state = ConversationState::Active;
        // Add a turn that expects a response.
        conv.add_turn(Turn {
            speaker: Entity::from_bits(2),
            intent: Intent::Ask,
            topic: crate::agent::mind::conversation::Topic::General,
            emotion: None,
            content: Vec::new(),
            timestamp: 0,
            expects_response: true, // partner asked
        });

        let mind = empty_mind();
        let listener = Entity::from_bits(2);
        assert_eq!(
            select_intent(&conv, &mind, None, listener, None, None, 0, false, false),
            Intent::Answer,
            "should Answer when partner asked last turn"
        );
    }

    #[test]
    fn select_intent_defaults_to_acknowledge() {
        let mut conv = Conversation::new(0, vec![Entity::from_bits(1), Entity::from_bits(2)], 0);
        conv.state = ConversationState::Active;
        conv.add_turn(Turn {
            speaker: Entity::from_bits(1),
            intent: Intent::Greet,
            topic: crate::agent::mind::conversation::Topic::General,
            emotion: None,
            content: Vec::new(),
            timestamp: 0,
            expects_response: false,
        });

        // Empty minds, no goal, low extraversion, no content → Acknowledge.
        let mind = empty_mind();
        let listener = Entity::from_bits(2);
        assert_eq!(
            select_intent(&conv, &mind, None, listener, None, None, 0, false, false),
            Intent::Acknowledge,
            "nothing to say → Acknowledge"
        );
    }
}
