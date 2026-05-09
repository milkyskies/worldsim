//! Declarative action definitions.
//!
//! An [`ActionDefinition`] is the single source of truth for an action's
//! identity, body channels, planning rules, runtime gates, and completion
//! effects. Expressed as static data (plus a small [`Hooks`] struct of
//! function pointers for cases that don't fit the enum variants), it's
//! interpreted by [`GenericAction`](super::generic_action::GenericAction)
//! — one interpreter for every action, not a per-action trait impl.
//!
//! Core pattern: every `src/agent/actions/action/*.rs` exports a
//! `pub static FOO_DEF: ActionDefinition = ActionDefinition { ... };`
//! and the registry wraps each def in [`GenericAction`]. Adding a new
//! action is a struct literal plus (at most) a couple of named helper
//! functions when the declarative machinery doesn't cover the logic.

use super::ActionType;
use super::channel::{ChannelUsage, Posture};
use super::motor::{ActionPrimitive, IntensityPolicy, Intent, TargetSelector};
use super::registry::{
    ActionContext, ActionKind, CompletionContext, LegCompleteContext, LegResult, TargetCandidate,
    TargetSource,
};
use crate::agent::body::need::NeedKind;
use crate::agent::body::needs::{PhysicalNeeds, PsychologicalDrives};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, MindGraph, Predicate, Triple};
use crate::world::field_grid_plugin::FieldGrids;
use bevy::math::{IVec2, Vec2};
use bevy::prelude::Entity;

// ============================================================================
// TRIPLE PATTERN TEMPLATES
// ============================================================================

/// Static template for a planner-visible triple pattern.
///
/// Compiled to a `TriplePattern` by [`GenericAction`] whenever the planner
/// asks for preconditions or plan_consumes. Declaring precondition shape as
/// data rather than code keeps the `(Self, Contains, Item(Wood, 3))` triple
/// written exactly once, no matter how many code paths read it (planner,
/// runtime gate, culture recipe derivation).
#[derive(Debug, Clone, Copy)]
pub enum Pattern {
    /// `(Self_, Contains, Item(concept, quantity))` — agent has at least the
    /// given quantity (planner uses at-least matching, #607).
    SelfContains { concept: Concept, quantity: u32 },
    /// `(Self_, Contains, ?)` with `isa_filter=Food` — agent has anything
    /// classified as food. Planner resolves via `IsA` inheritance.
    SelfContainsFood,
    /// `(Self_, Contains, ?)` — agent has any item at all.
    SelfContainsAny,
    /// `(Self_, Near, Concept)` — agent is near some instance of the concept.
    SelfNearConcept(Concept),
}

// ============================================================================
// EFFECT TEMPLATES
// ============================================================================

/// Static template for a plan-time triple that becomes true when the action
/// completes. Covers both fixed triples and target-derived triples.
#[derive(Debug, Clone, Copy)]
pub enum EffectTemplate {
    /// `(Self_, predicate, Exact(value))` — satisfies a need-pool goal.
    /// Used by Eat, Drink, Sleep, Rest, WarmUp, Graze.
    SelfNeedExact { predicate: Predicate, value: f32 },
    /// `(Self_, Near, Concept)` — agent ends up adjacent to an instance.
    /// Used by Build (spawns a site in place) and Construct (target becomes).
    SelfNearConcept(Concept),
    /// `(Self_, HasTrait, Concept)` — agent gains a trait tag.
    /// Used by WakeUp to mark the agent `Awake`.
    SelfHasTrait(Concept),
    /// `(Self_, Contains, Item(concept, quantity))` — agent gains items.
    /// Harvest's fallback placeholder (actual yield derived from target at
    /// plan time via [`TargetEffects::FromTargetProduces`]).
    SelfContains { concept: Concept, quantity: u32 },
}

/// How `plan_effects_for_target` should be computed.
///
/// When a target is present, the planner asks the action to project its
/// effects against that target. For most actions the static `plan_effects`
/// are enough. For a few, the effects depend on the target's `Produces`
/// or `Becomes` beliefs — those get dedicated variants so the pattern
/// stays declarative.
#[derive(Debug, Clone, Copy)]
pub enum TargetEffects {
    /// Use the static `plan_effects` unchanged regardless of target.
    Static,
    /// For each `(target, Produces, X)` belief (or each `(type, Produces, X)`
    /// via `IsA`), emit `(Self_, Contains, X)`. Used by Harvest, Attack,
    /// Bite — they all gain whatever their target produces.
    FromTargetProduces,
    /// For each `(target, Becomes, concept)` belief, emit
    /// `(Self_, Near, Concept)`. Used by Construct — after building, the
    /// agent is adjacent to the transformed entity.
    FromTargetBecomes,
    /// For each `(target, Becomes, recipe)` belief, look up
    /// `(recipe, Requires, Item(material, qty))` and emit
    /// `(target, Contains, Item)`. Used by Deposit — puts materials into
    /// a construction site.
    FromTargetBecomesRequirements,
    /// For each `(target, Contains, Item)` belief where qty > 0, emit
    /// `(Self_, Contains, Item)`. Used by Take — grabs whatever's there.
    FromTargetContains,
}

