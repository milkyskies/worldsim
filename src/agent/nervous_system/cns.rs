use super::urgency::{Urgency, UrgencySource};
use crate::agent::brains::planner::PlannerConfig;
use crate::agent::brains::thinking::{Goal, TriplePattern};
use crate::agent::mind::knowledge::{Predicate, Value};
use bevy::prelude::*;

#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct CentralNervousSystem {
    pub urgencies: Vec<Urgency>,
    pub current_goal: Option<Goal>,
}

/// Formulates goals based on the highest urgency
/// This is the "Decision Layer" - it decides WHAT to do, not HOW to do it.
pub fn formulate_goals(
    mut query: Query<(
        &mut CentralNervousSystem,
        &crate::agent::psyche::personality::Personality,
        &crate::agent::mind::knowledge::MindGraph,
    )>,
    config: Res<PlannerConfig>,
) {
    for (mut cns, _personality, _mind) in query.iter_mut() {
        // Find highest urgency
        cns.urgencies.sort_by(|a, b| {
            b.value
                .partial_cmp(&a.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(top_urgency) = cns.urgencies.first() {
            // Threshold to care
            if top_urgency.value < config.goal_formulation_threshold {
                cns.current_goal = None;
                continue;
            }

            // Map urgency to a Goal Fact (Desired State)
            let conditions = match top_urgency.source {
                UrgencySource::Hunger => {
                    vec![TriplePattern::self_has(Predicate::Hunger, Value::Int(0))]
                }
                UrgencySource::Energy => {
                    vec![TriplePattern::self_has(Predicate::Energy, Value::Int(100))]
                }
                UrgencySource::Social => vec![TriplePattern::self_has(
                    Predicate::SocialDrive,
                    Value::Int(0),
                )],
                UrgencySource::Pain => {
                    vec![TriplePattern::self_has(Predicate::Pain, Value::Int(0))]
                }
                // Placeholder for logic not yet fully implemented
                // For now, map others to empty conditions which implicitly means "do nothing specific" or "satisfied"
                UrgencySource::Thirst
                | UrgencySource::Fun
                | UrgencySource::Boredom
                | UrgencySource::Fear => vec![],
            };

            // Set the new goal
            cns.current_goal = Some(Goal {
                conditions,
                priority: top_urgency.value,
            });
        }
    }
}
