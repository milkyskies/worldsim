//! Unified Action trait and registry.
//!
//! Actions are self-describing - they define BOTH planning data AND execution behavior.
//! ONE definition serves the planner AND the executor.

use crate::agent::actions::ActionType;
use crate::agent::actions::action::{AttackAction, BiteAction};
use crate::agent::actions::channel::ChannelUsage;
use crate::agent::brains::thinking::{ActionTemplate, TriplePattern};
use crate::agent::events::FailureReason;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, MindGraph, Triple};
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;
use std::collections::HashMap;

// ============================================================================
// ACTION CONTEXT - Data needed for can_start checks
// ============================================================================

/// Context for runtime action checks
pub struct ActionContext<'a> {
    pub inventory: &'a ItemSlots,
    pub mind: &'a MindGraph,
    pub world_map: &'a crate::world::map::WorldMap,
    pub target_entity: Option<Entity>,
    pub target_position: Option<Vec2>,
    pub agent_position: Vec2,
}

// ============================================================================
// TARGET SOURCE - Where does this action's target come from?
// ============================================================================

/// Declares *where* the brain finds candidate targets for this action.
///
/// Replaces the old `TargetType` enum, which only said *what kind* of target
/// existed without saying how to find it — leaving the brain to invent a
/// per-action collector for every variant. With `TargetSource`, the action
/// declares the source and the generic `enumerate_targets` function in
/// `brains::target_enumeration` walks the right knowledge structure.
///
/// Adding a new tile-based action (Fish, Forage, Bathe) is a one-line change
/// to the action: declare `TargetSource::TileWithTrait(Concept::Fishable)`.
/// No new code in the brain or the planner.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetSource {
    /// No target enumeration — the action runs against the agent itself.
    /// Examples: Sleep, Wander, Idle, Eat (uses inventory), Build, Explore, Flee.
    None,
    /// Generated implicitly by the planner, never enumerated up front.
    /// Walk is the only such action — the regressive planner inserts Walk
    /// steps directly when it needs to satisfy a `LocatedAt` precondition.
    Implicit,
    /// Iterate entities the agent has knowledge of and keep the ones whose
    /// world `Affordance` component declares this action type.
    /// Examples: Harvest, Take, Deposit (apple trees, wood logs,
    /// stone nodes, berry bushes, etc).
    EntityAffordance,
    /// Iterate every perceived entity whose ontology trait inheritance contains
    /// the given concept. Used by Attack/Bite to find any entity the agent
    /// knows is `HasTrait Prey` — works regardless of whether the entity has
    /// an inventory or world `Affordance` component, since prey-ness lives
    /// in the agent's beliefs about the entity type, not on the entity itself.
    EntityWithTrait(Concept),
    /// Iterate tiles matching `Tile(?) HasTrait <concept>` in the MindGraph.
    /// The tile-based mirror of `EntityWithTrait`. Drink uses this with
    /// `Concept::Drinkable` so the planner can chain `Walk → Drink` against
    /// any known water tile.
    TileWithTrait(Concept),
}

// ============================================================================
// TARGET CANDIDATE - One concrete target the brain found for this action
// ============================================================================

/// One concrete target produced by `enumerate_targets`. Each variant carries
/// the world position so the planner can compute walk distance without a
/// second lookup.
///
/// This is the unified shape that flows from the brain into
/// `Action::to_template_for_target` — actions then specialize their dynamic
/// preconditions / consumes / effects based on which variant arrived.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TargetCandidate {
    /// No target — the action has nothing to specialize against.
    None,
    /// An entity target. `pos` is the entity's world position at enumeration
    /// time, used to snapshot a tile-based proximity precondition that the
    /// planner can solve via an implicit Walk step.
    Entity { entity: Entity, pos: Vec2 },
    /// A tile target. `pos` is the world centre of the tile.
    Tile { tile: (i32, i32), pos: Vec2 },
}

impl TargetCandidate {
    pub fn as_entity(&self) -> Option<Entity> {
        match self {
            TargetCandidate::Entity { entity, .. } => Some(*entity),
            _ => None,
        }
    }

