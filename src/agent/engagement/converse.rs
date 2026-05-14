//! `EngagementKind::Converse` — conversation data types, registry, and
//! per-tick lifecycle systems. Turn ownership lives on
//! [`Conversation::turn`], never on a component flag, so there's no
//! race possible.

use bevy::ecs::world::EntityWorldMut;
use bevy::prelude::*;
use std::collections::HashSet;

use super::component::{Engaged, EngagementEndReason, EngagementId, EngagementKind};
use super::markers::EngagedConverse;
use super::registry::EngagementRegistry;
use crate::agent::Agent;
use crate::agent::actions::registry::{ActionState, ActiveActions};
use crate::agent::actions::types::ActionType;
use crate::agent::body::needs::{Consciousness, PsychologicalDrives};
use crate::agent::brains::plan_memory::{
    HeldPlan, PlanAbandonReason, PlanMemory, PlanSource, PlanState,
};
use crate::agent::brains::thinking::{Goal, TriplePattern};
use crate::agent::events::{
    ConversationTopic, EngagementBeatPayload, FailureReason, GameEvent, SimEvent, SimEventKind,
};
use crate::agent::mind::knowledge::{Concept, Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::agent::mind::social_perception::CONVERSATION_RANGE;
use crate::agent::mind::theory_of_mind::{self, TheoryOfMind};
use crate::agent::psyche::emotions::{Emotion, EmotionalState};
use crate::agent::psyche::personality::Personality;
use crate::core::not_paused;
use crate::core::tick::TickCount;

// ============================================================================
// Tunables
// ============================================================================

pub const MAX_GROUP_SIZE: usize = 6;
pub const STALE_CONVERSATION_TICKS: u64 = 500;
pub const CHITCHAT_INTERVAL_TICKS: u64 = 30;
pub const URGENT_INTERVAL_TICKS: u64 = 15;
pub const FAREWELL_INTERVAL_TICKS: u64 = 15;
pub const NATURAL_END_TURN_COUNT: usize = 6;
pub const SOCIAL_DRIVE_PER_TURN: f32 = 0.03;
pub const SMALL_TALK_TRIPLES_PER_TURN: usize = 3;
pub const DANGER_WARN_SALIENCE: f32 = 0.7;
pub const DANGER_RECENCY_TICKS: u64 = 600;

// ============================================================================
// Data types
// ============================================================================

/// One live conversation. The kind-specific payload behind a Converse
/// engagement.
#[derive(Debug, Clone, Reflect)]
pub struct Conversation {
    pub id: EngagementId,
    pub participants: Vec<Entity>,
    /// Index into `participants` of the agent whose turn it currently is.
    pub turn: usize,
    pub state: ConversationState,
    pub started_at: u64,
    pub last_turn_at: u64,
    pub turns: Vec<Turn>,
    /// Listeners who signalled they want to speak next.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, serde::Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Default)]
pub enum Topic {
    #[default]
    General,
    Location(Concept),
    State(Entity),
    Person(Entity),
    Help,
}

impl Conversation {
    pub fn new(id: EngagementId, participants: Vec<Entity>, started_at: u64) -> Self {
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

    pub fn add_turn(&mut self, turn: Turn) {
        self.last_turn_at = turn.timestamp;
        self.turns.push(turn);
    }

    pub fn set_speaker(&mut self, speaker: Entity) {
        if let Some(idx) = self.participants.iter().position(|e| *e == speaker) {
            self.turn = idx;
        }
    }

    pub fn current_speaker(&self) -> Entity {
        self.participants[self.turn]
    }

    pub fn listeners_for(&self, speaker: Entity) -> impl Iterator<Item = Entity> + '_ {
        self.participants
            .iter()
            .copied()
            .filter(move |e| *e != speaker)
    }

    pub fn listeners(&self) -> impl Iterator<Item = Entity> + '_ {
        self.listeners_for(self.current_speaker())
    }

    pub fn last_turn_expects_response(&self) -> bool {
        self.turns
            .last()
            .map(|t| t.expects_response)
            .unwrap_or(false)
    }

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

/// Per-kind registry of live Converse engagements. Keyed by
/// [`EngagementId`] so the generic `Engaged` component on each
/// participant can find its payload here.
#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct ConverseRegistry {
    pub conversations: std::collections::HashMap<EngagementId, Conversation>,
}

impl ConverseRegistry {
    pub fn start(
        &mut self,
        ids: &mut EngagementRegistry,
        participants: Vec<Entity>,
        tick: u64,
    ) -> EngagementId {
        let id = ids.mint();
        self.conversations
            .insert(id, Conversation::new(id, participants, tick));
        id
    }

    pub fn get(&self, id: EngagementId) -> Option<&Conversation> {
        self.conversations.get(&id)
    }

    pub fn get_mut(&mut self, id: EngagementId) -> Option<&mut Conversation> {
        self.conversations.get_mut(&id)
    }

    pub fn find_active_for(&self, entity: Entity) -> Option<&Conversation> {
        self.conversations
            .values()
            .find(|c| c.state != ConversationState::Ended && c.participants.contains(&entity))
    }

    pub fn active(&self) -> impl Iterator<Item = &Conversation> {
        self.conversations
            .values()
            .filter(|c| c.state != ConversationState::Ended)
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct ConversePlugin;

impl Plugin for ConversePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConverseRegistry>()
            .add_systems(
                FixedUpdate,
                (
                    process_initiate_conversation
                        .after(crate::agent::nervous_system::execution::start_actions),
                    evaluate_conversation_continuation.after(emit_communication_events),
                )
                    .in_set(crate::core::PerfBucket::Communication)
                    .in_set(crate::core::PerfSubBucket::CommunicationLifecycle)
                    .run_if(not_paused),
            )
            .add_systems(
                FixedUpdate,
                (
                    select_turn_intent.after(process_initiate_conversation),
                    update_speaker_theory_of_mind.after(select_turn_intent),
                    process_received_communication.after(select_turn_intent),
                    emit_communication_events.after(process_received_communication),
                )
                    .in_set(crate::core::PerfBucket::Communication)
                    .in_set(crate::core::PerfSubBucket::CommunicationTurn)
                    .run_if(not_paused),
            );
    }
}

// ============================================================================
// 1. InitiateConversation lifecycle (entry point owned by this plugin)
// ============================================================================

pub fn process_initiate_conversation(
    mut commands: Commands,
    mut registry: ResMut<ConverseRegistry>,
    mut id_minter: ResMut<EngagementRegistry>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
    transforms: Query<&Transform, With<Agent>>,
    engaged: Query<&Engaged, With<Agent>>,
    mut active_actions: Query<(Entity, &mut ActiveActions), With<Agent>>,
    mut target_positions: Query<&mut crate::agent::TargetPosition>,
    mut plan_memory_query: Query<&mut PlanMemory>,
) {
    let pairs: Vec<(Entity, Option<Entity>)> = active_actions
        .iter()
        .filter_map(|(entity, active)| {
            active
                .get(ActionType::InitiateConversation)
                .map(|state| (entity, state.target_entity))
        })
        .collect();

    for (initiator, partner) in pairs {
        let now = tick.current;

        // Initiator already engaged (in conversation or otherwise) — drop
        // the stale action. Same-tick dedupe also checks the registry
        // because the Engaged component insert won't have flushed yet.
        if engaged.get(initiator).is_ok() || registry.find_active_for(initiator).is_some() {
            drop_stale_initiate(
                initiator,
                now,
                DropKind::Complete,
                &mut active_actions,
                &mut plan_memory_query,
                &mut sim_events,
            );
            continue;
        }
        let Some(partner) = partner else {
            drop_stale_initiate(
                initiator,
                now,
                DropKind::Abandon {
                    reason: FailureReason::NoTarget,
                },
                &mut active_actions,
                &mut plan_memory_query,
                &mut sim_events,
            );
            continue;
        };
        let Ok(partner_transform) = transforms.get(partner) else {
            drop_stale_initiate(
                initiator,
                now,
                DropKind::Abandon {
                    reason: FailureReason::TargetGone,
                },
                &mut active_actions,
                &mut plan_memory_query,
                &mut sim_events,
            );
            continue;
        };
        let Ok(initiator_transform) = transforms.get(initiator) else {
            continue;
        };
        let initiator_pos = initiator_transform.translation.truncate();
        let partner_pos = partner_transform.translation.truncate();
        let distance = initiator_pos.distance(partner_pos);

        if let Ok((_, mut active)) = active_actions.get_mut(initiator)
            && let Some(state) = active.get_mut(ActionType::InitiateConversation)
        {
            state.target_position = Some(partner_pos);
        }
        if let Ok(mut tp) = target_positions.get_mut(initiator) {
            tp.0 = Some(partner_pos);
        }

        if distance > CONVERSATION_RANGE {
            continue;
        }

        // In range. Two cases: (1) partner is mid-Converse → join the
        // existing engagement as a third+ participant, (2) partner is
        // free → mint a new engagement.
        let join_target: Option<EngagementId> = engaged
            .get(partner)
            .ok()
            .filter(|e| e.kind == EngagementKind::Converse)
            .map(|e| e.id)
            .or_else(|| registry.find_active_for(partner).map(|c| c.id));

        let (engagement_id, is_new) = if let Some(existing_id) = join_target {
            let Some(conv) = registry.conversations.get_mut(&existing_id) else {
                drop_stale_initiate(
                    initiator,
                    now,
                    DropKind::Abandon {
                        reason: FailureReason::TargetGone,
                    },
                    &mut active_actions,
                    &mut plan_memory_query,
                    &mut sim_events,
                );
                continue;
            };
            if conv.state == ConversationState::Ended {
                drop_stale_initiate(
                    initiator,
                    now,
                    DropKind::Abandon {
                        reason: FailureReason::Interrupted,
                    },
                    &mut active_actions,
                    &mut plan_memory_query,
                    &mut sim_events,
                );
                continue;
            }
            if !conv.add_participant(initiator) {
                drop_stale_initiate(
                    initiator,
                    now,
                    DropKind::Abandon {
                        reason: FailureReason::ConversationFull,
                    },
                    &mut active_actions,
                    &mut plan_memory_query,
                    &mut sim_events,
                );
                continue;
            }
            (existing_id, false)
        } else {
            let id = registry.start(&mut id_minter, vec![initiator, partner], now);
            (id, true)
        };

        commands.entity(initiator).insert((
            Engaged::new(EngagementKind::Converse, engagement_id),
            EngagedConverse(engagement_id),
        ));
        if is_new {
            commands.entity(partner).insert((
                Engaged::new(EngagementKind::Converse, engagement_id),
                EngagedConverse(engagement_id),
            ));
        }

        if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
            active.remove(ActionType::InitiateConversation);
            if !active.contains(ActionType::Converse) {
                active.insert(ActionState::new(ActionType::Converse, now));
            }
        }
        if let Ok(mut memory) = plan_memory_query.get_mut(initiator) {
            let doomed: Vec<_> = memory
                .plans
                .iter()
                .filter(|p| {
                    p.current()
                        .map(|a| a.action_type == ActionType::InitiateConversation)
                        .unwrap_or(false)
                })
                .map(|p| p.id)
                .collect();
            for id in doomed {
                memory.remove(id);
            }
        }
        if is_new
            && let Ok((_, mut active)) = active_actions.get_mut(partner)
            && !active.contains(ActionType::Converse)
        {
            active.insert(ActionState::new(ActionType::Converse, now));
        }
        if let Ok(mut tp) = target_positions.get_mut(initiator) {
            tp.0 = None;
        }

        if is_new {
            sim_events.write(SimEvent::new(
                now,
                vec![initiator, partner],
                SimEventKind::EngagementStarted {
                    kind: EngagementKind::Converse,
                    engagement_id,
                    participants: vec![initiator, partner],
                },
            ));
        } else {
            sim_events.write(SimEvent::single(
                now,
                initiator,
                SimEventKind::EngagementJoined {
                    kind: EngagementKind::Converse,
                    engagement_id,
                    joiner: initiator,
                },
            ));
        }
    }
}

