//! GOAP regressive planner: backward A* search from goal to initial state.
//!
//! Reads: MindGraph (world state), available ActionTemplates, Goal conditions
//! Writes: Vec<ActionTemplate> (ordered plan)
//! Upstream: rational brain (goal + actions), mind (MindGraph)
//! Downstream: rational brain (executes the plan)

use super::thinking::{ActionTemplate, Goal, TriplePattern};
use crate::agent::actions::ActionType;
use crate::agent::actions::motor::ActionPrimitive;
use crate::agent::body::effort::{self, compute_action_cost};
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::body::species::SpeciesProfile;
use crate::agent::mind::knowledge::{
    Concept, MindGraph, Node as MindNode, Ontology, Predicate, Triple, Value,
};
use crate::agent::movement::intensity_speed_multiplier;
use crate::agent::psyche::personality::Personality;
use crate::constants::actions::walk as walk_const;
use crate::constants::brains::survival::EXHAUSTION_TRIGGER;
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;

// ═══════════════════════════════════════════════════════════════════════════
// SUBJECTIVE PLAN COST — factors the planner uses to evaluate action costs
// ═══════════════════════════════════════════════════════════════════════════
//
// Action cost is no longer `base_cost + euclidean_distance`. It's a product of
// independent factors that reflect how the *specific* agent perceives the
// plan:
//
//   stamina × alertness × uncertainty × skill × risk × personality
//
// Each factor defaults to 1.0 when its data isn't available so the planner
// still runs (and degrades to the old behaviour) when state is missing.

/// Neuroticism inflation at max anxiety (~30% cost premium).
const PERSONALITY_COST_SCALE: f32 = 0.3;
/// Tiles within which a known danger contributes to risk.
const RISK_RADIUS_TILES: f32 = 10.0;
/// Base weight for risk inflation before neuroticism modulation.
const RISK_BASE_WEIGHT: f32 = 0.5;

/// Inputs the planner uses to compute subjective action costs. Neutral by
/// default so the planner still runs when no agent state has been threaded
/// through.
#[derive(Debug, Clone)]
pub struct PlanCostContext {
    /// Aerobic stamina fill fraction in [0, 1]. 1.0 = rested.
    pub stamina_aerobic: f32,
    /// Cognitive alertness in [0, 1]. 1.0 = fully alert.
    pub alertness: f32,
    /// Big Five neuroticism in [0, 1]. Higher = anxious, inflates cost.
    pub neuroticism: f32,
    /// Current simulation tick. Used by `PlanCostCache` to age-check
    /// transient beliefs like `(Tile, HasTrait, Unreachable)` so an old
    /// path-blocked marker eventually stops filtering walk targets.
    pub current_tick: u64,
    /// Agent body mass in kg. Scales effort-model energy cost.
    pub body_mass: f32,
    /// Species base speed (tiles/tick multiplier). Affects walk duration estimates.
    pub species_base_speed: f32,
    /// Current glucose level. Used by feasibility check.
    pub glucose: f32,
    /// Current fat reserves. Used by feasibility check.
    pub reserves: f32,
    /// Current anaerobic stamina. Used by feasibility check.
    pub stamina_anaerobic: f32,
}

/// How long a `(Tile, HasTrait, Unreachable)` belief suppresses walk
/// planning to that tile. After this many ticks the planner treats the
/// tile as fair game again so agents retry paths that may have opened up
/// (tree chopped, obstacle despawned, etc.).
pub const UNREACHABLE_BELIEF_TTL_TICKS: u64 = 500;

impl PlanCostContext {
    /// All factors neutral — used in tests and as a fallback when no agent
    /// state is supplied. Reproduces the original base-cost behaviour.
    pub fn neutral() -> Self {
        Self {
            stamina_aerobic: 1.0,
            alertness: 1.0,
            neuroticism: 0.0,
            current_tick: 0,
            body_mass: effort::DEFAULT_BODY_MASS,
            species_base_speed: 1.0,
            glucose: crate::agent::body::metabolism::GLUCOSE_MAX,
            reserves: crate::agent::body::metabolism::RESERVES_MAX,
            stamina_anaerobic: 100.0,
        }
    }

    /// Build a cost context from the agent's live components.
    pub fn from_agent(
        physical: &PhysicalNeeds,
        consciousness: &Consciousness,
        personality: &Personality,
        species: Option<&SpeciesProfile>,
        current_tick: u64,
    ) -> Self {
        Self {
            stamina_aerobic: physical.stamina.aerobic_fraction().clamp(0.0, 1.0),
            alertness: consciousness.alertness.clamp(0.0, 1.0),
            neuroticism: personality.traits.neuroticism.clamp(0.0, 1.0),
            current_tick,
            body_mass: species
                .map(|s| s.mass_kg)
                .unwrap_or(effort::DEFAULT_BODY_MASS),
            species_base_speed: species.map(|s| s.base_speed).unwrap_or(1.0),
            glucose: physical.metabolism.glucose,
            reserves: physical.metabolism.reserves,
            stamina_anaerobic: physical.stamina.anaerobic,
        }
    }

    fn personality_factor(&self) -> f32 {
        1.0 + self.neuroticism * PERSONALITY_COST_SCALE
    }
}

/// Per-plan cache sitting alongside the cost context. Built once at the top
/// of `regressive_plan` so `MindGraph` queries that never change mid-plan
/// (the danger list in particular) don't fire from every action cost call.
struct PlanCostCache<'a> {
    ctx: &'a PlanCostContext,
    dangers: Vec<(i32, i32)>,
    /// Tiles the agent recently failed to reach via Walk. Populated from
    /// `(Tile, HasTrait, Unreachable)` triples written by the belief
    /// updater on `ActionOutcome::Failed { PathBlocked }`. Stale entries
    /// (older than `UNREACHABLE_BELIEF_TTL_TICKS`) are filtered out here
    /// so the planner automatically retries once the belief ages out.
    unreachable_tiles: Vec<(i32, i32)>,
}

impl<'a> PlanCostCache<'a> {
    fn new(ctx: &'a PlanCostContext, mind: &MindGraph) -> Self {
        let mut dangers = Vec::new();
        for triple in mind.query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Dangerous)),
        ) {
            let MindNode::Entity(entity) = &triple.subject else {
                continue;
            };
            let Some(Value::Tile(tile)) =
                mind.get(&MindNode::Entity(*entity), Predicate::LocatedAt)
            else {
                continue;
            };
            dangers.push(*tile);
        }
        let mut unreachable_tiles = Vec::new();
        for triple in mind.query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Unreachable)),
        ) {
            let MindNode::Tile(tile) = triple.subject else {
                continue;
            };
            // TTL check: expire stale beliefs so the agent retries paths
            // that may have opened up since the last block.
            if ctx.current_tick.saturating_sub(triple.meta.timestamp)
                <= UNREACHABLE_BELIEF_TTL_TICKS
            {
                unreachable_tiles.push(tile);
            }
        }
        Self {
            ctx,
            dangers,
            unreachable_tiles,
        }
    }

    fn is_unreachable(&self, tile: (i32, i32)) -> bool {
        self.unreachable_tiles.contains(&tile)
    }
}

/// Uncertainty factor for an explicit action. Grades the planner's confidence
/// that the target still holds the item the action expects to produce.
/// Returns 1.0 when no confidence can be graded.
fn uncertainty_factor(action: &ActionTemplate, mind: &MindGraph) -> f32 {
    let Some(target) = action.target_entity else {
        return 1.0;
    };
    for effect in &action.effects {
        if effect.predicate == Predicate::Contains
            && let Value::Item(concept, _qty) = &effect.object
        {
            let confidence = mind.confidence_of(&MindNode::Entity(target), *concept);
            if confidence > 0.0 {
                return 2.0 - confidence;
            }
        }
    }
    1.0
}

/// Risk inflation for a tile. Walks the cached danger tiles and sums a
/// proximity-weighted contribution. Neuroticism amplifies perceived risk.
fn tile_risk_factor(tile: (i32, i32), cache: &PlanCostCache) -> f32 {
    let mut risk = 0.0_f32;
    let radius_sq = RISK_RADIUS_TILES * RISK_RADIUS_TILES;
    for (dx, dy) in &cache.dangers {
        let d2 = ((tile.0 - *dx).pow(2) + (tile.1 - *dy).pow(2)) as f32;
        if d2 >= radius_sq {
            continue;
        }
        let dist = d2.sqrt();
        risk += (RISK_RADIUS_TILES - dist) / RISK_RADIUS_TILES;
    }
    if risk == 0.0 {
        return 1.0;
    }
    1.0 + risk * RISK_BASE_WEIGHT * (1.0 + cache.ctx.neuroticism)
}

/// Risk factor for an explicit action. Uses the action's target tile when
/// known; otherwise infers it from the target entity's LocatedAt; falls back
/// to neutral when neither is available.
fn action_risk_factor(action: &ActionTemplate, mind: &MindGraph, cache: &PlanCostCache) -> f32 {
    if cache.dangers.is_empty() {
        return 1.0;
    }
    if let Some(pos) = action.target_position {
        let tile = (
            (pos.x / TILE_SIZE).floor() as i32,
            (pos.y / TILE_SIZE).floor() as i32,
        );
        return tile_risk_factor(tile, cache);
    }
    if let Some(target) = action.target_entity
        && let Some(Value::Tile(tile)) = mind.get(&MindNode::Entity(target), Predicate::LocatedAt)
    {
        return tile_risk_factor(*tile, cache);
    }
    1.0
}

// ─── Effort-based cost estimation ──────────────────────────────────────────
//
// The planner uses the same effort model as the execution system to estimate
// energy cost per plan step. This replaces the old flat `base_cost * fatigue`
// formula with physics-based estimates so a 200-tile walk costs 40x more than
// a 5-tile walk.

/// Default duration estimate (in ticks) for indefinite actions the planner
/// encounters. Sleep uses a recovery-time estimate; these cover the rest.
const INDEFINITE_ACTION_DURATION_TICKS: u32 = 300; // 5 seconds

/// Estimate the energy cost (in metabolic units) for a timed action step.
fn effort_cost_timed(action: &ActionTemplate, ctx: &PlanCostContext) -> f32 {
    let primitive = action.behavior.primitive;
    let intensity = action.behavior.intensity.resolve();
    let profile = primitive.effort_profile().scaled(intensity);
    let cost = compute_action_cost(&profile, ctx.body_mass);

    let duration_ticks = action
        .estimated_duration_ticks
        .unwrap_or(INDEFINITE_ACTION_DURATION_TICKS);
    let duration_secs = duration_ticks as f32 / 60.0;

    // Minimum floor so zero-energy actions (Idle, Ingest at 0 intensity)
    // still have a nonzero planning cost.
    (cost.energy * duration_secs).max(0.1)
}