    /// Tile coordinates of the target, snapshotted at enumeration time. For
    /// `Entity` targets this is the tile the entity occupied when the brain
    /// enumerated it — good enough for static targets like trees and rocks,
    /// stale for moving targets (a known #219 trade-off).
    pub fn tile(&self) -> Option<(i32, i32)> {
        match self {
            TargetCandidate::None => None,
            TargetCandidate::Entity { pos, .. } => Some((
                (pos.x / TILE_SIZE).floor() as i32,
                (pos.y / TILE_SIZE).floor() as i32,
            )),
            TargetCandidate::Tile { tile, .. } => Some(*tile),
        }
    }
}

// ============================================================================
// ACTION KIND - How the action executes
// ============================================================================

/// What kind of action is this?
#[derive(Debug, Clone, PartialEq)]
pub enum ActionKind {
    /// Instant action (check -> do -> done in one tick)
    Instant,
    /// Timed action (countdown ticks, then complete)
    Timed { duration_ticks: u32 },
    /// Movement action (move toward target until arrival)
    Movement,
}

// ============================================================================
// RUNTIME EFFECTS - Per-tick effects only
// ============================================================================

/// Per-tick effects applied while action is running
#[derive(Debug, Clone, Default)]
pub struct RuntimeEffects {
    pub stamina_per_sec: f32,
    pub hunger_per_sec: f32,
    pub alertness_per_sec: f32,
}

// ============================================================================
// COMPLETION CONTEXT - Passed to on_complete for actions to modify
// ============================================================================

/// A request to spawn a world entity at a position when an action completes.
/// Processed by the execution system after `on_complete` returns.
pub enum SpawnRequest {
    /// Spawn a finished entity directly. Used for "instant" spawns that have
    /// no construction phase (e.g. dropping an item, summoning).
    Entity {
        concept: crate::agent::mind::knowledge::Concept,
        position: bevy::prelude::Vec2,
    },
    /// Spawn a construction site that will become `target` when its slots fill
    /// (and optional labor is accumulated).
    /// `requirements` defines the slot configuration; `initial_items` are
    /// deposited into matching slots immediately (used when the agent already
    /// has the materials in hand). `labor_required` adds a `LaborAccumulated`
    /// condition so agents must Construct the site after stocking it.
    Site {
        target: crate::agent::mind::knowledge::Concept,
        position: bevy::prelude::Vec2,
        requirements: Vec<(crate::agent::mind::knowledge::Concept, u32)>,
        initial_items: Vec<(crate::agent::mind::knowledge::Concept, u32)>,
        /// `Some(n)` requires `n` labor ticks via the Construct action before
        /// the site can transform. `None` keeps the original SlotsFilled-only
        /// trigger for backward compatibility.
        labor_required: Option<u32>,
    },
    /// Attach a `Becomes` transformation to an existing world entity. The
    /// transformation fires immediately (`AfterTicks(0)`), so on the next
    /// `becomes_system` tick the substrate executes the requested mode.
    /// `mode` controls whether the substrate despawns and respawns
    /// (`Replace`) or morphs the existing entity in place (`InPlace`).
    /// Used by Attack/Bite to turn slain prey into corpses via in-place
    /// transformation.
    BecomesAttach {
        entity: bevy::prelude::Entity,
        target: crate::agent::mind::knowledge::Concept,
        mode: crate::world::becomes::BecomesMode,
    },
}

/// Context provided to actions when they complete
/// Actions modify this directly - fully declarative!
pub struct CompletionContext<'a> {
    pub physical: &'a mut crate::agent::body::needs::PhysicalNeeds,
    pub inventory: &'a mut crate::agent::item_slots::ItemSlots,
    /// Psychological drives (social, curiosity, etc.)
    pub drives: Option<&'a mut crate::agent::body::needs::PsychologicalDrives>,
    /// The agent's MindGraph (read-only). Lets actions consult the agent's
    /// beliefs about the target — e.g. Attack/Bite check
    /// `has_trait(target, Prey)` to gate the hunt-kill path.
    pub mind: &'a crate::agent::mind::knowledge::MindGraph,
    /// Target entity's inventory (for Harvest, etc.)
    pub target_inventory: Option<&'a mut crate::agent::item_slots::ItemSlots>,
    /// Target entity
    pub target_entity: Option<bevy::prelude::Entity>,
    /// Current tick for timestamping
    pub tick: u64,
    /// Position of the agent executing this action (for Build-style spawning).
    pub agent_position: bevy::prelude::Vec2,
    /// Entities the action wants spawned in the world after completion.
    /// The execution system processes these with `Commands` after `on_complete` returns.
    pub spawn_requests: &'a mut Vec<SpawnRequest>,
}