enum DropKind {
    Complete,
    Abandon { reason: FailureReason },
}

fn drop_stale_initiate(
    initiator: Entity,
    tick: u64,
    kind: DropKind,
    active_actions: &mut Query<(Entity, &mut ActiveActions), With<Agent>>,
    plan_memory_query: &mut Query<&mut PlanMemory>,
    sim_events: &mut MessageWriter<SimEvent>,
) {
    if let Ok((_, mut active)) = active_actions.get_mut(initiator) {
        active.remove(ActionType::InitiateConversation);
    }

    let abandon = matches!(kind, DropKind::Abandon { .. });
    if let Ok(mut memory) = plan_memory_query.get_mut(initiator) {
        let doomed: Vec<_> = memory
            .plans
            .iter()
            .filter(|p| {
                p.current()
                    .map(|a| a.action_type == ActionType::InitiateConversation)
                    .unwrap_or(false)
            })
            .map(|p| (p.id, p.driving_urgency))
            .collect();
        for (id, source) in doomed {
            memory.remove(id);
            if abandon {
                sim_events.write(SimEvent::plan_abandoned(
                    tick,
                    initiator,
                    id,
                    source,
                    PlanAbandonReason::StepAdvancedInvalid,
                ));
            }
        }
    }

    match kind {
        DropKind::Complete => {
            sim_events.write(SimEvent::single(
                tick,
                initiator,
                SimEventKind::ActionCompleted {
                    agent: initiator,
                    action: ActionType::InitiateConversation,
                    target: None,
                },
            ));
        }
        DropKind::Abandon { reason } => {
            sim_events.write(SimEvent::single(
                tick,
                initiator,
                SimEventKind::ActionFailed {
                    agent: initiator,
                    action: ActionType::InitiateConversation,
                    reason,
                },
            ));
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
// Verbal commitment + content selection helpers (unchanged from the
// pre-migration `agent::communication` module)
// ============================================================================

fn most_committed_goal(memory: &PlanMemory) -> Option<Goal> {
    let priority_state = |state: PlanState| match state {
        PlanState::Executing => 3,
        PlanState::Considering => 2,
        PlanState::Background => 1,
        PlanState::Suspended => 0,
    };
    memory
        .plans
        .iter()
        .max_by(|a, b| {
            priority_state(a.state).cmp(&priority_state(b.state)).then(
                a.commitment
                    .partial_cmp(&b.commitment)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        })
        .map(|p| p.goal.clone())
}

fn upsert_verbal_commitment(memory: &mut PlanMemory, concept: Concept, listener: Entity, now: u64) {
    let existing_id = memory
        .plans
        .iter()
        .find(|p| p.source.is_verbal_commitment() && p.goal.target_concept() == Some(concept))
        .map(|p| p.id);
    if let Some(id) = existing_id {
        if let Some(plan) = memory.get_mut(id) {
            plan.last_touched = now;
        }
        return;
    }
    let goal = Goal {
        conditions: vec![TriplePattern::new(
            Some(Node::Self_),
            Some(Predicate::Contains),
            Some(Value::Item(concept, 1)),
        )],
        priority: 0.5,
    };
    let id = memory.mint_plan_id();
    memory.insert(HeldPlan {
        id,
        goal,
        steps: Vec::new(),
        state: PlanState::Background,
        commitment: 0.0,
        subjective_cost: 0.0,
        source: PlanSource::VerbalCommitment {
            promised_to: listener,
            agreement_tick: now,
        },
        driving_urgency: crate::agent::nervous_system::urgency::UrgencySource::Commitment,
        created_at_urgency: 0.5,
        created_at: now,
        last_touched: now,
        current_step: 0,
    });
}

// ============================================================================
// 2. Select turn intent
// ============================================================================

pub fn select_turn_intent(
    mut registry: ResMut<ConverseRegistry>,
    tick: Res<TickCount>,
    minds: Query<&MindGraph>,
    toms: Query<&TheoryOfMind>,
    personalities: Query<&Personality>,
    mut plan_memories: Query<&mut PlanMemory>,
    mut consciousnesses: Query<&mut Consciousness>,
    mut drives: Query<&mut PsychologicalDrives>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    let now = tick.current;
    for conv in registry.conversations.values_mut() {
        if conv.state == ConversationState::Ended {
            continue;
        }
        if conv.participants.len() < 2 {
            continue;
        }

        let speaker = conv.current_speaker();
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
        let speaker_goal: Option<Goal> = plan_memories
            .get(speaker)
            .ok()
            .and_then(most_committed_goal);
        let goal = speaker_goal.as_ref();
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

        let (mut content, topic) = if matches!(intent, Intent::Share | Intent::Answer) {
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
        } else if !matches!(intent, Intent::Farewell) {
            let casual = crate::agent::mind::small_talk::pick_small_talk_triples(
                speaker_mind,
                speaker_tom,
                primary_listener,
                now,
                1,
            );
            (casual, Topic::General)
        } else {
            (Vec::new(), Topic::General)
        };

        if matches!(intent, Intent::Share | Intent::Ask | Intent::Answer)
            && let Some(goal_concept) = goal.and_then(Goal::target_concept)
        {
            if let Ok(mut memory) = plan_memories.get_mut(speaker) {
                upsert_verbal_commitment(&mut memory, goal_concept, primary_listener, now);
            }
            content.push(Triple::with_meta(
                Node::Entity(speaker),
                Predicate::Committed,
                Value::Concept(goal_concept),
                Metadata::default(),
            ));
        }

        let expects_response = matches!(intent, Intent::Greet | Intent::Ask);
        let listeners: Vec<Entity> = conv.listeners().collect();
        let content_len = content.len();
        let turn = Turn {
            speaker,
            intent,
            topic,
            emotion: None,
            content,
            timestamp: now,
            expects_response,
        };
        conv.add_turn(turn);
        conv.wants_to_speak.remove(&speaker);

        let speaker_drain_base =
            crate::constants::brains::cognition::CONVERSATION_SPEAKER_ALERTNESS_DRAIN;
        let listener_drain_base =
            crate::constants::brains::cognition::CONVERSATION_LISTENER_ALERTNESS_DRAIN;
        let extraversion_relief =
            crate::constants::brains::cognition::EXTRAVERSION_CONVERSATION_RELIEF;
        if let Ok(mut c) = consciousnesses.get_mut(speaker) {
            let extraversion = personalities
                .get(speaker)
                .map(|p| p.traits.extraversion())
                .unwrap_or(0.5);
            let drain = speaker_drain_base * (1.0 - extraversion * extraversion_relief);
            c.alertness = (c.alertness - drain).max(0.0);
        }
        for listener in listeners {
            if let Ok(mut c) = consciousnesses.get_mut(listener) {
                let extraversion = personalities
                    .get(listener)
                    .map(|p| p.traits.extraversion())
                    .unwrap_or(0.5);
                let drain = listener_drain_base * (1.0 - extraversion * extraversion_relief);
                c.alertness = (c.alertness - drain).max(0.0);
            }
        }
        if let Ok(mut d) = drives.get_mut(speaker)
            && d.companionship.value < 1.0
        {
            d.companionship.top_up(SOCIAL_DRIVE_PER_TURN);
        }

        if expects_response {
            conv.wants_to_speak.insert(primary_listener);
        }

        // Beat event for observability — one per actual turn.
        sim_events.write(SimEvent::single(
            now,
            speaker,
            SimEventKind::EngagementBeat {
                kind: EngagementKind::Converse,
                engagement_id: conv.id,
                agent: speaker,
                payload: EngagementBeatPayload::Converse {
                    speaker,
                    intent,
                    topic,
                    content_count: content_len,
                    expects_response,
                },
            },
        ));

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

        let next = pick_next_speaker(conv, &personalities);
        conv.set_speaker(next);
    }
}

pub(crate) fn speak_desire(personality: Option<&Personality>, wants_to_speak: bool) -> f32 {
    let extraversion = personality.map(|p| p.traits.extraversion()).unwrap_or(0.5);
    let agreeableness = personality.map(|p| p.traits.agreeableness()).unwrap_or(0.5);
    let base = 1.0 + extraversion * 2.0 - agreeableness * 0.6;
    if wants_to_speak { base + 2.5 } else { base }
}

pub(crate) fn pick_next_speaker(
    conv: &Conversation,
    personalities: &Query<&Personality>,
) -> Entity {
    let speaker = conv.current_speaker();
    let mut candidates: [Entity; MAX_GROUP_SIZE] = [speaker; MAX_GROUP_SIZE];
    let mut scores: [f32; MAX_GROUP_SIZE] = [0.0; MAX_GROUP_SIZE];
    let mut count = 0usize;
    let mut total = 0.0f32;
    for entity in conv.listeners() {
        let p = personalities.get(entity).ok();
        let wants = conv.wants_to_speak.contains(&entity);
        let score = speak_desire(p, wants).max(0.01);
        candidates[count] = entity;
        scores[count] = score;
        total += score;
        count += 1;
    }
    if count == 0 {
        return speaker;
    }
    if count == 1 {
        return candidates[0];
    }

    let seed = conv.id.0.wrapping_mul(2_654_435_761) ^ (conv.turns.len() as u64);
    let frac = ((seed % 1_000_000) as f32) / 1_000_000.0;
    let mut target = frac * total;
    for i in 0..count {
        target -= scores[i];
        if target <= 0.0 {
            return candidates[i];
        }
    }
    candidates[count - 1]
}

#[allow(clippy::too_many_arguments)]
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
    let neuroticism = personality.map(|p| p.traits.neuroticism()).unwrap_or(0.5);
    let extraversion = personality.map(|p| p.traits.extraversion()).unwrap_or(0.5);

    if conv.state == ConversationState::Greeting && conv.turns.is_empty() {
        return Intent::Greet;
    }

    if matches!(
        conv.state,
        ConversationState::Wrapping | ConversationState::Ended
    ) {
        return Intent::Farewell;
    }

    let warn_threshold = (DANGER_WARN_SALIENCE - (neuroticism - 0.5) * 0.2).clamp(0.3, 1.0);
    if has_danger_to_warn(speaker_mind, speaker_tom, listener, warn_threshold, now) {
        return Intent::Share;
    }

    if let Some(g) = goal
        && goal_needs_location(g)
    {
        return Intent::Ask;
    }

    if conv.last_turn_expects_response() {
        return Intent::Answer;
    }

    if has_deliberate {
        return Intent::Share;
    }

    let agreeableness = personality.map(|p| p.traits.agreeableness()).unwrap_or(0.5);
    let other_last = conv
        .turns
        .last()
        .filter(|t| t.speaker != conv.current_speaker());

    let default_traits = crate::agent::psyche::personality::PersonalityTraits::default();
    let traits = personality.map(|p| &p.traits).unwrap_or(&default_traits);
    if let Some(last) = other_last
        && let Some(emotion) = &last.emotion
        && crate::agent::psyche::emotions::emotion_valence(emotion.emotion_type, traits) < 0.0
        && agreeableness > 0.4
    {
        return Intent::Empathize;
    }

    if let Some(last) = other_last
        && matches!(last.intent, Intent::Share)
        && agreeableness > 0.5
    {
        return Intent::Agree;
    }

    if extraversion > 0.3 && has_casual {
        return Intent::Share;
    }

    Intent::Acknowledge
}

fn intent_interval(intent: Intent) -> u64 {
    match intent {
        Intent::Greet => 0,
        Intent::Farewell => FAREWELL_INTERVAL_TICKS,
        Intent::Ask | Intent::Answer => URGENT_INTERVAL_TICKS,
        _ => CHITCHAT_INTERVAL_TICKS,
    }
}

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
                .unwrap_or(true)
    })
}

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

