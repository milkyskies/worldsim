//! Central nervous system: formulates goals from the highest-urgency drive and tracks the current goal.
//!
//! Reads: Urgency list (from urgency module), Personality, MindGraph, Commitments, PlannerConfig
//! Writes: CentralNervousSystem (urgencies, current_goal)
//! Upstream: nervous_system::urgency (produces Urgency values), mind::knowledge (MindGraph), commitment (Commitments)
//! Downstream: brain_system (reads current_goal for rational planning)

use super::urgency::{Urgency, UrgencySource};
use crate::agent::brains::planner::PlannerConfig;
use crate::agent::brains::thinking::{Goal, TriplePattern};
use crate::agent::commitment::Commitments;
use crate::agent::mind::knowledge::{Predicate, Value};
use crate::agent::psyche::personality::Personality;
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
        &Personality,
        &crate::agent::mind::knowledge::MindGraph,
        Option<&Commitments>,
    )>,
    config: Res<PlannerConfig>,
) {
    for (mut cns, personality, _mind, commitments) in query.iter_mut() {
        // Find highest urgency
        cns.urgencies.sort_by(|a, b| {
            b.value
                .partial_cmp(&a.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let urgency_goal = cns.urgencies.first().and_then(|top| {
            if top.value < config.goal_formulation_threshold {
                return None;
            }

            let conditions = match top.source {
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
                UrgencySource::Thirst => {
                    vec![TriplePattern::self_has(Predicate::Thirst, Value::Int(0))]
                }
                UrgencySource::Fun
                | UrgencySource::Boredom
                | UrgencySource::Fear
                | UrgencySource::Territoriality => vec![],
            };

            Some(Goal {
                conditions,
                priority: top.value,
            })
        });

        // Commitment goal: if the agent has committed to a concept, build a goal
        // from the strongest commitment. The priority is scaled by strength and
        // conscientiousness — reliable agents hold commitments tighter.
        let commitment_goal = commitments
            .and_then(|c| c.strongest())
            .map(|commitment| Goal {
                conditions: vec![TriplePattern::self_has(
                    Predicate::Contains,
                    Value::Item(commitment.goal, 1),
                )],
                priority: commitment.priority(personality.traits.conscientiousness),
            });

        // Pick whichever goal has higher priority. Committed goals beat ambient
        // drives they outweigh, but cannot override life-threatening urgencies.
        cns.current_goal = match (urgency_goal, commitment_goal) {
            (Some(u), Some(c)) => Some(if c.priority > u.priority { c } else { u }),
            (Some(u), None) => Some(u),
            (None, Some(c)) => Some(c),
            (None, None) => None,
        };
    }
}
