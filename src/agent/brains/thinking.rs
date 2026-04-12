//! Thinking primitives: TriplePattern for MindGraph queries, Goal and ActionTemplate types for planning.
//!
//! Reads: MindGraph triples (via pattern matching), ActionType
//! Writes: TriplePattern, Goal, ActionTemplate (used by brains to express desired states and actions)
//! Upstream: mind::knowledge (MindGraph, Triple, Node, Predicate, Value)
//! Downstream: all brain systems, belief_state, nervous_system::cns (goal formulation)

use crate::agent::actions::ActionType;
use crate::agent::actions::motor::Behavior;
use crate::agent::mind::knowledge::{Concept, Node, Predicate, Triple, Value};
use bevy::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// TRIPLE PATTERN — For matching against MindGraph
// ═══════════════════════════════════════════════════════════════════════════

/// A pattern that can match against Triples in MindGraph.
/// None = wildcard (match any)
#[derive(Debug, Clone, PartialEq, Reflect, Default)]
pub struct TriplePattern {
    pub subject: Option<Node>,
    pub predicate: Option<Predicate>,
    pub object: Option<Value>,
    /// When set, a `Value::Item` in the object position must satisfy `IsA <concept>`
    /// in the ontology. Used to express "contains a Food item" without enumerating
    /// every edible concept. Checked by both mind and action satisfiability functions.
    pub isa_filter: Option<Concept>,
    /// When set, a `Value::Item` in the object position must satisfy `HasTrait <concept>`
    /// in the ontology. Complements `isa_filter` — use whichever is more natural for
    /// the constraint (e.g. `Edible` vs `Food`). Both filters AND together if both are set.
    pub trait_filter: Option<Concept>,
}

impl TriplePattern {
    pub fn new(s: Option<Node>, p: Option<Predicate>, o: Option<Value>) -> Self {
        Self {
            subject: s,
            predicate: p,
            object: o,
            isa_filter: None,
            trait_filter: None,
        }
    }

    /// Common pattern: (Self_, Predicate, Value)
    pub fn self_has(p: Predicate, v: Value) -> Self {
        Self::new(Some(Node::Self_), Some(p), Some(v))
    }

    /// Pattern for checking entity location
    pub fn entity_at(entity: Entity, tile: (i32, i32)) -> Self {
        Self::new(
            Some(Node::Entity(entity)),
            Some(Predicate::LocatedAt),
            Some(Value::Tile(tile)),
        )
    }

    /// Pattern for self at location
    pub fn self_at(tile: (i32, i32)) -> Self {
        Self::new(
            Some(Node::Self_),
            Some(Predicate::LocatedAt),
            Some(Value::Tile(tile)),
        )
    }

    /// Pattern for entity containing items
    pub fn entity_contains(entity: Entity) -> Self {
        Self::new(
            Some(Node::Entity(entity)),
            Some(Predicate::Contains),
            None, // Match any contents
        )
    }

    /// Pattern for self containing items
    pub fn self_contains() -> Self {
        Self::new(Some(Node::Self_), Some(Predicate::Contains), None)
    }

    /// Pattern for self containing an edible (Food) item.
    /// The `isa_filter` restricts matching to items whose concept `IsA Food`,
    /// so the planner will not chain "harvest stone → eat" to satisfy hunger.
    pub fn self_contains_food() -> Self {
        Self {
            isa_filter: Some(Concept::Food),
            ..Self::new(Some(Node::Self_), Some(Predicate::Contains), None)
        }
    }

    /// Check if self is awake (high alertness)
    pub fn self_awake() -> Self {
        Self::self_has(Predicate::Stamina, Value::Int(1)) // Placeholder - actual check is more complex
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ACTION TEMPLATE — For GOAP planner
// ═══════════════════════════════════════════════════════════════════════════

/// A template for an action the planner can use.
#[derive(Debug, Clone, Reflect)]
pub struct ActionTemplate {
    pub name: String,
    pub action_type: ActionType,
    pub behavior: Behavior,
    pub target_entity: Option<Entity>,
    pub target_position: Option<Vec2>,
    /// Patterns that must match in MindGraph for action to be valid
    pub preconditions: Vec<TriplePattern>,
    /// Concrete triples to assert when action completes
    pub effects: Vec<Triple>,
    /// Patterns removed from the world when this action executes (destructive effects).
    /// The planner uses this to track resource depletion during backward search,
    /// preventing it from generating plans that rely on the same finite resource twice.
    pub consumes: Vec<TriplePattern>,
    pub base_cost: f32,
    /// Resolved locomotion intensity in [0, 1] for Movement-class actions (#339).
    /// Derived from `behavior.intensity` at template creation; the brain
    /// overrides it at admission time with urgency-scaled resolution.
    pub locomotion_intensity: f32,
}

/// A goal the agent wants to achieve.
#[derive(Debug, Clone, Reflect)]
pub struct Goal {
    /// Patterns that must all be satisfied for goal to be complete
    pub conditions: Vec<TriplePattern>,
    pub priority: f32,
}

impl Goal {
    /// Extract the concept-level target of a resource-acquisition goal.
    /// Returns the first `Item` concept referenced by any condition — this
    /// is the thing the agent is pursuing (Apple, Campfire, ...). Drive-based
    /// goals (hunger, thirst, ...) have no concept target and return `None`.
    pub fn target_concept(&self) -> Option<crate::agent::mind::knowledge::Concept> {
        use crate::agent::mind::knowledge::Value;
        self.conditions.iter().find_map(|pattern| {
            if let Some(Value::Item(concept, _)) = pattern.object {
                Some(concept)
            } else {
                None
            }
        })
    }
}

// Custom equality: goals are equal if their CONDITIONS are the same
// Priority changes frequently but shouldn't reset the plan
impl PartialEq for Goal {
    fn eq(&self, other: &Self) -> bool {
        self.conditions == other.conditions
    }
}

// NOTE: Static ActionTemplate methods (wake_up, wander, explore, etc.) have been removed.
// Actions are now defined via the Action trait and accessed through ActionRegistry.
// Use: action_registry.get(ActionType::Wander).map(|a| a.to_template(None))
