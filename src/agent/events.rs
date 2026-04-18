//! Agent event types: GameEvent, ActionOutcomeEvent, and SimEvent — the shared message bus for agent interactions.
//!
//! Reads: ActionType, Concept (item types), Triple (knowledge content)
//! Writes: GameEvent (Interaction, SocialInteraction, KnowledgeShared), ActionOutcomeEvent (Success/Failed), SimEvent (unified observability bus)
//! Upstream: action execution systems (emit outcomes), conversation system (emits KnowledgeShared)
//! Downstream: belief_updater (consumes ActionOutcomeEvent), relationship systems (consume SocialInteraction), SimEvent consumers (#84, #123, #124, #125)

use super::actions::ActionType;
use super::brains::proposal::{BrainPowers, BrainProposal, BrainType, Intent};
use super::nervous_system::urgency::Urgency;
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
        /// Per-drive urgency values at the moment of the decision.
        urgencies: Vec<Urgency>,
    },

    /// An action was admitted into the running set.
    ActionStarted {
        agent: Entity,
        tick: u64,
        action: ActionType,
        target: Option<Entity>,
        /// Plan id from PlanMemory if this action was driven by the rational brain.
        plan_id: Option<u64>,
        /// Step index within the plan.
        plan_step: Option<usize>,
    },

    /// An action completed normally.
    ActionCompleted {
        agent: Entity,
        tick: u64,
        action: ActionType,
        /// The entity this action was running against, if any. Carried
        /// here so downstream systems (combat resolution, perception
        /// reactions) can find the target after `ActiveActions` has
        /// already dropped the completed action state.
        target: Option<Entity>,
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

    /// A new agent joined an existing conversation as an additional
    /// participant (group grew from N to N+1).
    ConversationJoined {
        joiner: Entity,
        tick: u64,
        conversation_id: u64,
    },

    /// A single agent left a multi-agent conversation gracefully while
    /// the rest kept talking. Distinct from `ConversationEnded` (whole
    /// group broke up) and `ConversationAbandoned` (leaver ditched rudely).
    ConversationLeft {
        leaver: Entity,
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

    /// Two agents acknowledged each other in passing (wave, nod, brief
    /// greeting). Not a conversation — no turns, no state machine. Just a
    /// social signal that bumps companionship and relationship warmth.
    SocialAcknowledgment {
        actor: Entity,
        target: Entity,
        tick: u64,
    },

    /// Knowledge was shared between agents.
    KnowledgeShared {
        speaker: Entity,
        listener: Entity,
        tick: u64,
        triple_count: usize,
    },

    /// An agent contributed one labor-tick to a construction site.
    /// Emitted once per active constructor per simulation tick by
    /// `labor_accumulation_system`.
    LaborContributed {
        agent: Entity,
        tick: u64,
        site: Entity,
    },

    /// An agent felt warmth from a heat source (temperature sense).
    WarmthPerceived {
        agent: Entity,
        tick: u64,
        source: Entity,
    },

    /// An agent heard a sound (hearing sense).
    SoundPerceived {
        agent: Entity,
        tick: u64,
        source: Entity,
        kind: crate::world::sense_sources::SoundKind,
    },

    /// An agent's theory of mind was updated — they changed their belief
    /// about what another agent knows.
    TheoryOfMindUpdated {
        agent: Entity,
        about: Entity,
        tick: u64,
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
        agent: Entity,
        tick: u64,
        from: Concept,
        to: Concept,
    },

    /// An environmental effect (aura, zone, emitter) was applied to an agent.
    /// Emitted once per agent per emitter per tick when the agent is in range.
    EffectApplied {
        agent: Entity,
        tick: u64,
        /// The entity that emitted the effect (campfire, hostile zone, etc.)
        source: Entity,
    },

    /// An agent's proficiency in a skill changed — practice, mentorship,
    /// or disuse decay. Fired once per meaningful delta.
    SkillChanged {
        agent: Entity,
        tick: u64,
        skill: crate::agent::skills::SkillKind,
        old_value: f32,
        new_value: f32,
    },

    /// An attacker landed a blow on a defender. Carries the struck part
    /// kind, damage magnitude, and the applied injury type so the JSONL
    /// log and debug tooling can reconstruct the fight blow by blow.
    CombatHit {
        attacker: Entity,
        defender: Entity,
        tick: u64,
        part_kind: crate::agent::biology::body::BodyNodeKind,
        damage: f32,
        injury_type: crate::agent::biology::body::InjuryType,
    },

    /// A dodge roll succeeded — the defender evaded the attacker's swing.
    /// Feeds the event log without forcing ActionFailed semantics on the
    /// action (the Attack itself still "completed", the hit just didn't).
    CombatMissed {
        attacker: Entity,
        defender: Entity,
        tick: u64,
    },

    /// A body part was severed — its HP hit zero and it was non-vital, so
    /// it fell off the owner and spawned a `SeveredPart` world entity.
    /// Covers limbs, jaws, ears, mouths — anything non-vital.
    PartSevered {
        entity: Entity,
        tick: u64,
        part_kind: crate::agent::biology::body::BodyNodeKind,
    },

    /// A genome was expressed into a phenotype at spawn.
    ///
    /// Emitted once per agent by `develop_phenotype_system` immediately after
    /// the genome is added. Carries the physical multipliers for quick
    /// inspection without querying the Phenotype component.
    PhenotypeDeveloped {
        agent: Entity,
        tick: u64,
        phenotype: crate::agent::body::genetics::phenotype::Phenotype,
    },

    /// The GOAP regressive planner ran a search for an agent. Carries search
    /// telemetry: iteration count, whether the search exhausted its budget,
    /// and the patterns that remained unsatisfied (if any).
    GoapSearchTelemetry {
        agent: Entity,
        tick: u64,
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
        agent: Entity,
        tick: u64,
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
        agent: Entity,
        tick: u64,
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
        agent: Entity,
        tick: u64,
        /// Debug-formatted goal conditions that the planner was trying to satisfy.
        goal_description: String,
        /// Debug-formatted patterns that remained unmet after the full search.
        unmet_patterns: Vec<String>,
    },

    /// A triple was added to or removed from an agent's MindGraph.
    /// Emitted in bulk by `drain_mindgraph_mutations` — one event per mutation.
    MindGraphMutation {
        agent: Entity,
        tick: u64,
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
        agent: Entity,
        tick: u64,
        /// FxHash of (position_tile_x, position_tile_y, urgency_sources_sorted, plan_ids_sorted).
        hash: u64,
    },
}

/// Which relationship dimension changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum RelationshipDimension {
    Trust,
    Affection,
}

/// How a theory of mind belief was formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum TheoryOfMindSource {
    /// "I told them this" — speaker recording what they shared
    Communicated,
    /// "They told me this, so they know it" — listener recording speaker's knowledge
    Received,
    /// "We both saw this" — shared experience from co-location
    SharedExperience,
}
