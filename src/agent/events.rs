//! Agent event types: GameEvent, ActionOutcomeEvent, and SimEvent — the shared message bus for agent interactions.
//!
//! Reads: ActionType, Concept (item types), Triple (knowledge content)
//! Writes: GameEvent (Interaction, SocialInteraction, KnowledgeShared), ActionOutcomeEvent (Success/Failed), SimEvent (unified observability bus)
//! Upstream: action execution systems (emit outcomes), conversation system (emits KnowledgeShared)
//! Downstream: belief_updater (consumes ActionOutcomeEvent), relationship systems (consume SocialInteraction), SimEvent consumers (#84, #123, #124, #125)

use super::actions::ActionType;
use super::brains::proposal::{BrainPowers, BrainProposal, BrainType};
use super::engagement::{EngagementEndReason, EngagementId, EngagementKind};
use super::nervous_system::urgency::Urgency;
use super::psyche::emotions::EmotionType;
use crate::agent::engagement::converse::{Intent as ConverseIntent, Topic as ConverseTopic};
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

/// Per-kind payload for [`SimEventKind::EngagementBeat`]. Each kind owns
/// its own variant with the data observers want from a single beat.
#[derive(Debug, Clone, Reflect, serde::Serialize)]
pub enum EngagementBeatPayload {
    Converse {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        speaker: Entity,
        intent: ConverseIntent,
        #[serde(skip)]
        topic: ConverseTopic,
        content_count: usize,
        expects_response: bool,
    },
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
    /// How much stamina rose (positive = stamina went up).
    pub stamina_gained: f32,
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
#[derive(Debug, Clone, Reflect, PartialEq, serde::Serialize)]
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
    /// The need this action would satisfy is already close enough to full.
    /// Emitted by the unified satiation gate in `Action::satiation` — e.g.
    /// Eat blocks at stomach ≥ 80%, Drink at hydration ≥ 95%, Sleep at
    /// wakefulness ≥ 95%. Carries the current fullness fraction (0..1)
    /// so UI / diagnostics can surface why the action refused.
    AlreadySatiated {
        kind: crate::agent::body::need::NeedKind,
        fullness: f32,
    },
    /// Agent is too far from target
    TooFar,
    /// Interrupted by something else
    Interrupted,
    /// A Walk could not reach its target tile: a straight-line step
    /// crossed a non-walkable tile. Carries the target tile so the belief
    /// updater can mark it Unreachable and the planner can avoid
    /// re-picking it on the next replan.
    PathBlocked { target_tile: (i32, i32) },
    /// Already did this (e.g., already introduced)
    AlreadyDone,
    /// No drinkable water adjacent to agent
    NoWaterNearby,
    /// Agent lacks required crafting or building materials
    MissingMaterials,
    /// The partner's conversation group is already full (capacity reached)
    /// or the partner is otherwise unavailable to join/add to a conversation.
    ConversationFull,
}

/// Event for communicating action outcomes to belief update system
#[derive(Event, Message, Debug, Clone, Reflect)]
pub struct ActionOutcomeEvent {
    pub actor: Entity,
    pub outcome: ActionOutcome,
}

/// Unified event bus capturing every meaningful simulation state change.
///
/// `tick` and `agents` live at the top level so `involves(e)` and tick
/// filtering are plain field access — no dispatch. `kind` holds the
/// variant-specific payload; `kind.as_ref()` (from `strum::AsRefStr`)
/// gives the variant name for JSON type tags and logging. Bevy events
/// are free if unread — zero performance impact without consumers.
#[derive(Event, Message, Debug, Clone)]
pub struct SimEvent {
    pub tick: u64,
    pub agents: Vec<Entity>,
    pub kind: SimEventKind,
}

impl SimEvent {
    pub fn new(tick: u64, agents: Vec<Entity>, kind: SimEventKind) -> Self {
        Self { tick, agents, kind }
    }

    pub fn single(tick: u64, agent: Entity, kind: SimEventKind) -> Self {
        Self {
            tick,
            agents: vec![agent],
            kind,
        }
    }

    pub fn pair(tick: u64, a: Entity, b: Entity, kind: SimEventKind) -> Self {
        Self {
            tick,
            agents: vec![a, b],
            kind,
        }
    }