// ============================================================================
// UNIFIED ACTION TRAIT
// ============================================================================

/// The unified Action trait - serves BOTH planning AND execution.
///
/// Each action defines:
/// - Identity: action_type, name
/// - Planning: preconditions, plan_effects, cost
/// - Execution: kind, can_start, on_fail, runtime_effects
pub trait Action: Send + Sync + 'static {
    // === IDENTITY ===

    /// The action type identifier
    fn action_type(&self) -> ActionType;

    /// Human-readable name
    fn name(&self) -> &'static str;

    // === FOR PLANNING (GOAP) ===

    /// Preconditions as TriplePatterns - what must be true before action
    /// Default: no preconditions
    fn preconditions(&self) -> Vec<TriplePattern> {
        vec![]
    }

    /// Effects as Triples - what becomes true after action completes
    /// Default: no effects
    fn plan_effects(&self) -> Vec<Triple> {
        vec![]
    }

    /// Patterns that this action removes from the world when it executes (destructive effects).
    /// Used by the planner to track resource depletion during backward search.
    /// Default: no consumptions
    fn plan_consumes(&self) -> Vec<TriplePattern> {
        vec![]
    }

    /// Base cost for planning (lower = preferred)
    fn cost(&self) -> f32 {
        1.0
    }

    /// Where does this action find its candidate targets?
    ///
    /// Drives `enumerate_targets` in `brains::target_enumeration`. The default
    /// `TargetSource::None` is right for any action that operates on the agent
    /// itself (Eat, Sleep, Build, ...). Override to point at entity affordances
    /// or tile traits.
    fn target_source(&self) -> TargetSource {
        TargetSource::None
    }

    /// Per-target preconditions, bound to one specific candidate.
    ///
    /// Use this for conditions that depend on *which* target the action is
    /// running against — e.g. Harvest's "the target must contain something"
    /// or Take's "this entity must hold an item." Static preconditions that
    /// don't reference the target belong in `preconditions()`.
    ///
    /// Default: empty.
    fn target_preconditions(
        &self,
        _target: &TargetCandidate,
        _mind: &MindGraph,
    ) -> Vec<TriplePattern> {
        vec![]
    }

    /// Per-target consumed patterns, bound to one specific candidate.
    ///
    /// Use this for resources the action removes from a *specific* target,
    /// so the planner can prevent two plan steps from harvesting the same
    /// stack. Static consumptions belong in `plan_consumes()`.
    ///
    /// Default: empty.
    fn target_consumes(&self, _target: &TargetCandidate, _mind: &MindGraph) -> Vec<TriplePattern> {
        vec![]
    }

    // === FOR EXECUTION ===

    /// What kind of action is this? (instant, timed, movement)
    fn kind(&self) -> ActionKind;

    /// Runtime check - can we actually start this action?
    /// Default: always can start
    fn can_start(&self, _ctx: &ActionContext) -> Result<(), FailureReason> {
        Ok(())
    }

    /// Planning check - is this action valid for this target/context?
    /// Validates if the agent *knows* enough to attempt this.
    /// Takes a `TargetCandidate` so tile-targeted actions can also gate.
    fn is_plan_valid(&self, _target: &TargetCandidate, _mind: &MindGraph) -> bool {
        true
    }

    /// Per-tick effects (stamina drain while moving, etc.)
    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects::default()
    }

    /// Body channels this action occupies, with intensity 0.0..=1.0 each.
    ///
    /// Returns a `'static` slice so the hot tick loop never allocates.
    /// Default: no channels - the action is purely cognitive (Idle, planning).
    fn body_channels(&self) -> &'static [ChannelUsage] {
        &[]
    }

    /// Whether this action can be preempted mid-execution. Default `true`.
    /// Reserved for future actions that should resist casual preemption
    /// regardless of channel saturation (crafting, ritual, surgery).
    fn interruptible(&self) -> bool {
        true
    }

    /// Called when action completes - action applies its own effects!
    /// This is where actions modify physical needs, inventory, etc.
    /// Default: do nothing
    fn on_complete(&self, _ctx: &mut CompletionContext) {
        // Override in actions that have completion effects
    }

    // === LOGGING ===

    /// Log message when action starts
    fn start_log(&self) -> Option<&'static str> {
        None
    }

    /// Log message when action completes
    fn complete_log(&self) -> Option<&'static str> {
        None
    }

    /// Dynamic plan effects for a specific target, derived from MindGraph.
    ///
    /// Override this in actions whose effects depend on the target (e.g. Harvest
    /// yields whatever the target entity actually produces). The default delegates
    /// to `plan_effects()` so most actions need not override.
    fn plan_effects_for_target(&self, _target: &TargetCandidate, _mind: &MindGraph) -> Vec<Triple> {
        self.plan_effects()
    }

    // === CONVERSION ===

    /// Build a planner-ready `ActionTemplate` for an action that needs no
    /// per-target enrichment from the MindGraph.
    ///
    /// Used by the survival brain (Eat, Sleep, Drink, Flee, Wander, Idle)
    /// and the emotional brain (Flee, Attack-on-entity), which propose
    /// concrete actions directly without going through `enumerate_targets`.
    /// Uses only `preconditions()` / `plan_effects()` / `plan_consumes()`.
    ///
    /// **Does not auto-inject a proximity precondition.** That intentionally
    /// only happens via `to_template_for_target`, which has a `MindGraph`
    /// reference and the per-target hooks. Callers that want the rich path
    /// (Harvest yielding what the target produces, Drink with `self_at(tile)`)
    /// must go through `to_template_for_target`.
    fn to_template(&self, target_entity: Option<Entity>) -> ActionTemplate {
        let action_type = self.action_type();
        ActionTemplate {
            name: self.name().to_string(),
            action_type,
            target_entity,
            target_position: None,
            preconditions: self.preconditions(),
            effects: self.plan_effects(),
            consumes: self.plan_consumes(),
            base_cost: self.cost(),
            // Start with the action's default locomotion intensity. The
            // brain may override it based on urgency before admission.
            locomotion_intensity: action_type.default_locomotion_intensity(),
        }
    }

    /// Build a planner-ready `ActionTemplate` for one concrete target,
    /// auto-injecting a proximity precondition so the regressive planner
    /// chains `Walk → action` for any candidate that carries a position.
    ///
    /// Single entry point for the rational brain. Actions that *don't* want
    /// the auto-walk (Walk itself, InitiateConversation) declare
    /// `TargetSource::Implicit` so the brain skips them entirely. Actions
    /// rarely need to override this — declaring `target_source()`,
    /// `target_preconditions()`, and `plan_effects_for_target()` is enough.
    fn to_template_for_target(&self, target: &TargetCandidate, mind: &MindGraph) -> ActionTemplate {
        let mut preconditions = self.preconditions();

        // Auto-inject the proximity precondition. We use tile-based form so
        // the runtime check (`are_preconditions_met`) can verify against the
        // agent's `Self_, LocatedAt, Tile(t)` belief from perception. The
        // tile is snapshotted from the candidate's current position.
        if let Some(tile) = target.tile() {
            preconditions.push(TriplePattern::self_at(tile));
        }

        preconditions.extend(self.target_preconditions(target, mind));

        let mut consumes = self.plan_consumes();
        consumes.extend(self.target_consumes(target, mind));

        let (target_entity, target_position) = match target {
            TargetCandidate::None => (None, None),
            TargetCandidate::Entity { entity, pos } => (Some(*entity), Some(*pos)),
            TargetCandidate::Tile { pos, .. } => (None, Some(*pos)),
        };

        let action_type = self.action_type();
        ActionTemplate {
            name: self.name().to_string(),
            action_type,
            target_entity,
            target_position,
            preconditions,
            effects: self.plan_effects_for_target(target, mind),
            consumes,
            base_cost: self.cost(),
            locomotion_intensity: action_type.default_locomotion_intensity(),
        }
    }
}