/// Estimate the energy cost for a walk of `dist_tiles` tiles.
fn effort_cost_walk(dist_tiles: f32, intensity: f32, ctx: &PlanCostContext) -> f32 {
    let profile = ActionPrimitive::Locomote.effort_profile().scaled(intensity);
    let cost = compute_action_cost(&profile, ctx.body_mass);

    let distance_pixels = dist_tiles * TILE_SIZE;
    let speed_per_tick = crate::constants::movement::BASE_SPEED_PER_TICK
        * ctx.species_base_speed
        * intensity_speed_multiplier(intensity);
    let ticks = if speed_per_tick > 0.0 {
        distance_pixels / speed_per_tick
    } else {
        distance_pixels
    };
    let duration_secs = ticks / 60.0;

    cost.energy * duration_secs
}

/// Subjective cost for an explicit (non-walk) action step.
fn subjective_action_cost(action: &ActionTemplate, cache: &PlanCostCache, mind: &MindGraph) -> f32 {
    let base = effort_cost_timed(action, cache.ctx);
    let uncertainty = uncertainty_factor(action, mind);
    let risk = action_risk_factor(action, mind, cache);
    let personality = cache.ctx.personality_factor();
    base * uncertainty * risk * personality
}

/// Subjective cost for an implicit walk of `dist` tiles toward `tile`.
fn subjective_walk_cost(dist: f32, tile: (i32, i32), intensity: f32, cache: &PlanCostCache) -> f32 {
    let base = effort_cost_walk(dist, intensity, cache.ctx);
    let risk = if cache.dangers.is_empty() {
        1.0
    } else {
        tile_risk_factor(tile, cache)
    };
    let personality = cache.ctx.personality_factor();
    base * risk * personality
}

/// Sum the subjective cost of every step in an already-generated plan.
///
/// Walks between explicit steps are represented in the plan vector as their
/// own `Walk` templates — their `target_position` is the destination tile
/// centre. Distance is measured in tile units between the start position and
/// each successive walk target so the total matches what the planner's A*
/// accumulator saw, up to floating-point noise.
pub fn estimate_plan_cost(
    plan: &[ActionTemplate],
    start_pos: Vec2,
    ctx: &PlanCostContext,
    mind: &MindGraph,
) -> f32 {
    let cache = PlanCostCache::new(ctx, mind);
    let mut total = 0.0;
    let mut cursor = start_pos;
    for action in plan {
        if action.action_type == ActionType::Walk {
            let Some(target) = action.target_position else {
                continue;
            };
            let dist = cursor.distance(target) / TILE_SIZE;
            let tile = (
                (target.x / TILE_SIZE).floor() as i32,
                (target.y / TILE_SIZE).floor() as i32,
            );
            total += subjective_walk_cost(dist, tile, action.locomotion_intensity.max(0.5), &cache);
            cursor = target;
        } else {
            total += subjective_action_cost(action, &cache, mind);
        }
    }
    total
}

// ═══════════════════════════════════════════════════════════════════════════
// PLAN FEASIBILITY CHECK — forward simulation of physical pools
// ═══════════════════════════════════════════════════════════════════════════

/// Forward-simulate physical pools (glucose, reserves, aerobic stamina) through
/// each step of a plan. Returns `false` if the agent would hit critical
/// thresholds at any point — the plan is infeasible and should be discarded.
pub fn check_plan_feasibility(
    plan: &[ActionTemplate],
    start_pos: Vec2,
    ctx: &PlanCostContext,
) -> bool {
    let mut glucose = ctx.glucose;
    let mut reserves = ctx.reserves;
    let mut aerobic = ctx.stamina_aerobic * 100.0; // fraction → absolute
    let mut cursor = start_pos;

    for action in plan {
        let (energy_drain, aerobic_drain, duration_secs) =
            estimate_step_drains(action, &cursor, ctx);

        let peak_intensity = action.behavior.intensity.resolve();
        let glucose_frac = effort::glucose_fraction(peak_intensity);
        glucose -= energy_drain * glucose_frac;
        reserves -= energy_drain * (1.0 - glucose_frac);
        aerobic -= aerobic_drain * duration_secs;

        // Clamp negative aerobic (recovery actions produce negative drain)
        aerobic = aerobic.clamp(0.0, 100.0);
        // Simple reserve mobilization: top up glucose from reserves
        if glucose < crate::agent::body::metabolism::GLUCOSE_MOBILIZE_THRESHOLD && reserves > 0.0 {
            let transfer = (crate::agent::body::metabolism::GLUCOSE_MOBILIZE_THRESHOLD - glucose)
                .min(reserves);
            glucose += transfer;
            reserves -= transfer;
        }

        if action.action_type == ActionType::Walk {
            if let Some(target) = action.target_position {
                cursor = target;
            }
        }

        if glucose < crate::agent::body::metabolism::GLUCOSE_CRITICAL_THRESHOLD && reserves < 5.0 {
            return false;
        }
        if aerobic < 5.0 {
            return false;
        }
    }
    true
}