    /// Helper for the three sites that report a plan lifecycle drop —
    /// verify pass, retain sweep, and the initiate-conversation stall
    /// path. Consolidates the boilerplate so every `PlanAbandoned`
    /// emission reports the same shape.
    pub fn plan_abandoned(
        tick: u64,
        agent: Entity,
        plan_id: crate::agent::brains::plan_memory::PlanId,
        driving_urgency: crate::agent::nervous_system::urgency::UrgencySource,
        reason: crate::agent::brains::plan_memory::PlanAbandonReason,
    ) -> Self {
        Self::single(
            tick,
            agent,
            SimEventKind::PlanAbandoned {
                agent,
                plan_id: plan_id.0,
                driving_urgency,
                reason,
            },
        )
    }

    pub fn involves(&self, entity: Entity) -> bool {
        self.agents.contains(&entity)
    }
}

#[derive(Debug, Clone, strum::AsRefStr, serde::Serialize)]
pub enum SimEventKind {
    /// A brain decision was made: the arbitration system selected actions.
    Decision {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        winner: Option<BrainType>,
        chosen_actions: Vec<ActionType>,
        powers: BrainPowers,

        #[serde(serialize_with = "crate::core::entity_serde::serialize_brain_proposals")]
        proposals: Arc<Vec<BrainProposal>>,
        /// Per-drive urgency values at the moment of the decision.
        urgencies: Vec<Urgency>,
    },

    /// An action was admitted into the running set.
    ActionStarted {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        action: ActionType,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity_opt")]
        target: Option<Entity>,
        /// Plan id from PlanMemory if this action was driven by the rational brain.
        plan_id: Option<u64>,
        /// Step index within the plan.
        plan_step: Option<usize>,
    },

    /// An action completed normally.
    ActionCompleted {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        action: ActionType,
        /// The entity this action was running against, if any. Carried
        /// here so downstream systems (combat resolution, perception
        /// reactions) can find the target after `ActiveActions` has
        /// already dropped the completed action state.
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity_opt")]
        target: Option<Entity>,
    },

    /// An action was preempted to make room for a higher-priority action.
    ActionPreempted {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        preempted_action: ActionType,
    },

    /// An action failed its can_start check.
    ActionFailed {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        action: ActionType,
        reason: FailureReason,
    },

    /// A plan in `PlanMemory` was removed before completion. The reason
    /// distinguishes urgency-based drops (stale sweep) from runtime
    /// invalidation (preconditions broke, step action failed). Emitted
    /// once per removal so tooling can correlate plan lifecycle with
    /// driving drives without probing internal state.
    PlanAbandoned {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        plan_id: u64,
        driving_urgency: crate::agent::nervous_system::urgency::UrgencySource,
        reason: crate::agent::brains::plan_memory::PlanAbandonReason,
    },

    /// An engagement was started between participants. Generic over
    /// kind — the first kind is `EngagementKind::Converse`; future
    /// kinds (Hunt, Tend, Court, …) reuse the same variant.
    EngagementStarted {
        kind: EngagementKind,
        engagement_id: EngagementId,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity_vec")]
        participants: Vec<Entity>,
    },

    /// An engagement ended. Reason distinguishes natural close,
    /// staleness, OOR, abandonment, and emotion-driven break.
    EngagementEnded {
        kind: EngagementKind,
        engagement_id: EngagementId,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity_vec")]
        participants: Vec<Entity>,
        reason: EngagementEndReason,
    },

    /// A new agent joined an existing engagement as an additional
    /// participant.
    EngagementJoined {
        kind: EngagementKind,
        engagement_id: EngagementId,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        joiner: Entity,
    },

    /// One inner-loop beat of an engagement (one conversation turn,
    /// one hunt strike, etc). Payload is kind-specific.
    EngagementBeat {
        kind: EngagementKind,
        engagement_id: EngagementId,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        payload: EngagementBeatPayload,
    },

    /// A relationship dimension changed between two agents.
    RelationshipChanged {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        other: Entity,
        dimension: RelationshipDimension,
        old_value: f32,
        new_value: f32,
    },

    /// An emotion was triggered or reinforced.
    EmotionTriggered {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        emotion: EmotionType,
        intensity: f32,
    },

    /// An agent died.
    Death {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        cause: String,
    },

    /// An agent perceived a new entity (wasn't visible last tick).
    EntityPerceived {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        target: Entity,
    },

