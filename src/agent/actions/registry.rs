//! Unified Action trait and registry.
//!
//! Actions are self-describing - they define BOTH planning data AND execution behavior.
//! ONE definition serves the planner AND the executor.

use crate::agent::actions::ActionType;
use crate::agent::actions::action::AttackAction;
use crate::agent::actions::channel::ChannelUsage;
use crate::agent::brains::thinking::{ActionTemplate, TriplePattern};
use crate::agent::events::FailureReason;
use crate::agent::inventory::Inventory;
use crate::agent::mind::knowledge::{MindGraph, Triple};
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;
use std::collections::HashMap;

// ============================================================================
// ACTION CONTEXT - Data needed for can_start checks
// ============================================================================

/// Context for runtime action checks
pub struct ActionContext<'a> {
    pub inventory: &'a Inventory,
    pub mind: &'a MindGraph,
    pub target_entity: Option<Entity>,
    pub target_position: Option<Vec2>,
    pub agent_position: Vec2,
}

// ============================================================================
// TARGET TYPE - What kind of target does this action need?
// ============================================================================

/// What kind of target does this action require for planning?
#[derive(Debug, Clone, PartialEq)]
pub enum TargetType {
    /// No target needed (Sleep, Eat, Idle, Wander)
    None,
    /// Needs a target entity (Harvest from tree, Attack enemy)
    Entity,
    /// Needs a target position (Walk to location)
    Position,
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
    pub energy_per_sec: f32,
    pub hunger_per_sec: f32,
    pub alertness_per_sec: f32,
}

// ============================================================================
// COMPLETION CONTEXT - Passed to on_complete for actions to modify
// ============================================================================

/// Context provided to actions when they complete
/// Actions modify this directly - fully declarative!
pub struct CompletionContext<'a> {
    pub physical: &'a mut crate::agent::body::needs::PhysicalNeeds,
    pub inventory: &'a mut crate::agent::inventory::Inventory,
    /// Psychological drives (social, curiosity, etc.)
    pub drives: Option<&'a mut crate::agent::body::needs::PsychologicalDrives>,
    /// Target entity's inventory (for Harvest, etc.)
    pub target_inventory: Option<&'a mut crate::agent::inventory::Inventory>,
    /// Conversation manager for social actions
    pub conversation_manager: Option<&'a mut crate::agent::mind::conversation::ConversationManager>,
    /// Topic for conversation actions
    pub topic: Option<crate::agent::mind::conversation::Topic>,
    /// Target entity
    pub target_entity: Option<bevy::prelude::Entity>,
    /// The actor performing the action
    pub actor: bevy::prelude::Entity,
    /// Content logic shared for conversation actions
    pub content: Vec<crate::agent::mind::knowledge::Triple>,
    /// Current tick for timestamping
    pub tick: u64,
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

    /// Base cost for planning (lower = preferred)
    fn cost(&self) -> f32 {
        1.0
    }

    /// What kind of target does this action need?
    /// This makes it explicit and self-documenting.
    fn target_type(&self) -> TargetType {
        TargetType::None // Most actions don't need targets
    }

    /// Does this action require being near the target?
    /// If true, the default to_template will add a location precondition
    /// causing the planner to generate a Walk step first.
    fn requires_proximity(&self) -> bool {
        false // Most actions don't require proximity
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
    /// Default: always valid
    fn is_plan_valid(
        &self,
        _target: Option<bevy::prelude::Entity>,
        _mind: &crate::agent::mind::knowledge::MindGraph,
    ) -> bool {
        true
    }

    /// Per-tick effects (energy drain while moving, etc.)
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

    /// Whether this action can be preempted mid-execution. Some actions
    /// (Sleep) resist arbitrary interruption; others (Walk, Idle) yield freely.
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

    // === CONVERSION ===

    /// Generate an ActionTemplate from this Action for the planner
    /// Automatically adds location precondition if requires_proximity() is true
    fn to_template(
        &self,
        target_entity: Option<Entity>,
        target_position: Option<Vec2>,
    ) -> ActionTemplate {
        let mut preconditions = self.preconditions();

        // Automatically add location precondition for proximity-requiring actions
        if self.requires_proximity()
            && let Some(pos) = target_position
        {
            let tile = (
                (pos.x / TILE_SIZE).floor() as i32,
                (pos.y / TILE_SIZE).floor() as i32,
            );
            preconditions.push(TriplePattern::self_at(tile));
        }

        ActionTemplate {
            name: self.name().to_string(),
            action_type: self.action_type(),
            target_entity,
            target_position,
            topic: None,
            content: Vec::new(),
            preconditions,
            effects: self.plan_effects(),
            base_cost: self.cost(),
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
    /// Topic for conversation actions
    pub topic: Option<crate::agent::mind::conversation::Topic>,
    /// Content for conversation actions
    pub content: Vec<crate::agent::mind::knowledge::Triple>,
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
            topic: None,
            content: Vec::new(),
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
    EatAction, ExploreAction, FleeAction, HarvestAction, IdleAction, IntroduceAction, SleepAction,
    TalkAction, WakeUpAction, WalkAction, WanderAction,
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
        registry.register(WalkAction);
        registry.register(FleeAction);
        registry.register(ExploreAction);
        registry.register(AttackAction);
        registry.register(HarvestAction);
        registry.register(WanderAction);
        // Social actions
        registry.register(IntroduceAction);
        registry.register(TalkAction);
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