// ============================================================================
// ACTION STATE - Runtime state for one active action
// ============================================================================

/// Runtime state for one active action.
///
/// Lives inside [`ActiveActions`] (the ECS component). An agent can have
/// many `ActionState`s running in parallel as long as their body channels
/// don't hard-conflict.
#[derive(Debug, Clone, Default, Reflect)]
pub struct ActionState {
    /// The action type being executed
    pub action_type: ActionType,
    /// Target entity (if any)
    pub target_entity: Option<Entity>,
    /// Target position (if any)
    pub target_position: Option<Vec2>,
    /// When the action started
    pub started_tick: u64,
    /// Ticks remaining for timed actions
    pub ticks_remaining: u32,
    /// Movement state - last tick for delta calculation
    pub last_movement_tick: u64,
    /// Fractional tick progress for degraded actions. When the channel load
    /// degrades a timed action, only a fraction of each tick "counts" -
    /// progress accumulates here and decrements `ticks_remaining` each time
    /// it crosses 1.0. Deterministic and replay-safe.
    pub progress_accumulator: f32,
    /// Desired locomotion intensity in [0, 1] for Movement-class actions (#339).
    /// `0.0` means this action isn't locomotion and the field is unused.
    /// The brain sets this from the action's default plus an urgency boost
    /// (see `ActionType::pick_locomotion_intensity`). The *effective*
    /// intensity used by the body may be lower when stamina is exhausted,
    /// but the desired intensity stored here stays put so the intent stays
    /// clear (e.g. an exhausted Flee is still trying to Flee at 1.0).
    pub locomotion_intensity: f32,
}