    /// An agent recognized a stranger (first encounter).
    StrangerDetected {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        stranger: Entity,
    },

    /// Two agents acknowledged each other in passing (wave, nod, brief
    /// greeting). Not a conversation — no turns, no state machine. Just a
    /// social signal that bumps companionship and relationship warmth.
    SocialAcknowledgment {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        actor: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        target: Entity,
    },

    /// Knowledge was shared between agents.
    KnowledgeShared {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        speaker: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        listener: Entity,
        triple_count: usize,
    },

    /// An agent contributed one labor-tick to a construction site.
    /// Emitted once per active constructor per simulation tick by
    /// `labor_accumulation_system`.
    LaborContributed {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        site: Entity,
    },

    /// An agent felt warmth from a heat source (temperature sense).
    WarmthPerceived {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        source: Entity,
    },

    /// An agent's thermal comfort crossed a named threshold (comfort /
    /// urgent / critical). Emitted by the warmth drain/recovery system so
    /// decision traces and tooling can see the warmth-drive pipeline fire.
    WarmthChanged {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        /// Warmth satisfaction before the change (0..1, high = comfortable).
        old_value: f32,
        /// Warmth satisfaction after the change (0..1).
        new_value: f32,
    },

    /// An agent's rest-quality crossed a named threshold (comfort / urgent
    /// / critical). Emitted by the rest-quality drain/recovery system.
    RestQualityChanged {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        /// Rest-quality satisfaction before the change (0..1, high = rested).
        old_value: f32,
        /// Rest-quality satisfaction after the change (0..1).
        new_value: f32,
    },

    /// An agent's food-security crossed a named threshold. Emitted by the
    /// food-security drain/recovery system.
    FoodSecurityChanged {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        /// Food-security satisfaction before the change (0..1, high = secure).
        old_value: f32,
        /// Food-security satisfaction after the change (0..1).
        new_value: f32,
    },

    /// An agent heard a sound (hearing sense).
    SoundPerceived {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        source: Entity,
        kind: crate::world::sense_sources::SoundKind,
    },

    /// An agent's theory of mind was updated — they changed their belief
    /// about what another agent knows.
    TheoryOfMindUpdated {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        about: Entity,
        /// How the belief was formed
        source: TheoryOfMindSource,
        /// Number of beliefs updated in this batch
        belief_count: usize,
    },

    /// A perishable item's freshness hit zero and its concept transitioned
    /// to its rotten variant (e.g. Apple → RottenApple). Fires once per item
    /// per spoilage — not every tick while rotten.
    ///
    /// `agent` is the entity holding the item (agent inventory, chest, etc.);
    /// not necessarily a thinking agent.
    ItemSpoiled {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        from: Concept,
        to: Concept,
    },

    /// A sapling finished growing and was replaced in place by its mature
    /// plant entity (e.g. `Sapling -> AppleTree`). Emitted once at the
    /// transition; `mature` is the freshly spawned mature entity, since the
    /// sapling itself has already been despawned.
    PlantMatured {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        mature: Entity,
        matured_into: Concept,
    },

    /// An environmental effect (aura, zone, emitter) was applied to an agent.
    /// Emitted once per agent per emitter per tick when the agent is in range.
    EffectApplied {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        /// The entity that emitted the effect (campfire, hostile zone, etc.)
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        source: Entity,
    },

    /// An agent's proficiency in a skill changed — practice, mentorship,
    /// or disuse decay. Fired once per meaningful delta.
    SkillChanged {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        skill: crate::agent::skills::SkillKind,
        old_value: f32,
        new_value: f32,
    },

    /// An attacker landed a blow on a defender. Carries the struck part
    /// kind, damage magnitude, and the applied injury type so the JSONL
    /// log and debug tooling can reconstruct the fight blow by blow.
    CombatHit {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        attacker: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        defender: Entity,
        part_kind: crate::agent::biology::body::BodyNodeKind,
        damage: f32,
        injury_type: crate::agent::biology::body::InjuryType,
    },

    /// A dodge roll succeeded — the defender evaded the attacker's swing.
    /// Feeds the event log without forcing ActionFailed semantics on the
    /// action (the Attack itself still "completed", the hit just didn't).
    CombatMissed {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        attacker: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        defender: Entity,
    },