// ============================================================================
// PLAN VALIDITY
// ============================================================================

/// Plan-time filter on target candidates. Runs during enumeration to drop
/// candidates that would make the plan nonsensical before the expensive
/// A* search sees them.
#[derive(Debug, Clone, Copy)]
pub enum PlanValidity {
    /// Always valid (no target check, or target check via preconditions).
    Always,
    /// Target must have at least one `Becomes` belief. Used by Construct.
    TargetHasBecomes,
    /// Target (entity directly, or via its `IsA` type chain) produces at
    /// least one item classified as `Food` or `Resource`. Rejects
    /// known-empty Contains (qty == 0). Used by Harvest, Attack, Bite.
    TargetProducesFoodOrResource,
    /// Target has at least one `Contains` belief with qty > 0. Used by Take.
    TargetContainsAny,
    /// Target has `Contains` of an item that `IsA Food`. Used by Devour.
    TargetContainsEdible,
    /// The agent's mind knows any recipe for the given concept (any
    /// `(concept, Requires, ?)` triple). Used by Build.
    RecipeKnown(Concept),
}

// ============================================================================
// RUNTIME GATES
// ============================================================================

/// Runtime precondition check. Runs when the execution system tries to
/// start the action; returning an error keeps the action out of
/// [`ActiveActions`](super::ActiveActions) and surfaces a failure reason to
/// the brain.
#[derive(Debug, Clone)]
pub enum Gate {
    /// Agent inventory contains at least `quantity` of `concept`.
    /// Maps failure to [`FailureReason::MissingMaterials`].
    InventoryHasQuantity { concept: Concept, quantity: u32 },
    /// Agent inventory contains at least one item `IsA Food`.
    /// Maps failure to [`FailureReason::NoEdibleFood`].
    InventoryHasFood,
    /// Agent inventory is non-empty.
    /// Maps failure to [`FailureReason::MissingMaterials`].
    InventoryNonEmpty,
    /// `target_entity` is `Some`. The inner [`FailureReason`] distinguishes
    /// "target no longer exists" (`TargetGone`) from "no target was chosen"
    /// (`NoTarget`), which matters for brain-side retry logic.
    TargetEntity(FailureReason),
    /// An adjacent 3×3 tile is water.
    /// Maps failure to [`FailureReason::NoWaterNearby`].
    AdjacentToWater,
    /// A known heat-emitting entity sits on self's current tile.
    /// Maps failure to [`FailureReason::TargetGone`].
    NearHeatEmitter,
    /// A known shelter-providing entity sits on self's current tile.
    /// Maps failure to [`FailureReason::TargetGone`].
    NearShelterProvider,
    /// Agent stands on a Grass tile.
    /// Maps failure to [`FailureReason::NoEdibleFood`].
    OnGrassTile,
}

// ============================================================================
// SATIATION GATES
// ============================================================================

/// Unified satiation gate. When set, the execution gate and the survival
/// brain consult this before admitting the action — a Drink won't start
/// when hydration ≥ 95%, a Sleep won't start when wakefulness ≥ 95%, etc.
#[derive(Debug, Clone, Copy)]
pub enum SatiationGate {
    /// Eat: stomach fraction + "does the next food item fit" check.
    EatStomach,
    /// Devour: stomach fraction only (no self-inventory channel).
    HungerStomach,
    /// Drink: `physical.hydration.value`.
    HydrationValue,
    /// WarmUp: `physical.warmth.value`.
    WarmthValue,
    /// RestInShelter: `physical.rest_quality.value`.
    RestQualityValue,
    /// Sleep: `physical.wakefulness.value`.
    WakefulnessValue,
    /// Rest: `physical.stamina.aerobic_fraction()`.
    StaminaAerobic,
}