impl ActionState {
    pub fn new(action_type: ActionType, tick: u64) -> Self {
        Self {
            action_type,
            started_tick: tick,
            last_movement_tick: tick.saturating_sub(1),
            ticks_remaining: 0,
            progress_accumulator: 0.0,
            target_entity: None,
            target_position: None,
            locomotion_intensity: action_type.default_locomotion_intensity(),
        }
    }

    pub fn with_target_entity(mut self, entity: Entity) -> Self {
        self.target_entity = Some(entity);
        self
    }

    pub fn with_target_position(mut self, pos: Vec2) -> Self {
        self.target_position = Some(pos);
        self
    }

    pub fn with_duration(mut self, ticks: u32) -> Self {
        self.ticks_remaining = ticks;
        self
    }

    pub fn with_locomotion_intensity(mut self, intensity: f32) -> Self {
        self.locomotion_intensity = intensity.clamp(0.0, 1.0);
        self
    }
}

// ============================================================================
// ACTIVE ACTIONS - Component holding the parallel set of running actions
// ============================================================================

/// All actions currently running on an agent.
///
/// Replaces the single-slot model. Multiple actions can coexist as long as
/// their [`ChannelUsage`] doesn't hard-conflict. The container preserves
/// uniqueness by [`ActionType`] - starting an action that's already running
/// updates that slot in place.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct ActiveActions {
    #[reflect(ignore)]
    running: Vec<ActionState>,
}

impl Default for ActiveActions {
    fn default() -> Self {
        // Every agent starts idle - this preserves the previous behavior where
        // the default `ActionState` had `ActionType::Idle`.
        Self {
            running: vec![ActionState::default()],
        }
    }
}