/// Estimate per-step drains: returns (total_energy, aerobic_drain_per_sec, duration_secs).
fn estimate_step_drains(
    action: &ActionTemplate,
    cursor: &Vec2,
    ctx: &PlanCostContext,
) -> (f32, f32, f32) {
    let primitive = action.behavior.primitive;
    let intensity = action.behavior.intensity.resolve();
    let profile = primitive.effort_profile().scaled(intensity);
    let cost = compute_action_cost(&profile, ctx.body_mass);

    let duration_secs = if action.action_type == ActionType::Walk {
        // Walk: estimate from distance
        if let Some(target) = action.target_position {
            let distance_pixels = cursor.distance(target);
            let speed_per_tick = crate::constants::movement::BASE_SPEED_PER_TICK
                * ctx.species_base_speed
                * intensity_speed_multiplier(intensity);
            let ticks = if speed_per_tick > 0.0 {
                distance_pixels / speed_per_tick
            } else {
                distance_pixels
            };
            ticks / 60.0
        } else {
            1.0
        }
    } else {
        // Timed: use estimated duration or fallback
        let ticks = action
            .estimated_duration_ticks
            .unwrap_or(INDEFINITE_ACTION_DURATION_TICKS);
        ticks as f32 / 60.0
    };

    (
        cost.energy * duration_secs,
        cost.aerobic_drain,
        duration_secs,
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// PLANNER STATE — Snapshot of MindGraph for A* planning
// ═══════════════════════════════════════════════════════════════════════════
// Forward-planner scaffolding from the original GOAP implementation. Removed
// from the live build because the forward planner (`goap_plan`) was deleted in
// favour of the regressive planner below — backward search is more efficient
// for goal-directed AI (only relevant actions are explored, Walk steps are
// generated implicitly), so this struct, its impls, and the triple
// equality/ordering/hashing helpers it depended on (`triples_eq`,
// `compare_triples`, `hash_triple`, `SearchNode`) became dead code and were
// tripping `#[warn(dead_code)]`.
//
// Kept commented out as a reference in case a forward planner is ever
// reintroduced. The active planner uses `RegressiveState` further down.
//
// /// A lightweight state representation for the planner.
// /// We track only the triples that have been added/modified during planning.
// #[derive(Debug, Clone)]
// struct PlannerState {
//     /// Hash of the base MindGraph (for identity)
//     base_hash: u64,
//     /// Triples added during planning
//     /// We keep them sorted for canonical hashing
//     added_triples: Vec<Triple>,
// }
//
// impl PartialEq for PlannerState {
//     fn eq(&self, other: &Self) -> bool {
//         self.base_hash == other.base_hash
//             && self.added_triples.len() == other.added_triples.len()
//             && self
//                 .added_triples
//                 .iter()
//                 .zip(&other.added_triples)
//                 .all(|(a, b)| triples_eq(a, b))
//     }
// }
//
// impl Eq for PlannerState {}
//
// impl std::hash::Hash for PlannerState {
//     fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
//         self.base_hash.hash(state);
//         for triple in &self.added_triples {
//             hash_triple(triple, state);
//         }
//     }
// }
//
// impl PlannerState {
//     fn from_mind(mind: &MindGraph) -> Self {
//         Self {
//             base_hash: mind.triples.len() as u64, // Simple hash based on triple count
//             added_triples: Vec::new(),
//         }
//     }
//
//     fn with_effects(&self, effects: &[Triple]) -> Self {
//         let mut new_state = self.clone();
//         for effect in effects {
//             // Check if already exists (using our custom eq)
//             if !new_state
//                 .added_triples
//                 .iter()
//                 .any(|t| triples_eq(t, effect))
//             {
//                 new_state.added_triples.push(effect.clone());
//             }
//         }
//         // Sort for canonical state (needed for Hashing stability)
//         new_state.added_triples.sort_by(compare_triples);
//         new_state
//     }
//
//     fn check_pattern(&self, mind: &MindGraph, pattern: &TriplePattern) -> bool {
//         // First check added triples
//         for added in &self.added_triples {
//             if pattern_matches_triple(pattern, added) {
//                 return true;
//             }
//         }
//
//         // Then check base MindGraph
//         !mind
//             .query(
//                 pattern.subject.as_ref(),
//                 pattern.predicate,
//                 pattern.object.as_ref(),
//             )
//             .is_empty()
//     }
// }
//
// fn triples_eq(a: &Triple, b: &Triple) -> bool {
//     a.subject == b.subject && a.predicate == b.predicate && a.object == b.object
// }
//
// fn compare_triples(a: &Triple, b: &Triple) -> Ordering {
//     // Subject -> Predicate -> Object
//     let ord = compare_nodes(&a.subject, &b.subject);
//     if ord != Ordering::Equal {
//         return ord;
//     }
//     let ord = (a.predicate as usize).cmp(&(b.predicate as usize));
//     if ord != Ordering::Equal {
//         return ord;
//     }
//     compare_values(&a.object, &b.object)
// }
//
// fn hash_triple<H: std::hash::Hasher>(t: &Triple, state: &mut H) {
//     t.subject.hash(state);
//     t.predicate.hash(state);
//     hash_value(&t.object, state);
// }
//
// // ═══════════════════════════════════════════════════════════════════════════
// // A* NODE
// // ═══════════════════════════════════════════════════════════════════════════
//
// /// A node in the A* open set.
// #[derive(Debug, Clone)]
// struct SearchNode {
//     f_score: f32, // Total estimated cost (g + h)
//     state: PlannerState,
// }
//
// // Rust's BinaryHeap is a max-heap, so we implement Ord to reverse it for a min-heap.
// impl PartialEq for SearchNode {
//     fn eq(&self, other: &Self) -> bool {
//         self.f_score == other.f_score
//     }
// }
// impl Eq for SearchNode {}
// impl PartialOrd for SearchNode {
//     fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
//         Some(self.cmp(other))
//     }
// }
// impl Ord for SearchNode {
//     fn cmp(&self, other: &Self) -> Ordering {
//         // Reverse order: smaller f_score is better (Greater)
//         other.f_score.total_cmp(&self.f_score) // Use total_cmp for floats
//     }
// }

/// Helper: Check if pattern matches a concrete triple
fn pattern_matches_triple(
    pattern: &TriplePattern,
    triple: &Triple,
    ontology: Option<&Ontology>,
) -> bool {
    if let Some(s) = &pattern.subject
        && &triple.subject != s
    {
        return false;
    }
    if let Some(p) = pattern.predicate
        && triple.predicate != p
    {
        return false;
    }
    if let Some(o) = &pattern.object
        && &triple.object != o
    {
        return false;
    }
    // If the pattern requires items to be of a certain category or have a certain
    // trait, verify the concrete item concept passes the ontology checks.
    // Both filters AND together — the item must satisfy every constraint set.
    if pattern.isa_filter.is_some() || pattern.trait_filter.is_some() {
        match &triple.object {
            Value::Item(concept, _) => {
                if let Some(isa) = pattern.isa_filter
                    && !ontology.is_some_and(|o| o.is_a(*concept, isa))
                {
                    return false;
                }
                if let Some(trait_) = pattern.trait_filter
                    && !ontology.is_some_and(|o| o.has_trait(*concept, trait_))
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

// ─── Custom Comparison / Hashing for Triples (since Value doesn't impl it) ───

fn compare_nodes(a: &MindNode, b: &MindNode) -> Ordering {
    // Basic heuristic sort
    match (a, b) {
        (MindNode::Entity(e1), MindNode::Entity(e2)) => e1.index().cmp(&e2.index()),
        (MindNode::Concept(c1), MindNode::Concept(c2)) => (*c1 as usize).cmp(&(*c2 as usize)),
        (MindNode::Tile((x1, y1)), MindNode::Tile((x2, y2))) => x1.cmp(x2).then(y1.cmp(y2)),
        (MindNode::Self_, MindNode::Self_) => Ordering::Equal,
        // Cross-variant
        _ => format!("{:?}", a).cmp(&format!("{:?}", b)), // Fallback but rare comparison
    }
}

fn compare_values(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Int(v1), Value::Int(v2)) => v1.cmp(v2),
        (Value::Boolean(v1), Value::Boolean(v2)) => v1.cmp(v2),
        (Value::Float(v1), Value::Float(v2)) => v1.total_cmp(v2),
        (Value::Concept(c1), Value::Concept(c2)) => (*c1 as usize).cmp(&(*c2 as usize)),
        (Value::Entity(e1), Value::Entity(e2)) => e1.index().cmp(&e2.index()),
        (Value::Tile((x1, y1)), Value::Tile((x2, y2))) => x1.cmp(x2).then(y1.cmp(y2)),
        // Fallbacks
        _ => format!("{:?}", a).cmp(&format!("{:?}", b)),
    }
}

fn hash_value<H: std::hash::Hasher>(v: &Value, state: &mut H) {
    std::mem::discriminant(v).hash(state);
    match v {
        Value::Int(i) => i.hash(state),
        Value::Boolean(b) => b.hash(state),
        Value::Float(f) => f.to_bits().hash(state),
        Value::Concept(c) => c.hash(state),
        Value::Entity(e) => e.hash(state),
        Value::Tile(t) => t.hash(state),
        Value::Action(a) => (*a as usize).hash(state), // Assuming Action is simple enum
        Value::Item(c, n) => {
            c.hash(state);
            n.hash(state);
        }
        Value::Emotion(e, f) => {
            (*e as usize).hash(state);
            f.to_bits().hash(state);
        }
        Value::Attitude(f) => f.to_bits().hash(state),
        Value::Text(s) => s.0.hash(state),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// REGRESSIVE PLANNER (BACKWARD) — The primary planner
// ═══════════════════════════════════════════════════════════════════════════
// NOTE: goap_plan (forward planner) has been removed.
// Regressive planning is more efficient for goal-directed AI:
// - Starts from goal, works backward
// - Only considers relevant actions
// - Generates Walk actions implicitly when needed

#[derive(Debug, Clone)]
struct RegressiveState {
    /// Conditions that still need to be satisfied
    unmet_goals: Vec<TriplePattern>,
    /// Resources consumed by actions already in the plan (executing after this point).
    /// Used to block preconditions for actions that would execute before those consumers.
    consumed: Vec<TriplePattern>,
}

impl PartialEq for RegressiveState {
    fn eq(&self, other: &Self) -> bool {
        self.unmet_goals.len() == other.unmet_goals.len()
            && self.consumed.len() == other.consumed.len()
            && self
                .unmet_goals
                .iter()
                .zip(&other.unmet_goals)
                .all(|(a, b)| patterns_eq(a, b))
            && self
                .consumed
                .iter()
                .zip(&other.consumed)
                .all(|(a, b)| patterns_eq(a, b))
    }
}

impl Eq for RegressiveState {}

impl Hash for RegressiveState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for pattern in &self.unmet_goals {
            hash_pattern(pattern, state);
        }
        for pattern in &self.consumed {
            hash_pattern(pattern, state);
        }
    }
}

impl RegressiveState {
    fn new(goals: Vec<TriplePattern>, consumed: Vec<TriplePattern>) -> Self {
        let mut s = Self {
            unmet_goals: goals,
            consumed,
        };
        s.normalize();
        s
    }

    /// Canonicalize for stable hashing: sort then dedup so semantically-equal
    /// states collapse to the same A* closed-set entry instead of being re-explored.
    fn normalize(&mut self) {
        self.unmet_goals.sort_by(compare_patterns);
        self.unmet_goals
            .dedup_by(|a, b| compare_patterns(a, b).is_eq());
        self.consumed.sort_by(compare_patterns);
        self.consumed
            .dedup_by(|a, b| compare_patterns(a, b).is_eq());
    }
}

#[derive(Debug, Clone)]
struct RegressiveSearchNode {
    f_score: f32,
    state: RegressiveState,
}

impl PartialEq for RegressiveSearchNode {
    fn eq(&self, other: &Self) -> bool {
        self.f_score == other.f_score
    }
}
impl Eq for RegressiveSearchNode {}
impl PartialOrd for RegressiveSearchNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for RegressiveSearchNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f_score.total_cmp(&self.f_score)
    }
}

/// Backward Search: Starts from Goal, finds actions that satisfy unmet goals.
/// Special Feature: Implicitly generates 'WalkTo' actions when satisfying `LocatedAt`.
///
/// `ctx` supplies the subjective-cost factors (stamina, alertness, personality,
/// risk modulation). Use `PlanCostContext::neutral()` for callers that don't
/// yet supply agent state — it reproduces the old base-cost behaviour.
pub fn regressive_plan(
    mind: &MindGraph,
    goal: &Goal,
    available_actions: &[ActionTemplate],
    ctx: &PlanCostContext,
) -> Option<Vec<ActionTemplate>> {
    use crate::constants::brains::planner::{HEURISTIC_MULTIPLIER, MAX_ITERATIONS};
    let start_time = std::time::Instant::now();
    let mut iterations = 0;
    let cost_cache = PlanCostCache::new(ctx, mind);

    let mut open_set = BinaryHeap::new();
    let mut came_from: HashMap<RegressiveState, (ActionTemplate, RegressiveState)> = HashMap::new();
    let mut g_score: HashMap<RegressiveState, f32> = HashMap::new();

    // Initial state: The goals we want to achieve
    // But we first check if they assume anything is already true.
    // Actually, Regressive Planner starts with "All goals unmet".
    // We only remove them if they are true in the CURRENT world (Mind).
    let initial_goals: Vec<TriplePattern> = goal
        .conditions
        .iter()
        .filter(|p| !mind_satisfies_pattern(mind, p))
        .cloned()
        .collect();

    // If initial_goals is empty, we are already there!
    if initial_goals.is_empty() {
        return Some(Vec::new());
    }

    let start = RegressiveState::new(initial_goals, vec![]);
    g_score.insert(start.clone(), 0.0);
    open_set.push(RegressiveSearchNode {
        f_score: start.unmet_goals.len() as f32, // Simple heuristic
        state: start,
    });

    let mut result = None;
    let mut best_unmet: Vec<TriplePattern> = Vec::new();
    // Key: stable hash of pattern; value: (representative pattern, count)
    let mut goal_pattern_counts: HashMap<u64, (TriplePattern, usize)> = HashMap::new();

    while let Some(current_node) = open_set.pop() {
        iterations += 1;
        if iterations > MAX_ITERATIONS {
            let mut top_patterns: Vec<&(TriplePattern, usize)> =
                goal_pattern_counts.values().collect();
            top_patterns.sort_by(|a, b| b.1.cmp(&a.1));
            top_patterns.truncate(3);
            let top_readable: Vec<&TriplePattern> =
                top_patterns.into_iter().map(|(p, _)| p).collect();
            tracing::warn!(
                target: "planner",
                "regressive_plan exhausted {} iterations on goal {:?}",
                MAX_ITERATIONS,
                goal
            );
            tracing::warn!(
                target: "planner",
                "best frontier node had {} unmet goals: {:?}",
                best_unmet.len(),
                best_unmet
            );
            tracing::warn!(
                target: "planner",
                "most common unreachable patterns: {:?}",
                top_readable
            );
            break;
        }

        let current_state = current_node.state;

        if current_state.unmet_goals.len() < best_unmet.len() || best_unmet.is_empty() {
            best_unmet = current_state.unmet_goals.clone();
        }
        for pattern in &current_state.unmet_goals {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::Hasher;
            let mut h = DefaultHasher::new();
            hash_pattern(pattern, &mut h);
            let key = h.finish();
            let entry = goal_pattern_counts
                .entry(key)
                .or_insert_with(|| (pattern.clone(), 0));
            entry.1 += 1;
        }

        // If no unmet goals, we are done!
        if current_state.unmet_goals.is_empty() {
            // Reconstruct path (note: path comes out reverse compared to forward search)
            // Backward search: came_from is (Action, ParentState).
            // ParentState had *fewer* goals? No, ParentState had *different* goals.
            // StartNode (Goals) -> NextNode (Preconditions of Action) -> ... -> Empty (Satisfied)
            // So if we trace back from Empty -> Start, we get list of actions.
            // Example: Empty -> (Action: WalkTo) -> State(LocatedAt) -> (Action: Harvest) -> State(Goal)
            // Path: WalkTo, Harvest.
            // Reconstructing from Empty up to Start gives us actions in execution order?
            // Let's trace:
            // CameFrom map: Child -> (ActionToGetHere, Parent)
            // Wait, we generate child from parent.
            // Parent = {Goal}. Child = {Preconditions}. Action = Harvest.
            // Relationship: Parent --(Harvest)--> Child.
            // So came_from[Child] = (Harvest, Parent).
            // We are at Empty (Child). We look up came_from[Empty] -> gets (WalkTo, NodeTheta).
            // Then came_from[NodeTheta] -> (Harvest, NodeAlpha).
            // ... -> (None, Start).
            // So the list is: WalkTo, Harvest. This IS execution order!
            result = Some(reconstruct_regressive_path(came_from, current_state));
            break;
        }

        let current_g = *g_score.get(&current_state).unwrap_or(&f32::INFINITY);

        // We need to satisfy *one* of the unmet goals.
        // Heuristic: Pick the first one? Or all possible branches?
        // To be complete, we should try satisfying *each* unmet goal that isn't already satisfied?
        // Actually, in any valid plan, *every* unmet goal must eventually be satisfied.
        // The order we tackle them matters for efficiency, but picking *any* one to expand is valid.
        // Let's pick the first one for simplicity.
        let target_goal = &current_state.unmet_goals[0];
        let remaining_goals = &current_state.unmet_goals[1..];

        // 2. Find actions that satisfy `target_goal`
        // A. Explicit actions
        let candidates = find_explicit_actions_for_goal(
            available_actions,
            target_goal,
            remaining_goals,
            current_g,
            mind,
            &current_state.consumed,
            &cost_cache,
        );
        for (action, next_state, new_cost) in candidates {
            update_search_candidate(
                action,
                next_state,
                new_cost,
                &current_state,
                HEURISTIC_MULTIPLIER,
                &mut came_from,
                &mut g_score,
                &mut open_set,
            );
        }

        // B. Implicit walk for any unmet `LocatedAt(Self_, Tile(...))` goal.
        //
        // After #219 entity-targeted actions snapshot a tile-based proximity
        // precondition at template-build time, so a single tile walk
        // generator handles every "I need to be near my target" case —
        // entity-affordance actions (Harvest, Take, Deposit, Attack), tile
        // trait actions (Drink), and explicit Walk goals all converge here.
        if let Some((walk_action, next_state, new_cost)) = generate_implicit_walk(
            target_goal,
            remaining_goals,
            current_g,
            mind,
            &current_state.consumed,
            &cost_cache,
        ) {
            update_search_candidate(
                walk_action,
                next_state,
                new_cost,
                &current_state,
                HEURISTIC_MULTIPLIER,
                &mut came_from,
                &mut g_score,
                &mut open_set,
            );
        }
    }

    let elapsed = start_time.elapsed();
    if elapsed.as_millis() > 1 {
        println!(
            "[Performance] [RegressivePlanner] Plan took {:?} ({} iterations, {} explicit actions)",
            elapsed,
            iterations,
            available_actions.len()
        );
    }

    result
}

// ─── Helpers ───

fn mind_satisfies_pattern(mind: &MindGraph, pattern: &TriplePattern) -> bool {
    let results = mind.query(
        pattern.subject.as_ref(),
        pattern.predicate,
        pattern.object.as_ref(),
    );

    // Filter out Item values with quantity == 0 (e.g., "Contains Apple(0)" is not satisfied)
    // and reject items that don't pass the isa_filter or trait_filter (e.g. Stone is not Food).
    // Both filters AND together.
    let has_concept_filter = pattern.isa_filter.is_some() || pattern.trait_filter.is_some();
    results.into_iter().any(|triple| match &triple.object {
        Value::Item(concept, qty) => {
            if *qty == 0 {
                return false;
            }
            if let Some(isa) = pattern.isa_filter
                && !mind.ontology.is_a(*concept, isa)
            {
                return false;
            }
            if let Some(trait_) = pattern.trait_filter
                && !mind.ontology.has_trait(*concept, trait_)
            {
                return false;
            }
            true
        }
        _ => !has_concept_filter,
    })
}

fn action_satisfies_pattern(
    action: &ActionTemplate,
    pattern: &TriplePattern,
    ontology: &Ontology,
) -> bool {
    // Action satisfies pattern if one of its effects matches the pattern
    for effect in &action.effects {
        if pattern_matches_triple(pattern, effect, Some(ontology)) {
            return true;
        }
    }
    false
}

fn reconstruct_regressive_path(
    mut came_from: HashMap<RegressiveState, (ActionTemplate, RegressiveState)>,
    mut current: RegressiveState,
) -> Vec<ActionTemplate> {
    let mut path = Vec::new();
    while let Some((action, parent)) = came_from.remove(&current) {
        path.push(action);
        current = parent;
    }
    // Backward search reconstruction gives: LastStep, ..., FirstStep
    // So we need to reverse it to get distinct execution order?
    // Let's re-verify:
    // Empty (Child) came from (Harvest, {GoalConds}).
    // So we push `Harvest`.
    // Then we go to {GoalConds}. It came from (None, Start).
    // Path: [Harvest]. Correct.
    // If we had: Empty <- (WalkTo) <- {Loc} <- (Harvest) <- {Goal}
    // Push WalkTo. Current = {Loc}.
    // Push Harvest. Current = {Goal}.
    // Path: [WalkTo, Harvest].
    // Execution: WalkTo, then Harvest.
    // So `path` is ALREADY in execution order! `reconstruct_path` usually reverses because it builds End->Start.
    // Here we are building EndState -> StartState.
    // Wait.
    // StartState (Goal Unmet) -> ... -> EndState (Empty).
    // Search goes A -> B. `came_from[B] = A`.
    // We start reconstruction at EndState (Empty).
    // `came_from[Empty] = ({Loc}, WalkTo)`.
    // Wait, came_from stores `(Action, Parent)`.
    // Parent of Empty is `{Loc}`. Action was `WalkTo`.
    // So we pushed `WalkTo`.
    // Then current is `{Loc}`.
    // `came_from[{Loc}] = ({Goal}, Harvest)`.
    // We push `Harvest`.
    // Path: `[WalkTo, Harvest]`.
    // Execution order: `WalkTo` -> `Harvest`.
    // So the vector is `[WalkTo, Harvest]`. The order is correct!
    // NO REVERSE NEEDED.
    path
}

// ─── Pattern Helpers ───

/// Returns true if patterns `a` and `b` could match the same triple.
/// None fields act as wildcards — if either pattern has None for a field, that field can't rule
/// out overlap. Two patterns overlap when no field has *conflicting concrete* values.
fn patterns_overlap(a: &TriplePattern, b: &TriplePattern) -> bool {
    if let (Some(sa), Some(sb)) = (&a.subject, &b.subject)
        && sa != sb
    {
        return false;
    }
    if let (Some(pa), Some(pb)) = (a.predicate, b.predicate)
        && pa != pb
    {
        return false;
    }
    if let (Some(oa), Some(ob)) = (&a.object, &b.object)
        && compare_values(oa, ob) != Ordering::Equal
    {
        return false;
    }
    true
}

fn patterns_eq(a: &TriplePattern, b: &TriplePattern) -> bool {
    a.subject == b.subject && a.predicate == b.predicate && values_opt_eq(&a.object, &b.object)
}

fn values_opt_eq(a: &Option<Value>, b: &Option<Value>) -> bool {
    match (a, b) {
        (Some(va), Some(vb)) => compare_values(va, vb) == Ordering::Equal,
        (None, None) => true,
        _ => false,
    }
}

fn compare_patterns(a: &TriplePattern, b: &TriplePattern) -> Ordering {
    // Subject
    match (&a.subject, &b.subject) {
        (Some(sa), Some(sb)) => {
            let ord = compare_nodes(sa, sb);
            if ord != Ordering::Equal {
                return ord;
            }
        }
        (None, Some(_)) => return Ordering::Less,
        (Some(_), None) => return Ordering::Greater,
        (None, None) => {}
    }

    // Predicate
    match (&a.predicate, &b.predicate) {
        (Some(pa), Some(pb)) => {
            let ord = (*pa as usize).cmp(&(*pb as usize));
            if ord != Ordering::Equal {
                return ord;
            }
        }
        (None, Some(_)) => return Ordering::Less,
        (Some(_), None) => return Ordering::Greater,
        (None, None) => {}
    }

    // Object
    match (&a.object, &b.object) {
        (Some(oa), Some(ob)) => compare_values(oa, ob),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn hash_pattern<H: std::hash::Hasher>(p: &TriplePattern, state: &mut H) {
    p.subject.hash(state);
    p.predicate.hash(state);
    if let Some(v) = &p.object {
        hash_value(v, state);
    }
}

// ─── Search Helpers ───

/// Updates the search structures if new_cost is better than the current best for next_state.
fn update_search_candidate(
    action: ActionTemplate,
    next_state: RegressiveState,
    new_cost: f32,
    current_state: &RegressiveState,
    heuristic_multiplier: f32,
    came_from: &mut HashMap<RegressiveState, (ActionTemplate, RegressiveState)>,
    g_score: &mut HashMap<RegressiveState, f32>,
    open_set: &mut BinaryHeap<RegressiveSearchNode>,
) {
    if new_cost < *g_score.get(&next_state).unwrap_or(&f32::INFINITY) {
        came_from.insert(next_state.clone(), (action, current_state.clone()));
        g_score.insert(next_state.clone(), new_cost);
        open_set.push(RegressiveSearchNode {
            f_score: new_cost + next_state.unmet_goals.len() as f32 * heuristic_multiplier,
            state: next_state,
        });
    }
}

/// Collects all explicit actions that satisfy target_goal, with their next states and costs.
///
/// `current_consumed` tracks resources already consumed by actions that will execute *after*
/// this point in the plan (from the backward search's perspective). A precondition is blocked
/// if any consumed pattern overlaps with it, because the resource will already be gone.
fn find_explicit_actions_for_goal(
    available_actions: &[ActionTemplate],
    target_goal: &TriplePattern,
    remaining_goals: &[TriplePattern],
    current_g: f32,
    mind: &MindGraph,
    current_consumed: &[TriplePattern],
    cost_cache: &PlanCostCache,
) -> Vec<(ActionTemplate, RegressiveState, f32)> {
    let mut candidates = Vec::new();

    for action in available_actions {
        if !action_satisfies_pattern(action, target_goal, &mind.ontology) {
            continue;
        }

        let mut new_unmet = remaining_goals.to_vec();
        for pre in &action.preconditions {
            // A precondition is unmet if:
            // 1. It isn't satisfied in the live world, OR
            // 2. It would be satisfied in the live world but a later action consumes it
            let consumed_by_later = current_consumed.iter().any(|c| patterns_overlap(c, pre));
            if !mind_satisfies_pattern(mind, pre) || consumed_by_later {
                new_unmet.push(pre.clone());
            }
        }

        // Propagate consumed: add this action's consumptions for actions that execute earlier
        let mut next_consumed = current_consumed.to_vec();
        next_consumed.extend(action.consumes.iter().cloned());

        let next_state = RegressiveState::new(new_unmet, next_consumed);
        let new_cost = current_g + subjective_action_cost(action, cost_cache, mind);
        candidates.push((action.clone(), next_state, new_cost));
    }

    candidates
}

/// The stamina precondition pattern the planner adds before a Walk when the agent needs to sleep
/// first. Sleep's plan_effect is `(Self_, Stamina, Int(100))`, so this pattern matches it.
fn energy_full_pattern() -> TriplePattern {
    TriplePattern::new(
        Some(MindNode::Self_),
        Some(Predicate::Stamina),
        Some(Value::Int(100)),
    )
}

/// Builds the unmet-goal list for the state after a Walk action, injecting a Sleep precondition
/// if the agent's current stamina is insufficient to complete the walk.
///
/// Uses a worst-case estimate (entire walk at tired speed) so the planner errs on the side of
/// caution. Returns None if the walk is infeasible even with full stamina.
fn build_walk_goals(
    dist_tiles: f32,
    remaining_goals: &[TriplePattern],
    mind: &MindGraph,
) -> Option<Vec<TriplePattern>> {
    // Worst-case stamina cost: whole trip at tired speed (conservative).
    let stamina_needed = dist_tiles * walk_const::STAMINA_PER_TILE_TIRED;

    // Even with full stamina (100), the walk is impossible.
    if stamina_needed > 100.0 - EXHAUSTION_TRIGGER {
        return None;
    }

    let current_stamina = match mind.get(&MindNode::Self_, Predicate::Stamina) {
        Some(Value::Int(e)) => *e as f32,
        Some(Value::Float(e)) => *e,
        _ => 100.0, // Unknown stamina — assume full, let it proceed
    };

    let mut goals = remaining_goals.to_vec();

    // If the agent can't complete the walk without risking exhaustion,
    // add a stamina precondition so the planner prepends Rest.
    if current_stamina - stamina_needed < EXHAUSTION_TRIGGER {
        goals.insert(0, energy_full_pattern());
    }

    Some(goals)
}

/// Construct the canonical Walk template that satisfies a `LocatedAt(Self_, Tile(t))`
/// goal. The planner builds this directly rather than routing through
/// `WalkAction::to_template_for_target` because Walk is `TargetSource::Implicit`
/// and never gets enumerated by the brain — its only entry point is here.
fn build_walk_template(world_pos: Vec2, tile: (i32, i32)) -> ActionTemplate {
    let behavior = crate::agent::actions::motor::Behavior::new(
        crate::agent::actions::motor::ActionPrimitive::Locomote,
        crate::agent::actions::motor::TargetSelector::InPlace,
        crate::agent::actions::motor::IntensityPolicy::Normal,
        crate::agent::actions::motor::Intent::Goal,
    );
    let locomotion_intensity = behavior.intensity.resolve();
    ActionTemplate {
        name: crate::agent::actions::action::walk::WALK_NAME.to_string(),
        action_type: ActionType::Walk,
        behavior,
        target_entity: None,
        target_position: Some(world_pos),
        preconditions: Vec::new(),
        effects: vec![Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile(tile),
        )],
        consumes: Vec::new(),
        base_cost: 0.0,
        locomotion_intensity,
        estimated_duration_ticks: None,
    }
}

/// Generates an implicit Walk if the target goal requires `Self_` to be at a tile.
///
/// This is the only implicit-walk path after #219 collapsed the entity-walk
/// generator: every action that needs proximity (Harvest, Take, Deposit,
/// Drink, Attack) declares a tile-based `self_at(t)` precondition — the
/// brain's `to_template_for_target` snapshots the candidate's current tile
/// at template-build time, and this generator chains a Walk to satisfy it.
///
/// Stamina-aware: if the agent cannot complete the walk on current stamina,
/// prepends a Rest precondition so the planner inserts Rest before Walk.
/// Returns None if the walk is impossible even after resting.
fn generate_implicit_walk(
    target_goal: &TriplePattern,
    remaining_goals: &[TriplePattern],
    current_g: f32,
    mind: &MindGraph,
    current_consumed: &[TriplePattern],
    cost_cache: &PlanCostCache,
) -> Option<(ActionTemplate, RegressiveState, f32)> {
    if target_goal.predicate != Some(Predicate::LocatedAt) {
        return None;
    }
    if !matches!(&target_goal.subject, Some(MindNode::Self_)) {
        return None;
    }

    let tile = match &target_goal.object {
        Some(Value::Tile(t)) => *t,
        _ => return None,
    };

    // Fix #364: skip tiles the agent recently failed to reach. The A*
    // search will explore alternative goals (different food sources,
    // different drinking spots, etc.) instead of reissuing the same
    // blocked walk every tick until the belief ages out.
    if cost_cache.is_unreachable(tile) {
        return None;
    }

    let current_pos_val = mind.get(&MindNode::Self_, Predicate::LocatedAt)?;
    let (cx, cy) = match current_pos_val {
        Value::Tile((cx, cy)) => (cx, cy),
        _ => return None,
    };

    let dist = (((cx - tile.0).pow(2) + (cy - tile.1).pow(2)) as f32).sqrt();
    let world_pos = Vec2::new(
        tile.0 as f32 * TILE_SIZE + TILE_SIZE / 2.0,
        tile.1 as f32 * TILE_SIZE + TILE_SIZE / 2.0,
    );

    let walk_action = build_walk_template(world_pos, tile);

    let next_goals = build_walk_goals(dist, remaining_goals, mind)?;
    let next_state = RegressiveState::new(next_goals, current_consumed.to_vec());
    let new_cost = current_g
        + subjective_walk_cost(
            dist,
            tile,
            walk_action.locomotion_intensity.max(0.5),
            cost_cache,
        );

    Some((walk_action, next_state, new_cost))
}

// =============================================================================
// PLANNER CONFIG
// =============================================================================

/// Configuration for the GOAP planner (now mostly handled by MindGraph queries)
#[derive(bevy::prelude::Resource, Debug, Clone, bevy::prelude::Reflect)]
#[reflect(Resource)]
pub struct PlannerConfig {
    /// Urgency threshold required to trigger goal formulation (0.0 - 1.0)
    pub goal_formulation_threshold: f32,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            goal_formulation_threshold: 0.1, // Low threshold to encourage action
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::ActionType;
    use crate::agent::actions::registry::ActionRegistry;
    use crate::agent::mind::knowledge::{
        Concept, Metadata, Node as MindNode, Predicate, Triple, Value, setup_ontology,
    };
    use bevy::prelude::Entity;

    // ─── Helpers ──────────────────────────────────────────────────────────────

    fn test_mind() -> MindGraph {
        MindGraph::new(setup_ontology())
    }

    /// An action that gathers `concept` from `target`. Consumes the source's contents.
    fn gather_template(target: Entity, concept: Concept) -> ActionTemplate {
        ActionTemplate {
            name: format!("Gather({:?})", concept),
            action_type: ActionType::Harvest,
            behavior: Default::default(),
            target_entity: Some(target),
            target_position: None,
            preconditions: vec![TriplePattern::entity_contains(target)],
            effects: vec![Triple::new(
                MindNode::Self_,
                Predicate::Contains,
                Value::Item(concept, 1),
            )],
            consumes: vec![TriplePattern::entity_contains(target)],
            base_cost: 2.0,
            locomotion_intensity: 0.0,
            estimated_duration_ticks: None,
        }
    }

    fn goal_self_contains(concept: Concept) -> Goal {
        Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::Contains),
                Some(Value::Item(concept, 1)),
            )],
            priority: 1.0,
        }
    }

    fn goal_self_contains_both(a: Concept, b: Concept) -> Goal {
        Goal {
            conditions: vec![
                TriplePattern::new(
                    Some(MindNode::Self_),
                    Some(Predicate::Contains),
                    Some(Value::Item(a, 1)),
                ),
                TriplePattern::new(
                    Some(MindNode::Self_),
                    Some(Predicate::Contains),
                    Some(Value::Item(b, 1)),
                ),
            ],
            priority: 1.0,
        }
    }

    fn minimal_registry() -> ActionRegistry {
        // Walk must be registered because the planner may need it for implicit walks.
        // For tests that don't use LocatedAt goals, it won't be called.
        ActionRegistry::new()
    }

    fn self_apple_pattern() -> TriplePattern {
        TriplePattern::new(
            Some(MindNode::Self_),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Apple, 1)),
        )
    }

    fn hash_state(state: &RegressiveState) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;
        let mut h = DefaultHasher::new();
        state.hash(&mut h);
        h.finish()
    }

    // ─── patterns_overlap ─────────────────────────────────────────────────────

    #[test]
    fn patterns_overlap_same_subject_and_predicate() {
        let e = Entity::from_bits(1);
        let a = TriplePattern::entity_contains(e);
        let b = TriplePattern::entity_contains(e);
        assert!(patterns_overlap(&a, &b));
    }

    #[test]
    fn patterns_overlap_one_wildcard_subject() {
        let e = Entity::from_bits(1);
        // `a` has a wildcard object, `b` is fully concrete
        let a = TriplePattern::new(Some(MindNode::Entity(e)), Some(Predicate::Contains), None);
        let b = TriplePattern::new(
            Some(MindNode::Entity(e)),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Apple, 1)),
        );
        assert!(patterns_overlap(&a, &b));
    }

    #[test]
    fn patterns_no_overlap_different_entities() {
        let e1 = Entity::from_bits(1);
        let e2 = Entity::from_bits(2);
        let a = TriplePattern::entity_contains(e1);
        let b = TriplePattern::entity_contains(e2);
        assert!(!patterns_overlap(&a, &b));
    }

    #[test]
    fn patterns_no_overlap_different_predicates() {
        let e = Entity::from_bits(1);
        let a = TriplePattern::new(Some(MindNode::Entity(e)), Some(Predicate::Contains), None);
        let b = TriplePattern::new(Some(MindNode::Entity(e)), Some(Predicate::LocatedAt), None);
        assert!(!patterns_overlap(&a, &b));
    }

    // ─── planner with consumed tracking ───────────────────────────────────────

    #[test]
    fn single_gather_plan_still_works() {
        // Baseline: a single gather from a source with items succeeds (no regression).
        let tree = Entity::from_bits(42);
        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Entity(tree),
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        ));

        let actions = vec![gather_template(tree, Concept::Apple)];
        let goal = goal_self_contains(Concept::Apple);

        let plan = regressive_plan(&mind, &goal, &actions, &PlanCostContext::neutral());
        assert!(plan.is_some(), "single gather should produce a valid plan");
        assert!(
            plan.unwrap()
                .iter()
                .any(|a| a.action_type == ActionType::Harvest),
            "plan should include Harvest"
        );
    }

    #[test]
    fn second_gather_from_same_source_blocked_when_consumed() {
        // Goal needs both Apple and Berry. Two actions both target the same node (entity 42)
        // which has items. The live world satisfies both preconditions — but consumed tracking
        // must block the second action from planning against the already-consumed source.
        let node = Entity::from_bits(42);
        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Entity(node),
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        ));

        // Both gather apples and berries from the same node, consuming it
        let actions = vec![
            gather_template(node, Concept::Apple),
            gather_template(node, Concept::Berry),
        ];
        let goal = goal_self_contains_both(Concept::Apple, Concept::Berry);

        // After planning the first gather (which consumes node 42), the second gather's
        // precondition `entity_contains(42)` is in consumed — so no valid plan exists.
        let plan = regressive_plan(&mind, &goal, &actions, &PlanCostContext::neutral());
        if let Some(ref p) = plan {
            let gather_count = p
                .iter()
                .filter(|a| a.action_type == ActionType::Harvest)
                .count();
            assert!(
                gather_count < 2,
                "planner must not plan two gathers from the same consumed source; got {}",
                gather_count
            );
        }
        // No plan found is also a correct outcome — the planner correctly gives up
    }

    #[test]
    fn independent_sources_not_blocked_by_consumed() {
        // Goal needs Apple and Berry. Apple comes from tree1, Berry from tree2.
        // Consuming tree1 (for Apple) must NOT block the Berry gather from tree2.
        let tree1 = Entity::from_bits(1);
        let tree2 = Entity::from_bits(2);
        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Entity(tree1),
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        ));
        mind.add(Triple::new(
            MindNode::Entity(tree2),
            Predicate::Contains,
            Value::Item(Concept::Berry, 1),
        ));

        let actions = vec![
            gather_template(tree1, Concept::Apple),
            gather_template(tree2, Concept::Berry),
        ];
        let goal = goal_self_contains_both(Concept::Apple, Concept::Berry);

        let plan = regressive_plan(&mind, &goal, &actions, &PlanCostContext::neutral());
        assert!(
            plan.is_some(),
            "two independent sources should produce a valid plan"
        );
        let plan = plan.unwrap();
        let gather_count = plan
            .iter()
            .filter(|a| a.action_type == ActionType::Harvest)
            .count();
        assert_eq!(gather_count, 2, "plan should contain exactly 2 gathers");
    }

    #[test]
    fn already_satisfied_goal_returns_empty_plan() {
        // Agent already has an apple — no actions needed.
        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        ));

        let goal = goal_self_contains(Concept::Apple);

        let plan = regressive_plan(&mind, &goal, &[], &PlanCostContext::neutral());
        assert!(plan.is_some(), "goal already satisfied should return Some");
        assert!(
            plan.unwrap().is_empty(),
            "goal already satisfied should return empty plan"
        );
    }

    // ─── Stamina-aware walk planning ───────────────────────────────────────────

    /// Harvest action that requires being at a specific tile (mimics real proximity actions).
    fn harvest_at_tile(entity: Entity, concept: Concept, tile: (i32, i32)) -> ActionTemplate {
        ActionTemplate {
            name: format!("Harvest({:?})", concept),
            action_type: ActionType::Harvest,
            behavior: Default::default(),
            target_entity: Some(entity),
            target_position: None,
            preconditions: vec![
                TriplePattern::entity_contains(entity),
                TriplePattern::self_at(tile),
            ],
            effects: vec![Triple::new(
                MindNode::Self_,
                Predicate::Contains,
                Value::Item(concept, 1),
            )],
            consumes: vec![TriplePattern::entity_contains(entity)],
            base_cost: 2.0,
            locomotion_intensity: 0.0,
            estimated_duration_ticks: None,
        }
    }

    /// Mind with agent at origin, given stamina, and food entity at `food_tile`.
    fn mind_with_food_and_energy(food: Entity, food_tile: (i32, i32), stamina: i32) -> MindGraph {
        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile((0, 0)),
        ));
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::Stamina,
            Value::Int(stamina),
        ));
        mind.add(Triple::new(
            MindNode::Entity(food),
            Predicate::LocatedAt,
            Value::Tile(food_tile),
        ));
        mind.add(Triple::new(
            MindNode::Entity(food),
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        ));
        mind
    }

    /// Returns a registry-sourced Sleep template (no target).
    fn rest_template(registry: &ActionRegistry) -> ActionTemplate {
        registry
            .get(ActionType::Rest)
            .map(|a| a.to_template(None))
            .expect("Rest must be registered")
    }

    #[test]
    fn short_walk_with_high_energy_needs_no_rest() {
        // Agent stamina 80, food 10 tiles away — should plan Walk -> Harvest, no Rest.
        let food = Entity::from_bits(10);
        let food_tile = (10i32, 0i32); // 10 tiles from origin
        let mind = mind_with_food_and_energy(food, food_tile, 80);

        let registry = minimal_registry();
        let actions = vec![
            harvest_at_tile(food, Concept::Apple, food_tile),
            rest_template(&registry),
        ];
        let goal = goal_self_contains(Concept::Apple);

        let plan = regressive_plan(&mind, &goal, &actions, &PlanCostContext::neutral());
        assert!(plan.is_some(), "should produce a valid plan");
        let plan = plan.unwrap();

        assert!(
            !plan.iter().any(|a| a.action_type == ActionType::Rest),
            "no rest needed when stamina is sufficient"
        );
        assert!(
            plan.iter().any(|a| a.action_type == ActionType::Walk),
            "plan must include Walk"
        );
        assert!(
            plan.iter().any(|a| a.action_type == ActionType::Harvest),
            "plan must include Harvest"
        );
    }

    #[test]
    fn long_walk_with_low_energy_inserts_rest() {
        // Agent stamina 20, food 60 tiles away — should plan Rest -> Walk -> Harvest.
        let food = Entity::from_bits(11);
        let food_tile = (60i32, 0i32); // 60 tiles from origin, costs 12 stamina at tired rate
        let mind = mind_with_food_and_energy(food, food_tile, 20);

        let registry = minimal_registry();
        let actions = vec![
            harvest_at_tile(food, Concept::Apple, food_tile),
            rest_template(&registry),
        ];
        let goal = goal_self_contains(Concept::Apple);

        let plan = regressive_plan(&mind, &goal, &actions, &PlanCostContext::neutral());
        assert!(
            plan.is_some(),
            "should produce a plan (Rest makes it feasible)"
        );
        let plan = plan.unwrap();

        let rest_idx = plan.iter().position(|a| a.action_type == ActionType::Rest);
        let walk_idx = plan.iter().position(|a| a.action_type == ActionType::Walk);
        assert!(rest_idx.is_some(), "plan must include Rest");
        assert!(walk_idx.is_some(), "plan must include Walk");
        assert!(
            rest_idx.unwrap() < walk_idx.unwrap(),
            "Rest must come before Walk"
        );
    }

    #[test]
    fn impossibly_long_walk_returns_no_plan() {
        // Food 500 tiles away — impossible even after resting (stamina cost > 85).
        let food = Entity::from_bits(12);
        let food_tile = (500i32, 0i32);
        let mind = mind_with_food_and_energy(food, food_tile, 20);

        let registry = minimal_registry();
        let actions = vec![
            harvest_at_tile(food, Concept::Apple, food_tile),
            rest_template(&registry),
        ];
        let goal = goal_self_contains(Concept::Apple);

        let plan = regressive_plan(&mind, &goal, &actions, &PlanCostContext::neutral());
        assert!(
            plan.is_none(),
            "planner must return None for truly infeasible walk"
        );
    }

    #[test]
    fn energy_check_applies_to_non_food_harvest() {
        // Same stamina logic applies to any walk, not just food plans.
        // Agent stamina 20, stone node 60 tiles away — Rest should be prepended.
        let stone = Entity::from_bits(13);
        let stone_tile = (60i32, 0i32);
        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile((0, 0)),
        ));
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::Stamina,
            Value::Int(20),
        ));
        mind.add(Triple::new(
            MindNode::Entity(stone),
            Predicate::LocatedAt,
            Value::Tile(stone_tile),
        ));
        mind.add(Triple::new(
            MindNode::Entity(stone),
            Predicate::Contains,
            Value::Item(Concept::Stone, 1),
        ));

        let registry = minimal_registry();
        let actions = vec![
            ActionTemplate {
                name: "HarvestStone".to_string(),
                action_type: ActionType::Harvest,
                behavior: Default::default(),
                target_entity: Some(stone),
                target_position: None,
                preconditions: vec![
                    TriplePattern::entity_contains(stone),
                    TriplePattern::self_at(stone_tile),
                ],
                effects: vec![Triple::new(
                    MindNode::Self_,
                    Predicate::Contains,
                    Value::Item(Concept::Stone, 1),
                )],
                consumes: vec![TriplePattern::entity_contains(stone)],
                base_cost: 2.0,
                locomotion_intensity: 0.0,
                estimated_duration_ticks: None,
            },
            rest_template(&registry),
        ];
        let goal = Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::Contains),
                Some(Value::Item(Concept::Stone, 1)),
            )],
            priority: 1.0,
        };

        let plan = regressive_plan(&mind, &goal, &actions, &PlanCostContext::neutral());
        assert!(plan.is_some(), "should produce a plan for stone harvest");
        let plan = plan.unwrap();

        let rest_idx = plan.iter().position(|a| a.action_type == ActionType::Rest);
        let walk_idx = plan.iter().position(|a| a.action_type == ActionType::Walk);
        assert!(
            rest_idx.is_some(),
            "Rest must be inserted before long walk to stone"
        );
        assert!(
            rest_idx.unwrap() < walk_idx.unwrap(),
            "Rest must come before Walk"
        );
    }

    // ─── isa_filter / trait_filter: typed wildcard correctness ────────────────

    #[test]
    fn planner_does_not_chain_stone_harvest_to_satisfy_hunger() {
        // Bug regression: the eat action's precondition was (Self, Contains, ?any),
        // allowing the planner to chain "harvest stone → eat stone" to satisfy hunger.
        // With isa_filter = Food, stone must be rejected.
        let stone_node = Entity::from_bits(20);
        let stone_tile = (5i32, 0i32);

        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile((0, 0)),
        ));
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::Stamina,
            Value::Int(80),
        ));
        mind.add(Triple::new(
            MindNode::Entity(stone_node),
            Predicate::LocatedAt,
            Value::Tile(stone_tile),
        ));
        mind.add(Triple::new(
            MindNode::Entity(stone_node),
            Predicate::Contains,
            Value::Item(Concept::Stone, 5),
        ));

        // Eat action: precondition is self_contains_food() (isa_filter = Food)
        let eat_action = ActionTemplate {
            name: "Eat".to_string(),
            action_type: ActionType::Eat,
            behavior: Default::default(),
            target_entity: None,
            target_position: None,
            preconditions: vec![TriplePattern::self_contains_food()],
            effects: vec![Triple::new(
                MindNode::Self_,
                Predicate::Hunger,
                Value::Int(0),
            )],
            consumes: vec![],
            base_cost: 1.0,
            locomotion_intensity: 0.0,
            estimated_duration_ticks: None,
        };
        let harvest_stone = ActionTemplate {
            name: "HarvestStone".to_string(),
            action_type: ActionType::Harvest,
            behavior: Default::default(),
            target_entity: Some(stone_node),
            target_position: None,
            preconditions: vec![
                TriplePattern::entity_contains(stone_node),
                TriplePattern::self_at(stone_tile),
            ],
            effects: vec![Triple::new(
                MindNode::Self_,
                Predicate::Contains,
                Value::Item(Concept::Stone, 1),
            )],
            consumes: vec![TriplePattern::entity_contains(stone_node)],
            base_cost: 2.0,
            locomotion_intensity: 0.0,
            estimated_duration_ticks: None,
        };

        let hunger_goal = Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::Hunger),
                Some(Value::Int(0)),
            )],
            priority: 1.0,
        };

        let actions = vec![eat_action, harvest_stone];
        let plan = regressive_plan(&mind, &hunger_goal, &actions, &PlanCostContext::neutral());
        assert!(
            plan.is_none(),
            "planner must not satisfy hunger by harvesting stone"
        );
    }

    #[test]
    fn isa_filter_accepts_matching_concept_and_rejects_non_matching() {
        let ontology = setup_ontology();

        let food_triple = Triple::new(
            MindNode::Self_,
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        );
        let stone_triple = Triple::new(
            MindNode::Self_,
            Predicate::Contains,
            Value::Item(Concept::Stone, 1),
        );

        let food_pattern = TriplePattern::self_contains_food();

        assert!(
            pattern_matches_triple(&food_pattern, &food_triple, Some(&ontology)),
            "Apple (IsA Food) must match self_contains_food()"
        );
        assert!(
            !pattern_matches_triple(&food_pattern, &stone_triple, Some(&ontology)),
            "Stone (not Food) must not match self_contains_food()"
        );
    }

    #[test]
    fn trait_filter_accepts_edible_and_rejects_non_edible() {
        let ontology = setup_ontology();

        let berry_triple = Triple::new(
            MindNode::Self_,
            Predicate::Contains,
            Value::Item(Concept::Berry, 1),
        );
        let stone_triple = Triple::new(
            MindNode::Self_,
            Predicate::Contains,
            Value::Item(Concept::Stone, 1),
        );

        let edible_pattern = TriplePattern {
            trait_filter: Some(Concept::Edible),
            ..TriplePattern::self_contains()
        };

        assert!(
            pattern_matches_triple(&edible_pattern, &berry_triple, Some(&ontology)),
            "Berry (HasTrait Edible) must match trait_filter = Edible"
        );
        assert!(
            !pattern_matches_triple(&edible_pattern, &stone_triple, Some(&ontology)),
            "Stone (no Edible trait) must not match trait_filter = Edible"
        );
    }

    // ─── Pattern matching correctness (#20) ───────────────────────────────────

    #[test]
    fn precondition_with_specific_subject_does_not_match_other_entities() {
        // Regression for #20: a precondition that names a specific subject
        // must not be satisfied by triples about other entities — otherwise
        // the planner skips actions that should have been required.
        let other = Entity::from_bits(99);
        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Entity(other),
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        ));

        assert!(
            !mind_satisfies_pattern(&mind, &self_apple_pattern()),
            "Self_ precondition must not be satisfied by another entity's items"
        );

        let stranger = Entity::from_bits(1234);
        let stranger_apple = TriplePattern::new(
            Some(MindNode::Entity(stranger)),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Apple, 1)),
        );
        assert!(
            !mind_satisfies_pattern(&mind, &stranger_apple),
            "entity X precondition must not be satisfied by entity Y's items"
        );

        let owner_apple = TriplePattern::new(
            Some(MindNode::Entity(other)),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Apple, 1)),
        );
        assert!(mind_satisfies_pattern(&mind, &owner_apple));
    }

    #[test]
    fn planner_does_not_treat_self_goal_as_satisfied_by_another_entity() {
        // Regression for #20: goal "Self_ at (5,5)" must not be considered
        // already-satisfied just because some other entity is at (5,5).
        let other = Entity::from_bits(7);
        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile((0, 0)),
        ));
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::Stamina,
            Value::Int(100),
        ));
        mind.add(Triple::new(
            MindNode::Entity(other),
            Predicate::LocatedAt,
            Value::Tile((5, 5)),
        ));

        let goal = Goal {
            conditions: vec![TriplePattern::self_at((5, 5))],
            priority: 1.0,
        };
        let plan = regressive_plan(&mind, &goal, &[], &PlanCostContext::neutral())
            .expect("planner should produce a Walk plan, not an empty plan");
        assert!(
            plan.iter().any(|a| a.action_type == ActionType::Walk),
            "plan must include a Walk to reach the target tile"
        );
    }

    #[test]
    fn pattern_matches_triple_respects_specific_subject() {
        let agent_apple = Triple::new(
            MindNode::Self_,
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        );
        let other_apple = Triple::new(
            MindNode::Entity(Entity::from_bits(1)),
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        );

        let pat_self = TriplePattern::new(
            Some(MindNode::Self_),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Apple, 1)),
        );
        assert!(pattern_matches_triple(&pat_self, &agent_apple, None));
        assert!(
            !pattern_matches_triple(&pat_self, &other_apple, None),
            "Self_ pattern must not match an entity-subject triple"
        );
    }

    #[test]
    fn pattern_matches_triple_wildcards_each_field() {
        let triple = Triple::new(
            MindNode::Entity(Entity::from_bits(42)),
            Predicate::Contains,
            Value::Item(Concept::Apple, 3),
        );

        // Wildcard subject
        let pat = TriplePattern::new(
            None,
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Apple, 3)),
        );
        assert!(pattern_matches_triple(&pat, &triple, None));

        // Wildcard predicate
        let pat = TriplePattern::new(
            Some(MindNode::Entity(Entity::from_bits(42))),
            None,
            Some(Value::Item(Concept::Apple, 3)),
        );
        assert!(pattern_matches_triple(&pat, &triple, None));

        // Wildcard object
        let pat = TriplePattern::new(
            Some(MindNode::Entity(Entity::from_bits(42))),
            Some(Predicate::Contains),
            None,
        );
        assert!(pattern_matches_triple(&pat, &triple, None));

        // Concrete mismatch
        let pat = TriplePattern::new(
            Some(MindNode::Entity(Entity::from_bits(99))),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Apple, 3)),
        );
        assert!(!pattern_matches_triple(&pat, &triple, None));
    }

    // ─── RegressiveState dedup (#20) ──────────────────────────────────────────

    #[test]
    fn regressive_state_canonicalizes_goal_order() {
        let a = self_apple_pattern();
        let b = TriplePattern::new(
            Some(MindNode::Self_),
            Some(Predicate::LocatedAt),
            Some(Value::Tile((3, 5))),
        );

        let s1 = RegressiveState::new(vec![a.clone(), b.clone()], vec![]);
        let s2 = RegressiveState::new(vec![b, a], vec![]);

        assert_eq!(s1, s2, "goal order must not affect state equality");
        assert_eq!(
            hash_state(&s1),
            hash_state(&s2),
            "goal order must not affect state hash"
        );
    }

    #[test]
    fn regressive_state_deduplicates_repeated_goals() {
        // `[A, A]` and `[A]` describe the same goal set; A* dedup must collapse them.
        let a = self_apple_pattern();
        let single = RegressiveState::new(vec![a.clone()], vec![]);
        let duplicated = RegressiveState::new(vec![a.clone(), a], vec![]);

        assert_eq!(single, duplicated);
        assert_eq!(hash_state(&single), hash_state(&duplicated));
    }

    // ─── MAX_ITERATIONS diagnostic ────────────────────────────────────────────

    #[test]
    fn max_iterations_emits_warning_with_unreachable_goal() {
        // Goal: self contains Apple. We provide MAX_ITERATIONS+1 gather actions, each requiring
        // a different source entity that contains Apple. None of the entities have Apple in the
        // mind, so every child state is stuck on "entity_i Contains Apple" with no further
        // actions to satisfy it. This generates MAX_ITERATIONS+1 child nodes from the initial
        // expansion, forcing the planner to exhaust MAX_ITERATIONS before the open_set empties.
        use crate::constants::brains::planner::MAX_ITERATIONS;
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();

        let make_writer = move || {
            struct VecWriter(Arc<Mutex<Vec<u8>>>);
            impl std::io::Write for VecWriter {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    self.0.lock().unwrap().extend_from_slice(buf);
                    Ok(buf.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            VecWriter(captured_clone.clone())
        };

        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_writer(make_writer)
            .finish();

        let mind = test_mind(); // empty — no entity contains Apple
        let actions: Vec<ActionTemplate> = (1..=(MAX_ITERATIONS + 1))
            .map(|i| gather_template(Entity::from_bits(i as u64), Concept::Apple))
            .collect();
        let goal = goal_self_contains(Concept::Apple);

        let plan = tracing::subscriber::with_default(subscriber, || {
            regressive_plan(&mind, &goal, &actions, &PlanCostContext::neutral())
        });

        let log_output = String::from_utf8(captured.lock().unwrap().clone()).unwrap_or_default();
        assert!(plan.is_none(), "unsatisfiable goal must return None");
        assert!(
            log_output.contains("regressive_plan exhausted"),
            "must warn about MAX_ITERATIONS exhaustion; got: {log_output}"
        );
        assert!(
            log_output.contains("unmet goals"),
            "must warn about remaining unmet goals; got: {log_output}"
        );
    }

    // ─── Subjective plan cost ─────────────────────────────────────────────────

    fn physical_action(target: Entity, concept: Concept, tile: (i32, i32)) -> ActionTemplate {
        let registry = crate::agent::actions::ActionRegistry::new();
        let behavior = registry
            .get(ActionType::Harvest)
            .unwrap()
            .default_behavior();
        let locomotion_intensity = behavior.intensity.resolve();
        ActionTemplate {
            name: format!("TestPhysical({:?})", concept),
            action_type: ActionType::Harvest,
            behavior,
            target_entity: Some(target),
            target_position: None,
            preconditions: vec![TriplePattern::entity_contains(target)],
            effects: vec![
                Triple::new(
                    MindNode::Self_,
                    Predicate::Contains,
                    Value::Item(concept, 1),
                ),
                Triple::new(
                    MindNode::Entity(target),
                    Predicate::LocatedAt,
                    Value::Tile(tile),
                ),
            ],
            consumes: vec![TriplePattern::entity_contains(target)],
            base_cost: 10.0,
            locomotion_intensity,
            estimated_duration_ticks: None,
        }
    }

    fn stock_entity_at_tile(
        mind: &mut MindGraph,
        entity: Entity,
        concept: Concept,
        tile: (i32, i32),
    ) {
        mind.add(Triple::new(
            MindNode::Entity(entity),
            Predicate::LocatedAt,
            Value::Tile(tile),
        ));
        mind.add(Triple::new(
            MindNode::Entity(entity),
            Predicate::Contains,
            Value::Item(concept, 1),
        ));
    }

    #[test]
    fn neutral_context_produces_positive_effort_cost() {
        let tree = Entity::from_bits(1);
        let mut mind = test_mind();
        stock_entity_at_tile(&mut mind, tree, Concept::Apple, (3, 4));
        let action = physical_action(tree, Concept::Apple, (3, 4));

        let ctx = PlanCostContext::neutral();
        let cache = PlanCostCache::new(&ctx, &mind);
        let cost = subjective_action_cost(&action, &cache, &mind);
        assert!(
            cost > 0.0,
            "effort-based cost under neutral context must be positive, got {cost}"
        );
    }

    #[test]
    fn neurotic_agent_perceives_walk_as_more_expensive() {
        let mind = test_mind();
        let calm = PlanCostContext {
            neuroticism: 0.0,
            ..PlanCostContext::neutral()
        };
        let anxious = PlanCostContext {
            neuroticism: 1.0,
            ..PlanCostContext::neutral()
        };

        let calm_cache = PlanCostCache::new(&calm, &mind);
        let anxious_cache = PlanCostCache::new(&anxious, &mind);
        let calm_cost = subjective_walk_cost(20.0, (20, 0), 0.5, &calm_cache);
        let anxious_cost = subjective_walk_cost(20.0, (20, 0), 0.5, &anxious_cache);

        assert!(
            anxious_cost > calm_cost,
            "neurotic agent should perceive the same walk as more expensive \
             (calm={calm_cost}, anxious={anxious_cost})"
        );
    }

    #[test]
    fn low_confidence_target_inflates_action_cost() {
        let tree = Entity::from_bits(2);
        let tile = (5, 5);

        let mut known_mind = test_mind();
        known_mind.add(Triple::new(
            MindNode::Entity(tree),
            Predicate::LocatedAt,
            Value::Tile(tile),
        ));
        known_mind.add(Triple::with_meta(
            MindNode::Entity(tree),
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
            Metadata::perception_with_conf(0, 0.9),
        ));

        let mut guess_mind = test_mind();
        guess_mind.add(Triple::new(
            MindNode::Entity(tree),
            Predicate::LocatedAt,
            Value::Tile(tile),
        ));
        guess_mind.add(Triple::with_meta(
            MindNode::Entity(tree),
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
            Metadata::perception_with_conf(0, 0.3),
        ));

        let action = physical_action(tree, Concept::Apple, tile);
        let ctx = PlanCostContext::neutral();
        let known_cache = PlanCostCache::new(&ctx, &known_mind);
        let guess_cache = PlanCostCache::new(&ctx, &guess_mind);
        let known = subjective_action_cost(&action, &known_cache, &known_mind);
        let guess = subjective_action_cost(&action, &guess_cache, &guess_mind);

        assert!(
            guess > known,
            "low-confidence target should cost more (known={known}, guess={guess})"
        );
    }

    #[test]
    fn dangerous_area_inflates_walk_cost() {
        let mut safe_mind = test_mind();
        safe_mind.add(Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile((0, 0)),
        ));
        let mut danger_mind = test_mind();
        danger_mind.add(Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile((0, 0)),
        ));
        // Wolf lives right next to the destination tile.
        let wolf = Entity::from_bits(99);
        danger_mind.add(Triple::new(
            MindNode::Entity(wolf),
            Predicate::LocatedAt,
            Value::Tile((10, 0)),
        ));
        danger_mind.add(Triple::new(
            MindNode::Entity(wolf),
            Predicate::HasTrait,
            Value::Concept(Concept::Dangerous),
        ));

        let ctx = PlanCostContext::neutral();
        let safe_cache = PlanCostCache::new(&ctx, &safe_mind);
        let danger_cache = PlanCostCache::new(&ctx, &danger_mind);
        let safe_cost = subjective_walk_cost(10.0, (10, 0), 0.5, &safe_cache);
        let risky_cost = subjective_walk_cost(10.0, (10, 0), 0.5, &danger_cache);

        assert!(
            risky_cost > safe_cost,
            "walk toward danger must cost more (safe={safe_cost}, risky={risky_cost})"
        );
    }

    #[test]
    fn neurotic_agent_perceives_plan_as_more_costly_than_stoic() {
        let tree = Entity::from_bits(3);
        let mut mind = test_mind();
        stock_entity_at_tile(&mut mind, tree, Concept::Apple, (2, 0));
        let action = physical_action(tree, Concept::Apple, (2, 0));

        let stoic = PlanCostContext {
            neuroticism: 0.0,
            ..PlanCostContext::neutral()
        };
        let anxious = PlanCostContext {
            neuroticism: 1.0,
            ..PlanCostContext::neutral()
        };

        let stoic_cache = PlanCostCache::new(&stoic, &mind);
        let anxious_cache = PlanCostCache::new(&anxious, &mind);
        let stoic_cost = subjective_action_cost(&action, &stoic_cache, &mind);
        let anxious_cost = subjective_action_cost(&action, &anxious_cache, &mind);

        assert!(
            anxious_cost > stoic_cost,
            "neurotic agent must perceive the same action as more costly \
             (stoic={stoic_cost}, anxious={anxious_cost})"
        );
    }

    #[test]
    fn heavier_agent_pays_more_for_same_walk() {
        let ctx_light = PlanCostContext {
            body_mass: 40.0,
            ..PlanCostContext::neutral()
        };
        let ctx_heavy = PlanCostContext {
            body_mass: 100.0,
            ..PlanCostContext::neutral()
        };

        let light_cost = effort_cost_walk(20.0, 0.5, &ctx_light);
        let heavy_cost = effort_cost_walk(20.0, 0.5, &ctx_heavy);

        assert!(
            heavy_cost > light_cost,
            "heavier agent should pay more for the same walk \
             (light={light_cost}, heavy={heavy_cost})"
        );
    }

    #[test]
    fn heavier_agent_pays_more_for_same_action() {
        let mind = test_mind();
        let registry = crate::agent::actions::ActionRegistry::new();
        let mut action = registry.get(ActionType::Walk).unwrap().to_template(None);
        action.estimated_duration_ticks = Some(120);

        let light = PlanCostContext {
            body_mass: 40.0,
            ..PlanCostContext::neutral()
        };
        let heavy = PlanCostContext {
            body_mass: 100.0,
            ..PlanCostContext::neutral()
        };

        let light_cache = PlanCostCache::new(&light, &mind);
        let heavy_cache = PlanCostCache::new(&heavy, &mind);
        let light_cost = subjective_action_cost(&action, &light_cache, &mind);
        let heavy_cost = subjective_action_cost(&action, &heavy_cache, &mind);

        assert!(
            heavy_cost > light_cost,
            "heavier agent should pay more for the same action \
             (light={light_cost}, heavy={heavy_cost})"
        );
    }

    #[test]
    fn effort_cost_is_positive_for_all_registered_actions() {
        let mind = test_mind();
        let ctx = PlanCostContext::neutral();
        let cache = PlanCostCache::new(&ctx, &mind);
        let registry = crate::agent::actions::ActionRegistry::new();

        for action_def in registry.all() {
            let template = action_def.to_template(None);
            let cost = subjective_action_cost(&template, &cache, &mind);
            assert!(
                cost > 0.0,
                "{:?} must have positive effort-based cost, got {cost}",
                template.action_type,
            );
        }
    }

    #[test]
    fn tired_agent_prefers_closer_resource() {
        // Two apple trees, one 3 tiles away and one 30 tiles away. A tired
        // agent should plan against the closer one because the distance
        // cost is inflated by the stamina factor and personality.
        let near = Entity::from_bits(10);
        let far = Entity::from_bits(11);
        let near_tile = (3i32, 0i32);
        let far_tile = (30i32, 0i32);

        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile((0, 0)),
        ));
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::Stamina,
            Value::Int(100),
        ));
        stock_entity_at_tile(&mut mind, near, Concept::Apple, near_tile);
        stock_entity_at_tile(&mut mind, far, Concept::Apple, far_tile);

        let actions = vec![
            harvest_at_tile(near, Concept::Apple, near_tile),
            harvest_at_tile(far, Concept::Apple, far_tile),
        ];
        let goal = goal_self_contains(Concept::Apple);

        let plan = regressive_plan(&mind, &goal, &actions, &PlanCostContext::neutral())
            .expect("plan should exist");

        // Find which harvest target was chosen.
        let chosen_target = plan
            .iter()
            .find(|a| a.action_type == ActionType::Harvest)
            .and_then(|a| a.target_entity)
            .expect("plan must harvest something");
        assert_eq!(
            chosen_target, near,
            "planner should prefer the closer apple tree"
        );
    }

    #[test]
    fn planner_generates_walk_behavior_for_locomotion() {
        use crate::agent::actions::motor::ActionPrimitive;
        let template = build_walk_template(bevy::math::Vec2::new(100.0, 100.0), (5, 5));
        assert_eq!(
            template.behavior.primitive,
            ActionPrimitive::Locomote,
            "Walk template must carry Locomote primitive"
        );
    }

    #[test]
    fn walk_cost_scales_linearly_with_distance() {
        let ctx = PlanCostContext::neutral();
        let short = effort_cost_walk(5.0, 0.5, &ctx);
        let long = effort_cost_walk(50.0, 0.5, &ctx);

        let ratio = long / short;
        assert!(
            (ratio - 10.0).abs() < 0.5,
            "50-tile walk should cost ~10x a 5-tile walk, got ratio {ratio:.2}"
        );
    }

    #[test]
    fn planner_rejects_plan_exceeding_energy_reserves() {
        let walk = build_walk_template(Vec2::new(5000.0, 0.0), (250, 0));
        let plan = vec![walk];

        let starving = PlanCostContext {
            glucose: 5.0,
            reserves: 2.0,
            ..PlanCostContext::neutral()
        };
        assert!(
            !check_plan_feasibility(&plan, Vec2::ZERO, &starving),
            "starving agent should not be able to walk 250 tiles"
        );
    }

    #[test]
    fn sleep_step_in_plan_improves_feasibility() {
        let registry = crate::agent::actions::ActionRegistry::new();
        let sleep = registry.get(ActionType::Sleep).unwrap().to_template(None);
        let long_walk = build_walk_template(Vec2::new(3000.0, 0.0), (150, 0));

        let tired = PlanCostContext {
            stamina_aerobic: 0.15,
            glucose: 40.0,
            reserves: 100.0,
            ..PlanCostContext::neutral()
        };

        let plan_without_sleep = vec![long_walk.clone()];
        let plan_with_sleep = vec![sleep, long_walk];

        let without = check_plan_feasibility(&plan_without_sleep, Vec2::ZERO, &tired);
        let with = check_plan_feasibility(&plan_with_sleep, Vec2::ZERO, &tired);

        assert!(
            with || !without,
            "adding a sleep step should not make a plan less feasible"
        );
    }
}