impl SatiationGate {
    pub fn need_kind(self) -> NeedKind {
        match self {
            SatiationGate::EatStomach | SatiationGate::HungerStomach => NeedKind::Hunger,
            SatiationGate::HydrationValue => NeedKind::Thirst,
            SatiationGate::WarmthValue => NeedKind::Warmth,
            SatiationGate::RestQualityValue => NeedKind::RestQuality,
            SatiationGate::WakefulnessValue => NeedKind::Sleep,
            SatiationGate::StaminaAerobic => NeedKind::Stamina,
        }
    }
}

// ============================================================================
// COMPLETION PREDICATE
// ============================================================================

/// Per-tick auto-completion check for indefinite (`u32::MAX`) timed actions.
#[derive(Debug, Clone, Copy)]
pub enum CompletionPredicate {
    /// Never auto-complete (Sleep, Idle, Construct, Converse — ended only
    /// by preemption or a lifecycle owner).
    Never,
    /// Complete when `physical.stamina.aerobic_fraction() >= threshold`.
    /// Used by Rest.
    AerobicAtLeast(f32),
    /// Complete when `physical.warmth.value >= threshold`. Used by WarmUp
    /// so the stance exits on goal-met, not on a fixed-duration timer.
    WarmthAtLeast(f32),
    /// Complete when `physical.rest_quality.value >= threshold`. Used by
    /// RestInShelter so the stance exits on goal-met, mirroring WarmUp.
    RestQualityAtLeast(f32),
}

// ============================================================================
// RUNTIME OPS
// ============================================================================

/// Declarative on-complete effects. Applied in order when a timed action
/// finishes. Custom logic that doesn't fit these variants lives in
/// [`Hooks::on_complete`].
#[derive(Debug, Clone, Copy)]
pub enum RuntimeOp {
    /// Remove `quantity` of `concept` from the agent's inventory.
    RemoveFromInventory { concept: Concept, quantity: u32 },
    /// `physical.hydration.top_up(amount)`.
    TopUpHydration(f32),
    /// `physical.warmth.top_up(amount)`.
    TopUpWarmth(f32),
    /// `physical.stamina.adjust_aerobic(amount)`.
    AdjustAerobic(f32),
    /// Emit [`SpawnRequest::Site`](super::registry::SpawnRequest::Site) at
    /// the agent's current position.
    SpawnSite {
        target: Concept,
        requirements: &'static [(Concept, u32)],
        initial_items: &'static [(Concept, u32)],
        labor_required: Option<u32>,
    },
}

// ============================================================================
// HOOKS
// ============================================================================

/// Function-pointer hooks for irreducibly custom logic. Every field is
/// `Option<fn>`; unset fields fall through to the declarative interpretation.
///
/// The hooks live adjacent to the static [`ActionDefinition`] in the same
/// file, so `EAT_DEF` with `hooks: Hooks { on_complete: Some(eat_on_complete), .. }`
/// is still colocated with `fn eat_on_complete(...)`. One interpreter, named
/// helpers — no per-action trait impl.
#[derive(Debug, Clone, Copy)]
pub struct Hooks {
    /// Runtime `can_start` check. Overrides the `gates` sequence when set.
    pub can_start: Option<fn(&ActionContext) -> Result<(), FailureReason>>,
    /// Runtime `on_complete` effect. Overrides the `on_complete_ops` list
    /// when set (actions usually set *either* ops *or* a hook, not both).
    pub on_complete: Option<fn(&mut CompletionContext)>,
    /// `on_leg_complete` for Movement/Ambient actions with custom pickers.
    pub on_leg_complete: Option<fn(&mut LegCompleteContext) -> LegResult>,
    /// Per-target planner precondition builder. Only set for actions whose
    /// target preconditions depend on the agent's beliefs about the target
    /// (Harvest's "Contains if fresh, else skip").
    pub target_preconditions: Option<fn(&TargetCandidate, &MindGraph) -> Vec<TriplePattern>>,
    /// Per-target planner consumption builder. Paired with
    /// `target_preconditions` — used by Harvest and Take.
    pub target_consumes: Option<fn(&TargetCandidate, &MindGraph) -> Vec<TriplePattern>>,
    /// Per-target effect override. Only set when [`TargetEffects`] variants
    /// don't cover the projection rule. Takes precedence over
    /// `target_effects` when present.
    pub plan_effects_for_target: Option<fn(&TargetCandidate, &MindGraph) -> Vec<Triple>>,
    /// Batch-score a list of candidate tiles by how well they match this
    /// action's preferred execution location. Arbitration samples the
    /// agent's local neighborhood with this scorer; if a meaningfully-
    /// better tile than the agent's current one exists, the proposal is
    /// replaced with a Walk toward it. Unset = fire in place regardless.
    ///
    /// Batch-over-tiles signature (not per-tile) so the scorer can
    /// filter perceived entities once, then score 82 tiles against the
    /// filtered set — instead of re-filtering per tile.
    ///
    /// Emergency semantics (e.g. "exhausted agents sleep wherever") are
    /// expressed by returning uniformly-zero scores; the prep pass's
    /// hysteresis then blocks any swap.
    pub location_preference: Option<fn(&PreferenceContext, &[IVec2]) -> Vec<f32>>,
}