pub fn update_speaker_theory_of_mind(
    registry: Res<ConverseRegistry>,
    tick: Res<TickCount>,
    mut toms: Query<&mut TheoryOfMind>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    for conv in registry.conversations.values() {
        let Some(turn) = conv.turns.last() else {
            continue;
        };
        if turn.timestamp != tick.current || turn.content.is_empty() {
            continue;
        }
        let speaker = turn.speaker;
        let Ok(mut speaker_tom) = toms.get_mut(speaker) else {
            continue;
        };
        let count = turn.content.len();
        for listener in conv.listeners_for(speaker) {
            speaker_tom.record_shared_triples(
                listener,
                &turn.content,
                theory_of_mind::COMMUNICATED_BELIEF_CONFIDENCE,
                tick.current,
            );
            sim_events.write(SimEvent::single(
                tick.current,
                speaker,
                SimEventKind::TheoryOfMindUpdated {
                    agent: speaker,
                    about: listener,
                    source: crate::agent::events::TheoryOfMindSource::Communicated,
                    belief_count: count,
                },
            ));
        }
    }
}

// ============================================================================
// 3. Process received communication (write hearsay into listener's mind)
// ============================================================================

fn fuzzify_hearsay(source: &Triple, tick: u64, speaker: Entity) -> Triple {
    let mut hearsay = source.clone();
    if let Value::Quantity(q) = &hearsay.object
        && let Some(fuzzy) = q.fuzzify()
    {
        hearsay.object = Value::Quantity(fuzzy);
    }
    hearsay.meta = Metadata::hearsay(tick, speaker);
    hearsay
}