    /// A body part was severed — its HP hit zero and it was non-vital, so
    /// it fell off the owner and spawned a `SeveredPart` world entity.
    /// Covers limbs, jaws, ears, mouths — anything non-vital.
    PartSevered {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        entity: Entity,
        part_kind: crate::agent::biology::body::BodyNodeKind,
    },

    /// `pick_flee_target` exhausted every escape candidate and the agent
    /// has no walkable retreat path. The threat-appraisal function reads
    /// the resulting `Cornered` component to drop the Fight threshold.
    Cornered {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
    },

    /// Lame status toggled (gained or lost) on an agent. Driven by leg
    /// `BodyNode` HP fractions crossing the lameness threshold.
    LamenessChanged {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        lame: bool,
    },

    /// Agent took heavy head damage and is dazed for `duration_ticks`,
    /// skipping action proposals until they recover.
    Dazed {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        duration_ticks: u32,
    },

    /// An agent witnessed combat involving someone else. Used by the
    /// witness-fear pipeline so future tooling can correlate observed
    /// violence with later behavioral shifts.
    WitnessedCombat {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        observer: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        attacker: Entity,
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        defender: Entity,
    },

    /// A genome was expressed into a phenotype at spawn.
    ///
    /// Emitted once per agent by `develop_phenotype_system` immediately after
    /// the genome is added. Carries the physical multipliers for quick
    /// inspection without querying the Phenotype component.
    PhenotypeDeveloped {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        phenotype: crate::agent::body::genetics::phenotype::Phenotype,
    },

    /// The GOAP regressive planner ran a search for an agent. Carries search
    /// telemetry: iteration count, whether the search exhausted its budget,
    /// and the patterns that remained unsatisfied (if any).
    GoapSearchTelemetry {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        /// Debug-formatted goal conditions.
        goal_description: String,
        iterations: usize,
        /// True when MAX_ITERATIONS was hit before a solution was found.
        exhausted: bool,
        /// Debug-formatted patterns that were still unmet when the search ended.
        best_unmet_goals: Vec<String>,
    },

    /// A GOAP plan was generated and inserted into PlanMemory.
    PlanGenerated {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        plan_id: u64,
        driving_urgency: crate::agent::nervous_system::urgency::UrgencySource,
        step_count: usize,
        subjective_cost: f32,
        /// Debug-formatted goal conditions.
        goal_description: String,
    },

    /// An action candidate was included (or considered) during target enumeration.
    /// Emitted once per surviving (action, target) pair from `collect_planning_actions`.
    TargetEnumerated {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        action_name: String,
        /// Debug-formatted target (entity id, tile coords, or "None").
        target_description: String,
        /// Why this candidate was kept: "is_plan_valid" or "belief_confidence:<f32>".
        inclusion_reason: String,
    },

    /// The GOAP planner could not find a plan for a goal. Carries the patterns
    /// that couldn't be satisfied, enabling diagnosis of "why can't I eat?"
    /// style questions.
    PatternRejected {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        /// Debug-formatted goal conditions that the planner was trying to satisfy.
        goal_description: String,
        /// Debug-formatted patterns that remained unmet after the full search.
        unmet_patterns: Vec<String>,
    },

    /// A triple was added to or removed from an agent's MindGraph.
    /// Emitted in bulk by `drain_mindgraph_mutations` — one event per mutation.
    MindGraphMutation {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        /// "Add" or "Remove".
        op: String,
        /// Debug-formatted subject node.
        subject: String,
        /// Debug-formatted predicate.
        predicate: String,
        /// Debug-formatted object value.
        object: String,
    },

    /// Per-tick hash of an agent's observable state. Comparing hashes across
    /// two runs with different seeds pinpoints the exact tick of divergence.
    AgentStateHash {
        #[serde(serialize_with = "crate::core::entity_serde::serialize_entity")]
        agent: Entity,
        /// FxHash of (position_tile_x, position_tile_y, urgency_sources_sorted, plan_ids_sorted).
        hash: u64,
    },
}

/// Which relationship dimension changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, serde::Serialize)]
pub enum RelationshipDimension {
    Trust,
    Affection,
}

/// How a theory of mind belief was formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, serde::Serialize)]
pub enum TheoryOfMindSource {
    /// "I told them this" — speaker recording what they shared
    Communicated,
    /// "They told me this, so they know it" — listener recording speaker's knowledge
    Received,
    /// "We both saw this" — shared experience from co-location
    SharedExperience,
}