impl ActiveActions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Empty container - useful for tests and entities that don't auto-idle.
    pub fn empty() -> Self {
        Self {
            running: Vec::new(),
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, ActionState> {
        self.running.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, ActionState> {
        self.running.iter_mut()
    }

    pub fn len(&self) -> usize {
        self.running.len()
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    /// Find a running action by its type.
    pub fn get(&self, action_type: ActionType) -> Option<&ActionState> {
        self.running.iter().find(|a| a.action_type == action_type)
    }

    /// Find a running action by its type for mutation.
    pub fn get_mut(&mut self, action_type: ActionType) -> Option<&mut ActionState> {
        self.running
            .iter_mut()
            .find(|a| a.action_type == action_type)
    }

    pub fn contains(&self, action_type: ActionType) -> bool {
        self.get(action_type).is_some()
    }

    /// Insert a new action, replacing any existing entry with the same type.
    pub fn insert(&mut self, action: ActionState) {
        if let Some(slot) = self.get_mut(action.action_type) {
            *slot = action;
        } else {
            self.running.push(action);
        }
    }

    /// Remove an action by type, returning the removed state if present.
    pub fn remove(&mut self, action_type: ActionType) -> Option<ActionState> {
        let idx = self
            .running
            .iter()
            .position(|a| a.action_type == action_type)?;
        Some(self.running.remove(idx))
    }

    /// Clear all running actions and reset to a single Idle slot.
    pub fn reset_to_idle(&mut self, tick: u64) {
        self.running.clear();
        self.running.push(ActionState::new(ActionType::Idle, tick));
    }

    /// Drop all actions, leaving the container empty.
    pub fn clear(&mut self) {
        self.running.clear();
    }

    /// "Primary" action - the most demanding currently running action by total
    /// channel intensity. Falls back to the first slot. Used by legacy callers
    /// (UI, perception) that need a single `ActionState`.
    pub fn primary<'a>(&'a self, registry: &ActionRegistry) -> Option<&'a ActionState> {
        // Cache total intensity once per action so the comparator is O(n) total,
        // not O(n log n) registry hits.
        let mut scored: Vec<(f32, &ActionState)> = self
            .running
            .iter()
            .map(|s| {
                let intensity: f32 = registry
                    .get(s.action_type)
                    .map(|d| d.body_channels().iter().map(|c| c.intensity).sum())
                    .unwrap_or(0.0);
                (intensity, s)
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.first().map(|(_, s)| *s)
    }

    /// Build the [`crate::agent::actions::channel::ChannelLoad`] aggregate over
    /// all currently running actions.
    pub fn channel_load(
        &self,
        registry: &ActionRegistry,
    ) -> crate::agent::actions::channel::ChannelLoad {
        let mut load = crate::agent::actions::channel::ChannelLoad::new();
        for action in &self.running {
            if let Some(def) = registry.get(action.action_type) {
                load.add(def.body_channels());
            }
        }
        load
    }
}

// ============================================================================
// REGISTRY - Stores all actions, serves both planning and execution
// ============================================================================

use super::action::{
    BuildAction, ConstructAction, ConverseAction, DepositAction, DrinkAction, EatAction,
    ExploreAction, FleeAction, GrazeAction, HarvestAction, IdleAction, InitiateConversationAction,
    SleepAction, TakeAction, WakeUpAction, WalkAction, WanderAction,
};

#[derive(Resource, Default)]
pub struct ActionRegistry {
    actions: HashMap<ActionType, Box<dyn Action>>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        let mut registry = Self::default();
        // Register all actions from action/ directory
        registry.register(IdleAction);
        registry.register(SleepAction);
        registry.register(WakeUpAction);
        registry.register(EatAction);
        registry.register(DrinkAction);
        registry.register(GrazeAction);
        registry.register(WalkAction);
        registry.register(FleeAction);
        registry.register(ExploreAction);
        registry.register(AttackAction);
        registry.register(BiteAction);
        registry.register(HarvestAction);
        registry.register(BuildAction);
        registry.register(ConstructAction);
        registry.register(DepositAction);
        registry.register(TakeAction);
        registry.register(WanderAction);
        // Conversation actions — owned by the CommunicationPlugin.
        registry.register(InitiateConversationAction);
        registry.register(ConverseAction);
        registry
    }

    pub fn register<A: Action>(&mut self, action: A) {
        self.actions.insert(action.action_type(), Box::new(action));
    }

    pub fn get(&self, action_type: ActionType) -> Option<&dyn Action> {
        self.actions.get(&action_type).map(|a| a.as_ref())
    }

    /// Get all registered actions (for planner to iterate)
    pub fn all(&self) -> impl Iterator<Item = &dyn Action> {
        self.actions.values().map(|a| a.as_ref())
    }

    /// Find actions whose effects could satisfy a goal pattern
    pub fn actions_satisfying(&self, predicate: impl Fn(&Triple) -> bool) -> Vec<&dyn Action> {
        self.actions
            .values()
            .filter(|a| a.plan_effects().iter().any(&predicate))
            .map(|a| a.as_ref())
            .collect()
    }
}
