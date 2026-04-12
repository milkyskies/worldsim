//! Central nervous system: formulates goals from the highest-urgency drive and tracks the current goal.
//!
//! Reads: Urgency list (from urgency module), Personality, MindGraph, PlanMemory, PlannerConfig
//! Writes: CentralNervousSystem (urgencies, current_goal)
//! Upstream: nervous_system::urgency (produces Urgency values), mind::knowledge (MindGraph), brains::plan_memory (verbal commitments)
//! Downstream: brain_system (reads current_goal for rational planning)

use super::urgency::{Urgency, UrgencySource};
use crate::agent::brains::plan_memory::{PlanMemory, PlanSource};
use crate::agent::brains::planner::PlannerConfig;
use crate::agent::brains::thinking::{Goal, TriplePattern};
use crate::agent::mind::knowledge::{Predicate, Value};
use crate::agent::psyche::personality::Personality;
use bevy::prelude::*;

#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct CentralNervousSystem {
    pub urgencies: Vec<Urgency>,
    pub current_goal: Option<Goal>,
    /// Set by `generate_urgency` when a drive's raw input has crossed its
    /// `sleep_wake_threshold`. The survival brain reads this to decide
    /// whether a sleeping agent should be roused. Independent from
    /// `urgencies` because it compares against the *pre-gated* input — the
    /// biological wake pathway sees the raw signal, not the alertness-
    /// dampened urgency.
    pub sleep_wake_trigger: Option<UrgencySource>,
}

/// Base priority a verbal-commitment plan contributes to CNS goal
/// selection. Keeps committed goals attractive but not so attractive
/// that they override life-threatening needs like hunger or fear.
pub const VERBAL_COMMITMENT_PRIORITY_BASE: f32 = 0.4;
/// Multiplier on conscientiousness added on top of the base priority —
/// reliable agents hold verbal commitments tighter.
pub const VERBAL_COMMITMENT_PRIORITY_BONUS: f32 = 0.3;

/// Listener-side demand reduction: when the agent's MindGraph already
/// contains a `(?peer, Committed, my_goal_concept)` triple, the goal's
/// priority is multiplied by `1.0 - PEER_COMMITMENT_DISCOUNT`. The
/// other agent has volunteered to handle this concept, so the listener
/// drops their own competing pursuit of it. This is the "5 cold agents
/// build 1 shelter" coordination behaviour from #338.
pub const PEER_COMMITMENT_DISCOUNT: f32 = 0.4;

/// Returns true if any peer in `mind` has a `Committed` triple
/// targeting the given concept. Self-committed triples (where the
/// subject is `Self_`) are ignored — those are the agent's *own*
/// commitments, not peer broadcasts.
fn peer_committed_to(
    mind: &crate::agent::mind::knowledge::MindGraph,
    concept: crate::agent::mind::knowledge::Concept,
) -> bool {
    use crate::agent::mind::knowledge::{Node, Predicate, Value};
    let triples = mind.query(
        None,
        Some(Predicate::Committed),
        Some(&Value::Concept(concept)),
    );
    triples.iter().any(|t| !matches!(t.subject, Node::Self_))
}

/// Formulates goals based on the highest urgency
/// This is the "Decision Layer" - it decides WHAT to do, not HOW to do it.
pub fn formulate_goals(
    mut query: Query<(
        &mut CentralNervousSystem,
        &Personality,
        &crate::agent::mind::knowledge::MindGraph,
        Option<&PlanMemory>,
    )>,
    config: Res<PlannerConfig>,
) {
    for (mut cns, personality, mind, plan_memory) in query.iter_mut() {
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
                UrgencySource::Stamina => {
                    vec![TriplePattern::self_has(Predicate::Stamina, Value::Int(100))]
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
                | UrgencySource::Curiosity
                | UrgencySource::Fear
                | UrgencySource::Territoriality
                | UrgencySource::Sleepiness => vec![],
            };

            Some(Goal {
                conditions,
                priority: top.value,
            })
        });

        // Verbal-commitment goal: promote the strongest background
        // verbal-commitment plan to a full CNS goal so the rational
        // brain picks it up on the next thinking tick. Priority scales
        // with conscientiousness — reliable agents hold commitments
        // tighter — but is deliberately capped below life-threatening
        // drives.
        let commitment_goal = plan_memory.and_then(|memory| {
            memory
                .plans
                .iter()
                .filter(|p| matches!(p.source, PlanSource::VerbalCommitment { .. }))
                .max_by(|a, b| {
                    a.commitment
                        .partial_cmp(&b.commitment)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|plan| Goal {
                    conditions: plan.goal.conditions.clone(),
                    priority: VERBAL_COMMITMENT_PRIORITY_BASE
                        + personality.traits.conscientiousness * VERBAL_COMMITMENT_PRIORITY_BONUS,
                })
        });

        // Pick whichever goal has higher priority. Committed goals beat ambient
        // drives they outweigh, but cannot override life-threatening urgencies.
        let mut chosen = match (urgency_goal, commitment_goal) {
            (Some(u), Some(c)) => Some(if c.priority > u.priority { c } else { u }),
            (Some(u), None) => Some(u),
            (None, Some(c)) => Some(c),
            (None, None) => None,
        };

        // Listener-side demand reduction (#338): if any peer has
        // already publicly committed to the chosen goal's concept,
        // discount its priority. The agent stops competing for the
        // same resource and lets their peer handle it.
        if let Some(goal) = chosen.as_mut()
            && let Some(concept) = goal.target_concept()
            && peer_committed_to(mind, concept)
        {
            goal.priority *= 1.0 - PEER_COMMITMENT_DISCOUNT;
        }

        cns.current_goal = chosen;
    }
}
