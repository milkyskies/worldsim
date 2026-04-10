//! GOAP regressive planner: backward A* search from goal to initial state.
//!
//! Reads: MindGraph (world state), available ActionTemplates, Goal conditions
//! Writes: Vec<ActionTemplate> (ordered plan)
//! Upstream: rational brain (goal + actions), mind (MindGraph)
//! Downstream: rational brain (executes the plan)

use super::thinking::{ActionTemplate, Goal, TriplePattern};
use crate::agent::actions::ActionType;
use crate::agent::mind::knowledge::{MindGraph, Node as MindNode, Predicate, Triple, Value};
use crate::constants::actions::walk as walk_const;
use crate::constants::brains::survival::EXHAUSTION_TRIGGER;
use crate::world::map::TILE_SIZE;
use bevy::prelude::*;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;

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
fn pattern_matches_triple(pattern: &TriplePattern, triple: &Triple) -> bool {
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
pub fn regressive_plan(
    mind: &MindGraph,
    goal: &Goal,
    available_actions: &[ActionTemplate],
) -> Option<Vec<ActionTemplate>> {
    use crate::constants::brains::planner::{HEURISTIC_MULTIPLIER, MAX_ITERATIONS};
    let start_time = std::time::Instant::now();
    let mut iterations = 0;

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
    results.into_iter().any(|triple| match &triple.object {
        Value::Item(_, qty) => *qty > 0,
        _ => true,
    })
}

fn action_satisfies_pattern(action: &ActionTemplate, pattern: &TriplePattern) -> bool {
    // Action satisfies pattern if one of its effects matches the pattern
    for effect in &action.effects {
        if pattern_matches_triple(pattern, effect) {
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
) -> Vec<(ActionTemplate, RegressiveState, f32)> {
    let mut candidates = Vec::new();

    for action in available_actions {
        if !action_satisfies_pattern(action, target_goal) {
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
        let new_cost = current_g + action.base_cost;
        candidates.push((action.clone(), next_state, new_cost));
    }

    candidates
}

/// The energy precondition pattern the planner adds before a Walk when the agent needs to sleep
/// first. Sleep's plan_effect is `(Self_, Energy, Int(100))`, so this pattern matches it.
fn energy_full_pattern() -> TriplePattern {
    TriplePattern::new(
        Some(MindNode::Self_),
        Some(Predicate::Energy),
        Some(Value::Int(100)),
    )
}

/// Builds the unmet-goal list for the state after a Walk action, injecting a Sleep precondition
/// if the agent's current energy is insufficient to complete the walk.
///
/// Uses a worst-case estimate (entire walk at tired speed) so the planner errs on the side of
/// caution. Returns None if the walk is infeasible even with full energy.
fn build_walk_goals(
    dist_tiles: f32,
    remaining_goals: &[TriplePattern],
    mind: &MindGraph,
) -> Option<Vec<TriplePattern>> {
    // Worst-case energy cost: whole trip at tired speed (conservative).
    let energy_needed = dist_tiles * walk_const::ENERGY_PER_TILE_TIRED;

    // Even with full energy (100), the walk is impossible.
    if energy_needed > 100.0 - EXHAUSTION_TRIGGER {
        return None;
    }

    let current_energy = match mind.get(&MindNode::Self_, Predicate::Energy) {
        Some(Value::Int(e)) => *e as f32,
        Some(Value::Float(e)) => *e,
        _ => 100.0, // Unknown energy — assume full, let it proceed
    };

    let mut goals = remaining_goals.to_vec();

    // If the agent can't complete the walk without risking exhaustion sleep-interruption,
    // add an energy precondition so the planner prepends Sleep.
    if current_energy - energy_needed < EXHAUSTION_TRIGGER {
        goals.insert(0, energy_full_pattern());
    }

    Some(goals)
}

/// Construct the canonical Walk template that satisfies a `LocatedAt(Self_, Tile(t))`
/// goal. The planner builds this directly rather than routing through
/// `WalkAction::to_template_for_target` because Walk is `TargetSource::Implicit`
/// and never gets enumerated by the brain — its only entry point is here.
fn build_walk_template(world_pos: Vec2, tile: (i32, i32)) -> ActionTemplate {
    ActionTemplate {
        name: crate::agent::actions::action::walk::WALK_NAME.to_string(),
        action_type: ActionType::Walk,
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
/// Energy-aware: if the agent cannot complete the walk on current energy,
/// prepends a Sleep precondition so the planner inserts Sleep before Walk.
/// Returns None if the walk is impossible even after sleeping.
fn generate_implicit_walk(
    target_goal: &TriplePattern,
    remaining_goals: &[TriplePattern],
    current_g: f32,
    mind: &MindGraph,
    current_consumed: &[TriplePattern],
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
    let new_cost = current_g + dist;

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
        Concept, Node as MindNode, Predicate, Triple, Value, setup_ontology,
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

        let plan = regressive_plan(&mind, &goal, &actions);
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
        let plan = regressive_plan(&mind, &goal, &actions);
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

        let plan = regressive_plan(&mind, &goal, &actions);
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

        let plan = regressive_plan(&mind, &goal, &[]);
        assert!(plan.is_some(), "goal already satisfied should return Some");
        assert!(
            plan.unwrap().is_empty(),
            "goal already satisfied should return empty plan"
        );
    }

    // ─── Energy-aware walk planning ───────────────────────────────────────────

    /// Harvest action that requires being at a specific tile (mimics real proximity actions).
    fn harvest_at_tile(entity: Entity, concept: Concept, tile: (i32, i32)) -> ActionTemplate {
        ActionTemplate {
            name: format!("Harvest({:?})", concept),
            action_type: ActionType::Harvest,
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
        }
    }

    /// Mind with agent at origin, given energy, and food entity at `food_tile`.
    fn mind_with_food_and_energy(food: Entity, food_tile: (i32, i32), energy: i32) -> MindGraph {
        let mut mind = test_mind();
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::LocatedAt,
            Value::Tile((0, 0)),
        ));
        mind.add(Triple::new(
            MindNode::Self_,
            Predicate::Energy,
            Value::Int(energy),
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
    fn sleep_template(registry: &ActionRegistry) -> ActionTemplate {
        registry
            .get(ActionType::Sleep)
            .map(|a| a.to_template(None))
            .expect("Sleep must be registered")
    }

    #[test]
    fn short_walk_with_high_energy_needs_no_sleep() {
        // Agent energy 80, food 10 tiles away — should plan Walk → Harvest, no Sleep.
        let food = Entity::from_bits(10);
        let food_tile = (10i32, 0i32); // 10 tiles from origin
        let mind = mind_with_food_and_energy(food, food_tile, 80);

        let registry = minimal_registry();
        let actions = vec![
            harvest_at_tile(food, Concept::Apple, food_tile),
            sleep_template(&registry),
        ];
        let goal = goal_self_contains(Concept::Apple);

        let plan = regressive_plan(&mind, &goal, &actions);
        assert!(plan.is_some(), "should produce a valid plan");
        let plan = plan.unwrap();

        assert!(
            !plan.iter().any(|a| a.action_type == ActionType::Sleep),
            "no sleep needed when energy is sufficient"
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
    fn long_walk_with_low_energy_inserts_sleep() {
        // Agent energy 20, food 60 tiles away — should plan Sleep → Walk → Harvest.
        let food = Entity::from_bits(11);
        let food_tile = (60i32, 0i32); // 60 tiles from origin, costs 12 energy at tired rate
        let mind = mind_with_food_and_energy(food, food_tile, 20);

        let registry = minimal_registry();
        let actions = vec![
            harvest_at_tile(food, Concept::Apple, food_tile),
            sleep_template(&registry),
        ];
        let goal = goal_self_contains(Concept::Apple);

        let plan = regressive_plan(&mind, &goal, &actions);
        assert!(
            plan.is_some(),
            "should produce a plan (Sleep makes it feasible)"
        );
        let plan = plan.unwrap();

        let sleep_idx = plan.iter().position(|a| a.action_type == ActionType::Sleep);
        let walk_idx = plan.iter().position(|a| a.action_type == ActionType::Walk);
        assert!(sleep_idx.is_some(), "plan must include Sleep");
        assert!(walk_idx.is_some(), "plan must include Walk");
        assert!(
            sleep_idx.unwrap() < walk_idx.unwrap(),
            "Sleep must come before Walk"
        );
    }

    #[test]
    fn impossibly_long_walk_returns_no_plan() {
        // Food 500 tiles away — impossible even after sleeping (energy cost > 85).
        let food = Entity::from_bits(12);
        let food_tile = (500i32, 0i32);
        let mind = mind_with_food_and_energy(food, food_tile, 20);

        let registry = minimal_registry();
        let actions = vec![
            harvest_at_tile(food, Concept::Apple, food_tile),
            sleep_template(&registry),
        ];
        let goal = goal_self_contains(Concept::Apple);

        let plan = regressive_plan(&mind, &goal, &actions);
        assert!(
            plan.is_none(),
            "planner must return None for truly infeasible walk"
        );
    }

    #[test]
    fn energy_check_applies_to_non_food_harvest() {
        // Same energy logic applies to any walk, not just food plans.
        // Agent energy 20, stone node 60 tiles away — Sleep should be prepended.
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
            Predicate::Energy,
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
            },
            sleep_template(&registry),
        ];
        let goal = Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::Contains),
                Some(Value::Item(Concept::Stone, 1)),
            )],
            priority: 1.0,
        };

        let plan = regressive_plan(&mind, &goal, &actions);
        assert!(plan.is_some(), "should produce a plan for stone harvest");
        let plan = plan.unwrap();

        let sleep_idx = plan.iter().position(|a| a.action_type == ActionType::Sleep);
        let walk_idx = plan.iter().position(|a| a.action_type == ActionType::Walk);
        assert!(
            sleep_idx.is_some(),
            "Sleep must be inserted before long walk to stone"
        );
        assert!(
            sleep_idx.unwrap() < walk_idx.unwrap(),
            "Sleep must come before Walk"
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
            Predicate::Energy,
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
        let plan = regressive_plan(&mind, &goal, &[])
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
        assert!(pattern_matches_triple(&pat_self, &agent_apple));
        assert!(
            !pattern_matches_triple(&pat_self, &other_apple),
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
        assert!(pattern_matches_triple(&pat, &triple));

        // Wildcard predicate
        let pat = TriplePattern::new(
            Some(MindNode::Entity(Entity::from_bits(42))),
            None,
            Some(Value::Item(Concept::Apple, 3)),
        );
        assert!(pattern_matches_triple(&pat, &triple));

        // Wildcard object
        let pat = TriplePattern::new(
            Some(MindNode::Entity(Entity::from_bits(42))),
            Some(Predicate::Contains),
            None,
        );
        assert!(pattern_matches_triple(&pat, &triple));

        // Concrete mismatch
        let pat = TriplePattern::new(
            Some(MindNode::Entity(Entity::from_bits(99))),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Apple, 3)),
        );
        assert!(!pattern_matches_triple(&pat, &triple));
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
            regressive_plan(&mind, &goal, &actions)
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
}
