use super::thinking::{ActionTemplate, Goal, TriplePattern};
use crate::agent::actions::ActionType;
use crate::agent::mind::knowledge::{MindGraph, Node as MindNode, Predicate, Triple, Value};
use bevy::prelude::*;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;

// ═══════════════════════════════════════════════════════════════════════════
// PLANNER STATE — Snapshot of MindGraph for A* planning
// ═══════════════════════════════════════════════════════════════════════════

/// A lightweight state representation for the planner.
/// We track only the triples that have been added/modified during planning.
#[derive(Debug, Clone)]
struct PlannerState {
    /// Hash of the base MindGraph (for identity)
    base_hash: u64,
    /// Triples added during planning
    /// We keep them sorted for canonical hashing
    added_triples: Vec<Triple>,
}

impl PartialEq for PlannerState {
    fn eq(&self, other: &Self) -> bool {
        self.base_hash == other.base_hash
            && self.added_triples.len() == other.added_triples.len()
            && self
                .added_triples
                .iter()
                .zip(&other.added_triples)
                .all(|(a, b)| triples_eq(a, b))
    }
}

impl Eq for PlannerState {}

impl std::hash::Hash for PlannerState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.base_hash.hash(state);
        for triple in &self.added_triples {
            hash_triple(triple, state);
        }
    }
}

impl PlannerState {
    fn from_mind(mind: &MindGraph) -> Self {
        Self {
            base_hash: mind.triples.len() as u64, // Simple hash based on triple count
            added_triples: Vec::new(),
        }
    }

    fn with_effects(&self, effects: &[Triple]) -> Self {
        let mut new_state = self.clone();
        for effect in effects {
            // Check if already exists (using our custom eq)
            if !new_state
                .added_triples
                .iter()
                .any(|t| triples_eq(t, effect))
            {
                new_state.added_triples.push(effect.clone());
            }
        }
        // Sort for canonical state (needed for Hashing stability)
        new_state.added_triples.sort_by(compare_triples);
        new_state
    }

    fn check_pattern(&self, mind: &MindGraph, pattern: &TriplePattern) -> bool {
        // First check added triples
        for added in &self.added_triples {
            if pattern_matches_triple(pattern, added) {
                return true;
            }
        }

        // Then check base MindGraph
        !mind
            .query(
                pattern.subject.as_ref(),
                pattern.predicate,
                pattern.object.as_ref(),
            )
            .is_empty()
    }
}

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

fn triples_eq(a: &Triple, b: &Triple) -> bool {
    a.subject == b.subject && a.predicate == b.predicate && a.object == b.object
}

fn compare_triples(a: &Triple, b: &Triple) -> Ordering {
    // Subject -> Predicate -> Object

    // Manual comparison for Node
    let ord = compare_nodes(&a.subject, &b.subject);
    if ord != Ordering::Equal {
        return ord;
    }

    // Predicate is simple enum
    let ord = (a.predicate as usize).cmp(&(b.predicate as usize));
    if ord != Ordering::Equal {
        return ord;
    }

    compare_values(&a.object, &b.object)
}

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

fn hash_triple<H: std::hash::Hasher>(t: &Triple, state: &mut H) {
    t.subject.hash(state);
    t.predicate.hash(state);
    hash_value(&t.object, state);
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
        Value::Text(s) => s.hash(state),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// A* NODE
// ═══════════════════════════════════════════════════════════════════════════

/// A node in the A* open set.
#[derive(Debug, Clone)]
struct SearchNode {
    f_score: f32, // Total estimated cost (g + h)
    state: PlannerState,
}

// Rust's BinaryHeap is a max-heap, so we implement Ord to reverse it for a min-heap.
impl PartialEq for SearchNode {
    fn eq(&self, other: &Self) -> bool {
        self.f_score == other.f_score
    }
}
impl Eq for SearchNode {}
impl PartialOrd for SearchNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SearchNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse order: smaller f_score is better (Greater)
        other.f_score.total_cmp(&self.f_score) // Use total_cmp for floats
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
}