pub fn process_received_communication(
    registry: Res<ConverseRegistry>,
    mut minds: Query<&mut MindGraph>,
    mut toms: Query<&mut TheoryOfMind>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    for conv in registry.conversations.values() {
        let Some(turn) = conv.turns.last() else {
            continue;
        };
        if turn.timestamp != tick.current {
            continue;
        }
        if turn.content.is_empty() {
            continue;
        }
        for listener in conv.listeners_for(turn.speaker) {
            let Ok(mut mind) = minds.get_mut(listener) else {
                continue;
            };
            for triple in &turn.content {
                mind.assert(fuzzify_hearsay(triple, tick.current, turn.speaker));
            }

            if let Ok(mut listener_tom) = toms.get_mut(listener) {
                let count = turn.content.len();
                listener_tom.record_shared_triples(
                    turn.speaker,
                    &turn.content,
                    theory_of_mind::COMMUNICATED_BELIEF_CONFIDENCE,
                    tick.current,
                );
                sim_events.write(SimEvent::single(
                    tick.current,
                    listener,
                    SimEventKind::TheoryOfMindUpdated {
                        agent: listener,
                        about: turn.speaker,
                        source: crate::agent::events::TheoryOfMindSource::Received,
                        belief_count: count,
                    },
                ));
            }
        }
    }
}

