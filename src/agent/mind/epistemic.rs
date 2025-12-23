use crate::agent::brains::thinking::TriplePattern;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use bevy::prelude::*;

/// Types of knowledge an agent might seek
#[derive(Debug, Clone, PartialEq, Eq, Reflect)]
pub enum EpistemicGoal {
    KnowLocationOf(Concept), // "Where is food?"
    KnowItemAt(Entity),      // "What is in this container?"
}

/// Detect what knowledge is missing for a goal
pub fn identify_knowledge_gap(goal: &TriplePattern, mind: &MindGraph) -> Option<EpistemicGoal> {
    // Case 1: Goal is "Have Item(X)" but we don't know where X is
    if is_possession_goal(goal) {
        if let Some(Value::Int(_)) = goal.object {
            // It's a "Have Resource" goal (e.g. Hunger -> Have Food)
            // We need to know where the resource is located
            // But wait, the goal itself doesn't specify the concept clearly if it's just "Hunger=0"
            // Actually, the RationalBrain turns Hunger=0 into "Have Food"
            // Let's assume the goal passed here is the PRECONDITION for the plan
            // e.g. "Have Apple"
        }
    }

    // Better approach: Look at the pattern specifically

    // If pattern is (Self, Contains, Item(Concept, N))
    if goal.predicate == Some(Predicate::Contains) && goal.subject == Some(Node::Self_) {
        if let Some(Value::Item(concept, _)) = goal.object {
            // We want to have 'concept'. Do we know where to find it?
            let known_locations = mind.query(
                None, // Any subject (the item source)
                Some(Predicate::Contains),
                Some(&Value::Item(concept, 1)), // Contains at least 1
            );

            // Filter for things that aren't us
            let sources: Vec<_> = known_locations
                .into_iter()
                .filter(|t| t.subject != Node::Self_)
                .collect();

            if sources.is_empty() {
                // We don't know where this item is!
                return Some(EpistemicGoal::KnowLocationOf(concept));
            }
        }
    }

    None
}

fn is_possession_goal(pattern: &TriplePattern) -> bool {
    pattern.subject == Some(Node::Self_) && pattern.predicate == Some(Predicate::Contains)
}