impl Hooks {
    pub const EMPTY: Self = Self {
        can_start: None,
        on_complete: None,
        on_leg_complete: None,
        target_preconditions: None,
        target_consumes: None,
        plan_effects_for_target: None,
        location_preference: None,
    };
}

/// Inputs a location-preference scorer reads from. Built once per agent
/// per arbitration tick and passed to each admitted action's preference
/// scorer. Identical in shape to the drift scorer's context — both
/// mechanisms read the same per-agent world snapshot.
pub struct PreferenceContext<'a> {
    pub agent_pos: Vec2,
    pub self_concept: Option<Concept>,
    pub physical: &'a PhysicalNeeds,
    pub drives: Option<&'a PsychologicalDrives>,
    pub mind: &'a MindGraph,
    /// Pre-resolved (entity, world position) pairs for visible entities
    /// so scorers don't hit the ECS per tile.
    pub visible: &'a [(Entity, Vec2)],
    /// Parallel-indexed with `visible`: `Some(concept)` when the visible
    /// entity has an `EntityType` component. Lets scorers ask trait
    /// questions at the concept level via the ontology cache without
    /// the per-call `(entity, IsA, ?)` walk.
    pub visible_types: &'a [Option<Concept>],
    pub fields: &'a FieldGrids,
}

// ============================================================================
// RECIPE
// ============================================================================

/// Data about what an action builds. Present on Build-style actions so
/// [`crate::agent::culture::create_cultural_knowledge`] can auto-derive
/// `(concept, Requires, Item)` / `Provides` / `BuildTime` triples instead
/// of redeclaring the same numbers a second time.
#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    pub concept: Concept,
    pub requirements: &'static [(Concept, u32)],
    pub provides: &'static [Concept],
    pub build_time_ticks: u32,
}

// ============================================================================
// ACTION DEFINITION
// ============================================================================

/// The single source of truth for an action.
///
/// Written as a `pub static FOO_DEF: ActionDefinition = ActionDefinition { ... };`
/// in each `action/*.rs` file. Interpreted by
/// [`GenericAction`](super::generic_action::GenericAction) — one interpreter
/// shared across all 24 actions, replacing the per-action trait impls.
pub struct ActionDefinition {
    // ── Identity ────────────────────────────────────────────────────────
    pub action_type: ActionType,
    pub kind: ActionKind,
    pub target_source: TargetSource,
    pub base_cost: f32,

    // ── Behavior (decomposed from `Behavior` so the def can be a const) ─
    pub primitive: ActionPrimitive,
    pub target_selector: TargetSelector,
    pub intensity: IntensityPolicy,
    pub intent: Intent,

    // ── Body ────────────────────────────────────────────────────────────
    pub body_channels: &'static [ChannelUsage],
    pub posture: Option<Posture>,
    pub interruptible: bool,

    // ── Logging + per-tick effects ──────────────────────────────────────
    pub start_log: Option<&'static str>,
    pub complete_log: Option<&'static str>,
    pub joy_per_sec: f32,
    pub stomach_carbs_per_sec: f32,

    // ── Planning (data) ─────────────────────────────────────────────────
    pub preconditions: &'static [Pattern],
    pub plan_effects: &'static [EffectTemplate],
    pub plan_consumes: &'static [Pattern],
    pub target_effects: TargetEffects,
    pub plan_validity: PlanValidity,

    // ── Runtime (data) ──────────────────────────────────────────────────
    pub gates: &'static [Gate],
    pub satiation: Option<SatiationGate>,
    pub completion: CompletionPredicate,
    pub on_complete_ops: &'static [RuntimeOp],

    // ── Escape hatches for irreducibly custom logic ─────────────────────
    pub hooks: Hooks,

    // ── Optional recipe data for culture auto-derivation ────────────────
    pub recipe: Option<Recipe>,
}