// ============================================================================
// 4. Emit communication events (downstream feeds: relationships, emotions)
// ============================================================================

pub fn emit_communication_events(
    registry: Res<ConverseRegistry>,
    tick: Res<TickCount>,
    social_graph: Res<crate::agent::psyche::social_graph::SocialGraph>,
    mut events: MessageWriter<GameEvent>,
    agents: Query<(&MindGraph, &EmotionalState, &Personality)>,
) {
    for conv in registry.conversations.values() {
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
            let valence =
                compute_interaction_valence(turn, speaker, listener, &agents, &social_graph);
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
    social_graph: &crate::agent::psyche::social_graph::SocialGraph,
) -> f32 {
    let base = valence_base(turn.intent);

    let listener_affection = social_graph.affection(listener, speaker);

    let (speaker_mood, speaker_agreeableness) = agents
        .get(speaker)
        .map(|(_, e, p)| (e.current_mood, p.traits.agreeableness()))
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

pub fn evaluate_conversation_continuation(
    mut commands: Commands,
    mut registry: ResMut<ConverseRegistry>,
    mut sim_events: MessageWriter<SimEvent>,
    mut game_events: MessageWriter<GameEvent>,
    tick: Res<TickCount>,
    transforms: Query<&Transform>,
    actives: Query<&ActiveActions>,
) {
    let mut to_finalize: Vec<EngagementId> = Vec::new();

    for (id, conv) in registry.conversations.iter_mut() {
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

        let graceful_state = conv.turns.last().map(|t| t.intent) == Some(Intent::Farewell);
        let mut leavers: Vec<(Entity, bool)> = Vec::new();
        for (entity, pos) in &positions {
            let Some(my_pos) = pos else {
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
            let counterparties: Vec<Entity> = conv
                .participants
                .iter()
                .copied()
                .filter(|e| e != leaver && !leavers.iter().any(|(l, _)| l == e))
                .collect();

            if !*graceful {
                for other in &counterparties {
                    sim_events.write(SimEvent::pair(
                        tick.current,
                        *leaver,
                        *other,
                        SimEventKind::EngagementEnded {
                            kind: EngagementKind::Converse,
                            engagement_id: *id,
                            participants: vec![*leaver, *other],
                            reason: EngagementEndReason::Abandoned,
                        },
                    ));
                    game_events.write(GameEvent::SocialInteraction {
                        actor: *leaver,
                        target: *other,
                        action: ActionType::Converse,
                        topic: None,
                        valence: -0.4,
                    });
                }
            }

            commands
                .entity(*leaver)
                .remove::<Engaged>()
                .remove::<EngagedConverse>();
            commands.entity(*leaver).queue(RemoveConverseMarker);
            conv.remove_participant(*leaver);
        }

        if conv.participants.len() < 2 {
            conv.state = ConversationState::Ended;
            to_finalize.push(*id);
        }
    }

    for id in to_finalize {
        if let Some(conv) = registry.conversations.get(&id) {
            let reason = if conv.turns.last().map(|t| t.intent) == Some(Intent::Farewell) {
                EngagementEndReason::Natural
            } else if tick.current.saturating_sub(conv.last_turn_at) > STALE_CONVERSATION_TICKS {
                EngagementEndReason::Stale
            } else {
                EngagementEndReason::Natural
            };
            sim_events.write(SimEvent::new(
                tick.current,
                conv.participants.clone(),
                SimEventKind::EngagementEnded {
                    kind: EngagementKind::Converse,
                    engagement_id: id,
                    participants: conv.participants.clone(),
                    reason,
                },
            ));
            for entity in &conv.participants {
                commands
                    .entity(*entity)
                    .remove::<Engaged>()
                    .remove::<EngagedConverse>();
                commands.entity(*entity).queue(RemoveConverseMarker);
            }
        }
        registry.conversations.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{Magnitude, Quantity};

    fn e(id: u64) -> Entity {
        Entity::from_bits(id)
    }

    fn conv_id(n: u64) -> EngagementId {
        EngagementId(n)
    }

    #[test]
    fn add_participant_respects_capacity() {
        let mut conv = Conversation::new(conv_id(0), vec![e(1), e(2)], 0);
        for i in 3..=(MAX_GROUP_SIZE as u64) {
            assert!(conv.add_participant(e(i)));
        }
        assert!(!conv.add_participant(e(100)));
        assert_eq!(conv.participants.len(), MAX_GROUP_SIZE);
    }

    #[test]
    fn add_participant_rejects_duplicates() {
        let mut conv = Conversation::new(conv_id(0), vec![e(1), e(2)], 0);
        assert!(!conv.add_participant(e(1)));
    }

    #[test]
    fn remove_participant_keeps_current_speaker_stable() {
        let mut conv = Conversation::new(conv_id(0), vec![e(1), e(2), e(3)], 0);
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
        let mut conv = Conversation::new(conv_id(0), vec![e(1), e(2), e(3)], 0);
        conv.set_speaker(e(3));
        conv.remove_participant(e(3));
        assert!(conv.turn < conv.participants.len());
    }

    #[test]
    fn listeners_excludes_current_speaker() {
        let mut conv = Conversation::new(conv_id(0), vec![e(1), e(2), e(3)], 0);
        conv.set_speaker(e(2));
        let listeners: Vec<Entity> = conv.listeners().collect();
        assert_eq!(listeners.len(), 2);
        assert!(listeners.contains(&e(1)));
        assert!(listeners.contains(&e(3)));
        assert!(!listeners.contains(&e(2)));
    }

    #[test]
    fn hearsay_fuzzifies_exact_quantity_to_around() {
        let speaker = Entity::from_bits(1);
        let source = Triple::new(
            Node::Self_,
            Predicate::Hunger,
            Value::Quantity(Quantity::Exact(60.0)),
        );
        let hearsay = fuzzify_hearsay(&source, 100, speaker);
        assert!(
            matches!(hearsay.object, Value::Quantity(Quantity::Around(v)) if (v - 60.0).abs() < 0.001)
        );
        assert_eq!(
            hearsay.meta.source,
            crate::agent::mind::knowledge::Source::Hearsay
        );
        assert_eq!(hearsay.meta.informant, Some(speaker));
    }

    #[test]
    fn hearsay_chain_compounds_fuzzification() {
        let speaker = Entity::from_bits(1);
        let hop_1 = fuzzify_hearsay(
            &Triple::new(
                Node::Self_,
                Predicate::Hunger,
                Value::Quantity(Quantity::Exact(37.0)),
            ),
            100,
            speaker,
        );
        let hop_2 = fuzzify_hearsay(&hop_1, 200, speaker);
        let hop_3 = fuzzify_hearsay(&hop_2, 300, speaker);
        assert!(matches!(hop_1.object, Value::Quantity(Quantity::Around(_))));
        assert!(matches!(
            hop_2.object,
            Value::Quantity(Quantity::OrderOfMagnitude(_))
        ));
        assert!(matches!(
            hop_3.object,
            Value::Quantity(Quantity::Qualitative(_))
        ));
    }

    #[test]
    fn hearsay_at_qualitative_floor_does_not_drop_triple() {
        let speaker = Entity::from_bits(1);
        let source = Triple::new(
            Node::Self_,
            Predicate::Hunger,
            Value::Quantity(Quantity::Qualitative(Magnitude::High)),
        );
        let hearsay = fuzzify_hearsay(&source, 100, speaker);
        assert!(matches!(
            hearsay.object,
            Value::Quantity(Quantity::Qualitative(Magnitude::High))
        ));
    }

    #[test]
    fn ask_intent_produces_lower_base_valence_than_share() {
        assert!(valence_base(Intent::Ask) < valence_base(Intent::Share));
    }
}
