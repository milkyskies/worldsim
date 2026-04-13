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
    /// Estimated duration in ticks for effort-based cost estimation.
    /// `Some(n)` for timed actions with known duration; `None` for
    /// Movement actions (duration depends on distance) and indefinite
    /// actions (Sleep, Idle, Construct with `u32::MAX`).
    pub estimated_duration_ticks: Option<u32>,
    /// Concept filter for `LookFor`-style goal-directed search. Flows
    /// from the brain's fallback proposal through execution dispatch
    /// into `ActionState` and `LegCompleteContext` so the target picker
    /// can bias chunk selection. `None` for every non-search action.
    pub search_filter: Option<SearchFilter>,
}

/// Concept/trait filter for goal-directed search actions.
///
/// Mirrors `TriplePattern::isa_filter` / `trait_filter` in a compact form
/// that can be stored on `ActionTemplate`/`ActionState` without dragging
/// the whole pattern shape onto the runtime state. `LookForAction`
/// consumes one of these via its `LegCompleteContext` to pick targets
/// biased toward chunks with matching `Produces` hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Reflect)]
pub struct SearchFilter {
    pub isa: Option<Concept>,
    pub trait_: Option<Concept>,
}

impl SearchFilter {
    pub fn concept(isa: Concept) -> Self {
        Self {
            isa: Some(isa),
            trait_: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.isa.is_none() && self.trait_.is_none()
    }

    /// Human-readable short label for logs and reasoning strings.
    pub fn describe(&self) -> String {
        if let Some(c) = self.isa {
            format!("{:?}", c)
        } else if let Some(t) = self.trait_ {
            format!("trait {:?}", t)
        } else {
            "something".to_string()
        }
    }
}

impl ActionTemplate {
    /// Escalate the behavior's intensity based on drive urgency (0-1).
    ///
    /// A starving agent sprints to food; a mildly hungry one walks.
    /// Updates both `behavior.intensity` and the cached
    /// `locomotion_intensity` scalar.
    pub fn escalate_intensity(&mut self, urgency: f32) {
        self.behavior.intensity = self
            .behavior
            .intensity
            .clone()
            .escalate_for_urgency(urgency);
        self.locomotion_intensity = self.behavior.intensity.resolve();
    }
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

/// Walk one planning step backward from a goal to find the concept the
/// agent should be searching for when no concrete instance is known.
///
/// For each action in the registry, check whether any of its planning
/// effects satisfies any condition on `goal`. If so, look at that
/// action's preconditions for a `TriplePattern` that carries an
/// `isa_filter` / `trait_filter` — those are the "look for something
/// matching this concept" expressions the planner already uses (see
/// `TriplePattern::self_contains_food`).
///
/// Returns a `SearchFilter` built from the first matching action's
/// filter. `None` when no action satisfies the goal, or when the
/// satisfier has no concept filter at all (meaning "there's nothing
/// specific to search for" — e.g. `RestAction` acts on self-state, not
/// on a findable resource).
pub fn derive_search_concept(
    goal: &Goal,
    registry: &crate::agent::actions::ActionRegistry,
) -> Option<SearchFilter> {
    let mut actions: Vec<&dyn crate::agent::actions::registry::Action> = registry.all().collect();
    actions.sort_by_key(|a| a.action_type() as usize);

    for action in actions {
        let effects = action.plan_effects();
        if effects.is_empty() {
            continue;
        }
        let satisfies_goal = effects.iter().any(|effect| {
            goal.conditions
                .iter()
                .any(|cond| goal_condition_matches_effect(cond, effect))
        });
        if !satisfies_goal {
            continue;
        }
        for pre in action.preconditions() {
            if pre.isa_filter.is_some() || pre.trait_filter.is_some() {
                return Some(SearchFilter {
                    isa: pre.isa_filter,
                    trait_: pre.trait_filter,
                });
            }
        }
    }
    None
}

/// True when the concrete `effect` satisfies the goal `cond`. Goal
/// conditions rarely carry `isa_filter`/`trait_filter` (they're
/// `self_has` patterns produced by `goal_for_urgency`), so this is the
/// subject/predicate/object portion of the planner's
/// `pattern_matches_triple`. If a future goal ever uses ontology
/// filters, plumb the MindGraph through here and delegate to the
/// planner's version with an ontology.
fn goal_condition_matches_effect(cond: &TriplePattern, effect: &Triple) -> bool {
    if let Some(s) = &cond.subject
        && &effect.subject != s
    {
        return false;
    }
    if let Some(p) = cond.predicate
        && effect.predicate != p
    {
        return false;
    }
    if let Some(o) = &cond.object
        && &effect.object != o
    {
        return false;
    }
    true
}

#[cfg(test)]
mod derive_search_concept_tests {
    use super::*;
    use crate::agent::actions::{ActionRegistry, action};

    #[test]
    fn derive_search_concept_chases_eat_precondition_to_food() {
        // Eat's plan_effect is (Self, Hunger, 0) and its precondition is
        // self_contains_food (isa_filter = Food). A hunger goal must
        // resolve to a Food search via one-step-back introspection.
        let mut registry = ActionRegistry::default();
        registry.register(action::EatAction);

        let goal = Goal {
            conditions: vec![TriplePattern::self_has(Predicate::Hunger, Value::Int(0))],
            priority: 1.0,
        };

        let result = derive_search_concept(&goal, &registry);
        assert_eq!(result, Some(SearchFilter::concept(Concept::Food)));
    }

    #[test]
    fn derive_search_concept_returns_none_for_drives_without_isa_filter() {
        // Rest has a (Self, Stamina, 100) effect but its precondition
        // has no concept filter — it's a self-state action, not a
        // resource-acquisition one. Derive must return None so the
        // Rational fallback skips this drive instead of proposing a
        // useless search.
        let mut registry = ActionRegistry::default();
        registry.register(action::RestAction);

        let goal = Goal {
            conditions: vec![TriplePattern::self_has(Predicate::Stamina, Value::Int(100))],
            priority: 1.0,
        };

        let result = derive_search_concept(&goal, &registry);
        assert!(
            result.is_none(),
            "drives whose satisfier has no isa_filter must not trigger LookFor; got {result:?}"
        );
    }

    #[test]
    fn derive_search_concept_returns_none_when_no_action_satisfies_goal() {
        // Registry with an unrelated action (Wander has no effects).
        // A hunger goal has nothing that matches, so derive returns None.
        let mut registry = ActionRegistry::default();
        registry.register(action::WanderAction);

        let goal = Goal {
            conditions: vec![TriplePattern::self_has(Predicate::Hunger, Value::Int(0))],
            priority: 1.0,
        };

        assert!(derive_search_concept(&goal, &registry).is_none());
    }
}