impl PartialEq for RegressiveState {
    fn eq(&self, other: &Self) -> bool {
        self.unmet_goals.len() == other.unmet_goals.len()
            && self
                .unmet_goals
                .iter()
                .zip(&other.unmet_goals)
                .all(|(a, b)| patterns_eq(a, b))
    }
}

impl Eq for RegressiveState {}

impl Hash for RegressiveState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for pattern in &self.unmet_goals {
            hash_pattern(pattern, state);
        }
    }
}

impl RegressiveState {
    fn new(goals: Vec<TriplePattern>) -> Self {
        let mut s = Self { unmet_goals: goals };
        s.normalize();
        s
    }

    /// Sort goals for canonical hashing
    fn normalize(&mut self) {
        self.unmet_goals.sort_by(compare_patterns);
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
    action_registry: &crate::agent::actions::ActionRegistry,
) -> Option<Vec<ActionTemplate>> {
    let start_time = std::time::Instant::now();
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 200;

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

    let start = RegressiveState::new(initial_goals);
    g_score.insert(start.clone(), 0.0);
    open_set.push(RegressiveSearchNode {
        f_score: start.unmet_goals.len() as f32, // Simple heuristic
        state: start,
    });

    let mut result = None;

    while let Some(current_node) = open_set.pop() {
        iterations += 1;
        if iterations > MAX_ITERATIONS {
            break;
        }

        let current_state = current_node.state;

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

        // 1. Check if `target_goal` is ALREADY satisfied in MindGraph (and we just didn't filter it yet?
        // No, we filter on init. But maybe we added it as a precondition?
        if mind_satisfies_pattern(mind, target_goal) {
            // It's satisfied by world state!
            // New state is just remaining goals. Action = None?
            // Wait, "Action = None" doesn't fit `came_from`.
            // Ideally we filter these out immediately when creating the state.
            // Let's assume `RegressiveState::new` cleans up satisfied goals?
            // But checking MindGraph is cheap.
            // If it is satisfied, we just transition to `remaining_goals` with 0 cost.
            // We do need to record the transition path... but maybe we skip recording action?
            // Or better: filter preconditions *before* adding to UnmetGoals.
            // See "Filter Preconditions" below.
        }

        // 2. Find actions that satisfy `target_goal`
        // A. Explicit Actions
        for action in available_actions {
            if action_satisfies_pattern(action, target_goal) {
                // This action is a candidate!
                // New Unmet = (Remaining Goals) + (Action Preconditions)
                // Filter out preconditions that are already true in MindGraph!
                let mut new_unmet = remaining_goals.to_vec();
                for pre in &action.preconditions {
                    if !mind_satisfies_pattern(mind, pre) {
                        new_unmet.push(pre.clone());
                    }
                }

                let next_state = RegressiveState::new(new_unmet);
                let new_cost = current_g + action.base_cost; // Add action cost

                if new_cost < *g_score.get(&next_state).unwrap_or(&f32::INFINITY) {
                    came_from.insert(next_state.clone(), (action.clone(), current_state.clone()));
                    g_score.insert(next_state.clone(), new_cost);
                    open_set.push(RegressiveSearchNode {
                        f_score: new_cost + next_state.unmet_goals.len() as f32 * 5.0,
                        state: next_state,
                    });
                }
            }
        }

        // B. Implicit Movement (The Special Rule)
        // If target_goal is `LocatedAt(X)`, we can generate `WalkTo(X)`.
        if let Some(Predicate::LocatedAt) = target_goal.predicate
            && let Some(MindNode::Self_) = target_goal.subject
        {
            if let Some(Value::Tile(tile)) = target_goal.object {
                // We can walk here!
                // Implicit Action: WalkTo
                // Preconditions: None (always valid to try walking)
                // Cost: Distance from current pos (MindGraph) to target tile.
                if let Some(current_pos_val) = mind.get(&MindNode::Self_, Predicate::LocatedAt)
                    && let Value::Tile((cx, cy)) = current_pos_val
                {
                    let dist = (((cx - tile.0).pow(2) + (cy - tile.1).pow(2)) as f32).sqrt();
                    // FIX: Convert Tile Coords to World Coords
                    // Assuming TILE_SIZE = 16.0 (Default)
                    const TILE_SIZE: f32 = 16.0;
                    let world_pos = Vec2::new(
                        tile.0 as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                        tile.1 as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                    );

                    let walk_action = action_registry
                        .get(ActionType::Walk)
                        .map(|a| a.to_template(None, Some(world_pos)))
                        .expect("Walk action must be registered");

                    // New state: Removing LocatedAt requirement. No new preconditions.
                    let new_unmet = remaining_goals.to_vec();
                    let next_state = RegressiveState::new(new_unmet);
                    let new_cost = current_g + dist;

                    if new_cost < *g_score.get(&next_state).unwrap_or(&f32::INFINITY) {
                        came_from.insert(next_state.clone(), (walk_action, current_state.clone()));
                        g_score.insert(next_state.clone(), new_cost);
                        open_set.push(RegressiveSearchNode {
                            f_score: new_cost + next_state.unmet_goals.len() as f32 * 5.0,
                            state: next_state,
                        });
                    }
                }
            }
            // Handle explicit entity target if needed (e.g. LocatedAt Entity)
            // But mostly our goals use Tiles for movement results.
            if let Some(MindNode::Entity(e)) = target_goal.object.as_ref().map(|v| match v {
                Value::Entity(e) => MindNode::Entity(*e),
                _ => MindNode::Self_, // dummy
            }) && let MindNode::Entity(_) = target_goal
                .object
                .as_ref()
                .unwrap()
                .as_entity()
                .map(MindNode::Entity)
                .unwrap_or(MindNode::Self_)
            {
                // If target is an entity location, we check if we know where it is
                // This logic matches how `rational.rs` makes actions.
                // But we need to construct a robust WalkTo.
                // For now, let's assume interactions define `LocatedAt` using Tiles or specific Entity Predicates.
                // If the goal is `LocatedAt(Entity)`, we need a lookup.
                if let Some(pos_val) = mind.get(&MindNode::Entity(e), Predicate::LocatedAt)
                    && let Value::Tile((tx, ty)) = pos_val
                {
                    // Found it! Generate WalkTo
                    if let Some(current_pos_val) = mind.get(&MindNode::Self_, Predicate::LocatedAt)
                        && let Value::Tile((cx, cy)) = current_pos_val
                    {
                        let dist = (((cx - tx).pow(2) + (cy - ty).pow(2)) as f32).sqrt();
                        const TILE_SIZE: f32 = 16.0;
                        let world_pos = Vec2::new(
                            *tx as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                            *ty as f32 * TILE_SIZE + TILE_SIZE / 2.0,
                        );
                        let walk_action = action_registry
                            .get(ActionType::Walk)
                            .map(|a| a.to_template(Some(e), Some(world_pos)))
                            .expect("Walk action must be registered");

                        let new_unmet = remaining_goals.to_vec();
                        let next_state = RegressiveState::new(new_unmet);
                        let new_cost = current_g + dist;

                        if new_cost < *g_score.get(&next_state).unwrap_or(&f32::INFINITY) {
                            came_from
                                .insert(next_state.clone(), (walk_action, current_state.clone()));
                            g_score.insert(next_state.clone(), new_cost);
                            open_set.push(RegressiveSearchNode {
                                f_score: new_cost + next_state.unmet_goals.len() as f32 * 5.0,
                                state: next_state,
                            });
                        }
                    }
                }
            }
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

// ─── Pattern Hashing Mocks ───

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
