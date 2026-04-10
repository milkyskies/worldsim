//! Rational brain: deliberate goal-directed planning via GOAP.
//!
//! Reads: RationalBrain, Consciousness, ItemSlots, MindGraph, VisibleObjects, CentralNervousSystem
//! Writes: RationalBrain (plan/goal), BrainProposal
//! Upstream: cns (current_goal), planner (regressive_plan), mind (MindGraph)
//! Downstream: brains::proposal (winner selection)

use crate::agent::actions::ActionType;
use crate::agent::body::needs::Consciousness;
use crate::agent::brains::proposal::{BrainProposal, BrainType, Intent};
use crate::agent::brains::target_enumeration::enumerate_targets;
use crate::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{MindGraph, Predicate, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::constants::brains::rational::{
    EXPLORE_FALLBACK_PRIORITY_MULTIPLIER, IDLE_WANDER_URGENCY, MIN_ALERTNESS_FOR_PLANNING,
    PLAN_CONTINUATION_URGENCY,
};
use crate::world::map::WorldMap;
use bevy::prelude::*;

#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct RationalBrain {
    #[reflect(ignore)]
    pub current_plan: Option<Vec<ActionTemplate>>,
    #[reflect(ignore)]
    pub current_goal: Option<Goal>,
    #[reflect(ignore)]
    pub plan_index: usize,
}

/// Check if an action's effects are all satisfied in the MindGraph.
fn is_step_complete(action: &ActionTemplate, mind: &MindGraph) -> bool {
    // Empty effects = never auto-completes (like Idle, Curl Up)
    if action.effects.is_empty() {
        return false;
    }

    // All effect triples have matching patterns in mind?
    action.effects.iter().all(|effect| {
        !mind
            .query(
                Some(&effect.subject),
                Some(effect.predicate),
                Some(&effect.object),
            )
            .is_empty()
    })
}

/// Check if action's preconditions are still met
fn are_preconditions_met(action: &ActionTemplate, mind: &MindGraph) -> bool {
    // If no preconditions, always possible
    if action.preconditions.is_empty() {
        return true;
    }

    // All preconditions must be satisfied
    action.preconditions.iter().all(|pre| {
        let subject = pre.subject.as_ref();
        let predicate = pre.predicate;
        let object = pre.object.as_ref();

        let results = mind.query(subject, predicate, object);

        let valid_results: Vec<_> = results
            .into_iter()
            .filter(|triple| match &triple.object {
                Value::Item(_, qty) => *qty > 0,
                _ => true,
            })
            .collect();

        !valid_results.is_empty()
    })
}

pub fn update_rational_brain(
    mut query: Query<(
        Entity,
        &mut RationalBrain,
        &mut crate::agent::brains::proposal::BrainState,
        &Consciousness,
        &ItemSlots,
        &Transform,
        &VisibleObjects,
        &crate::agent::nervous_system::cns::CentralNervousSystem,
        &MindGraph,
    )>,
    tick: Res<crate::core::tick::TickCount>,
    ns_config: Res<crate::agent::nervous_system::config::NervousSystemConfig>,
    _world_map: Res<WorldMap>,
    action_registry: Res<crate::agent::actions::ActionRegistry>,
    mut game_log: ResMut<crate::core::GameLog>,
    affordances: Query<(
        &GlobalTransform,
        Option<&crate::agent::affordance::Affordance>,
    )>,
) {
    let perf_logging = game_log.is_enabled(crate::core::log::LogCategory::Performance);
    let start_time = if perf_logging {
        Some(std::time::Instant::now())
    } else {
        None
    };
    let mut plan_attempts = 0;

    for (
        entity,
        mut brain,
        mut brain_state,
        consciousness,
        _inventory,
        _transform,
        _visible,
        cns,
        mind,
    ) in query.iter_mut()
    {
        // 1. Plan Verification
        let mut plan_finished = false;
        let mut plan_invalid = false;
        let mut explore_found_resources = false;
        let mut should_advance = false;
        let current_plan_len = brain.current_plan.as_ref().map(|p| p.len()).unwrap_or(0);
        let current_index = brain.plan_index;

        if let Some(plan) = &brain.current_plan {
            if current_index < plan.len() {
                let action = &plan[current_index];

                if is_step_complete(action, mind) {
                    should_advance = true;
                    if current_index + 1 >= current_plan_len {
                        plan_finished = true;
                    }
                }

                if !are_preconditions_met(action, mind) {
                    plan_invalid = true;
                }

                if action.action_type == ActionType::Explore {
                    let known_resources = mind.query(None, Some(Predicate::Contains), None);
                    let has_resources = known_resources.iter().any(|triple| {
                        if let crate::agent::mind::knowledge::Value::Item(_, qty) = &triple.object {
                            *qty > 0
                        } else {
                            false
                        }
                    });
                    if has_resources {
                        explore_found_resources = true;
                    }
                }
            } else {
                plan_finished = true;
            }
        }

        if should_advance {
            brain.plan_index += 1;
        }

        if explore_found_resources {
            brain.current_plan = None;
            brain.plan_index = 0;
        } else if plan_finished || plan_invalid {
            brain.current_plan = None;
            brain.current_goal = if plan_invalid {
                None
            } else {
                brain.current_goal.take()
            };
            if plan_invalid {
                // update_rational_brain runs before start_actions (data-conflict ordering).
                // Clearing chosen_actions here prevents start_actions from re-starting the
                // stale action this tick and on subsequent ticks until three_brains_system
                // next fires and re-populates the list with a valid proposal.
                brain_state.chosen_actions.clear();
            }
        }

        // 2. Heavy Thinking (Replanning)
        let should_replan = plan_invalid || tick.should_run(entity, ns_config.thinking_interval);
        if !should_replan {
            continue;
        }

        // CONSCIOUSNESS CHECK: Can't plan while asleep
        if consciousness.alertness < MIN_ALERTNESS_FOR_PLANNING {
            continue;
        }

        if let Some(cns_goal) = &cns.current_goal {
            let new_goal = convert_cns_goal(cns_goal);
            if brain.current_goal.as_ref() != Some(&new_goal) {
                brain.current_goal = Some(new_goal);
                brain.current_plan = None;
                brain.plan_index = 0;
            }
        } else if brain.current_goal.is_some() {
            brain.current_goal = None;
            brain.current_plan = None;
            brain.plan_index = 0;
        }

        let plan_valid = brain
            .current_plan
            .as_ref()
            .is_some_and(|p| brain.plan_index < p.len());

        if !plan_valid {
            brain.current_plan = None;
            brain.plan_index = 0;
        }

        // 3. Form Plan
        if brain.current_plan.is_none() && brain.current_goal.is_some() {
            let goal = brain.current_goal.as_ref().unwrap();
            let actions = collect_planning_actions(
                &action_registry,
                mind,
                &affordances,
                PlanningMode::Replan,
            );

            plan_attempts += 1;

            if perf_logging && actions.len() > 20 {
                let action_names: Vec<String> = actions.iter().map(|a| a.name.clone()).collect();
                game_log.performance(format!(
                    "[RationalBrain] Ent {:?} planning with {} actions: {:?}",
                    entity,
                    actions.len(),
                    action_names
                ));
            }

            if let Some(plan) = crate::agent::brains::planner::regressive_plan(mind, goal, &actions)
            {
                brain.current_plan = Some(plan);
                brain.plan_index = 0;
            }
        }
    }

    if let Some(start) = start_time {
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 2 {
            game_log.performance(format!(
                "[RationalBrain] System update took {:?} ({} agents planned)",
                elapsed, plan_attempts
            ));
        }
    }
}

fn convert_cns_goal(
    old_goal: &crate::agent::brains::thinking::Goal,
) -> crate::agent::brains::thinking::Goal {
    old_goal.clone()
}

pub fn rational_brain_propose(
    brain: &RationalBrain,
    cns: &crate::agent::nervous_system::cns::CentralNervousSystem,
    _inventory: &ItemSlots,
    transform: &Transform,
    mind: &MindGraph,
    _visible: &crate::agent::mind::perception::VisibleObjects,
    _world_map: &WorldMap,
    action_registry: &crate::agent::actions::ActionRegistry,
    affordances: &Query<(
        &GlobalTransform,
        Option<&crate::agent::affordance::Affordance>,
    )>,
) -> Option<BrainProposal> {
    // The intent for any goal-directed rational proposal is derived from the
    // top urgency source that drove goal formulation. If no urgency, this is
    // idle wandering (Intent::None).
    let goal_intent = cns
        .urgencies
        .first()
        .map(|u| Intent::from_urgency_source(u.source))
        .unwrap_or(Intent::None);

    if let Some(plan) = &brain.current_plan
        && brain.plan_index < plan.len()
    {
        let action = &plan[brain.plan_index];
        // Re-verify preconditions before proposing (e.g., still have food to eat?)
        if are_preconditions_met(action, mind) {
            return Some(BrainProposal {
                brain: BrainType::Rational,
                action: action.clone(),
                urgency: PLAN_CONTINUATION_URGENCY,
                intent: goal_intent,
                reasoning: format!("Continuing plan step {}: {}", brain.plan_index, action.name),
            });
        }
        // Preconditions no longer met - fall through to replan
    }

    if let Some(goal) = &cns.current_goal {
        let agent_pos = transform.translation.truncate();
        let mut actions =
            collect_planning_actions(action_registry, mind, affordances, PlanningMode::Propose);

        // Proposal path: bias selection toward closer targets so the agent
        // doesn't grab a far-away tree when a near one will do.
        for template in actions.iter_mut() {
            if let Some(pos) = template.target_position {
                template.base_cost += agent_pos.distance(pos);
            }
        }

        if let Some(plan) = crate::agent::brains::planner::regressive_plan(mind, goal, &actions) {
            if let Some(first_action) = plan.first() {
                return Some(BrainProposal {
                    brain: BrainType::Rational,
                    action: first_action.clone(),
                    urgency: goal.priority,
                    intent: goal_intent,
                    reasoning: format!("New plan for goal: {:?}", goal.conditions),
                });
            } else {
                let wander_action = action_registry
                    .get(ActionType::Wander)
                    .map(|a| a.to_template(None))
                    .expect("Wander action must be registered");
                return Some(BrainProposal {
                    brain: BrainType::Rational,
                    action: wander_action,
                    urgency: crate::constants::brains::rational::GOAL_SATISFIED_WANDER_URGENCY,
                    intent: Intent::None,
                    reasoning: "Goal already satisfied, wandering".to_string(),
                });
            }
        }

        // Fallback: Explore to find resources ourselves
        // TODO(#46): reintroduce epistemic ask via CommunicationPlugin
        let explore_action = action_registry
            .get(ActionType::Explore)
            .map(|a| a.to_template(None))
            .expect("Explore action must be registered");

        return Some(BrainProposal {
            brain: BrainType::Rational,
            action: explore_action,
            urgency: goal.priority * EXPLORE_FALLBACK_PRIORITY_MULTIPLIER,
            intent: goal_intent,
            reasoning: "Can't plan - exploring for resources".to_string(),
        });
    }

    let wander_action = action_registry
        .get(ActionType::Wander)
        .map(|a| a.to_template(None))
        .expect("Wander action must be registered");
    Some(BrainProposal {
        brain: BrainType::Rational,
        action: wander_action,
        urgency: IDLE_WANDER_URGENCY,
        intent: Intent::None,
        reasoning: "Nothing to do, wandering".to_string(),
    })
}

/// Which gating policy `collect_planning_actions` uses to filter candidates.
///
/// The two policies are deliberately split because the rational brain runs
/// them at different frequencies and trusts different signals:
///
/// - [`PlanningMode::Replan`] runs on the slower `thinking_interval` from
///   `update_rational_brain`. It uses the legacy belief-confidence gate so
///   the planner doesn't chase rumours about long-decayed targets.
/// - [`PlanningMode::Propose`] runs every tick from `rational_brain_propose`.
///   It defers to each `Action::is_plan_valid` instead — the action's own
///   freshness check is faster than scoring a confidence pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanningMode {
    Replan,
    Propose,
}

/// Walk every action in the registry, ask `enumerate_targets` for its
/// candidates, and turn each survivor into an `ActionTemplate` via
/// `to_template_for_target`.
///
/// Single replacement for the old `collect_resource_targets` (planning path)
/// and `collect_affordance_targets` (proposal path). Distance-based cost
/// adjustment is the caller's job — the proposal path adds it, the plan
/// path doesn't, matching pre-#219 behaviour.
fn collect_planning_actions(
    action_registry: &crate::agent::actions::ActionRegistry,
    mind: &MindGraph,
    affordances: &Query<(
        &GlobalTransform,
        Option<&crate::agent::affordance::Affordance>,
    )>,
    mode: PlanningMode,
) -> Vec<ActionTemplate> {
    let mut actions = Vec::new();
    let belief_state = crate::agent::mind::belief_state::BeliefState::new(mind);

    for action in action_registry.all() {
        let source = action.target_source();
        for candidate in enumerate_targets(&source, action.action_type(), mind, affordances) {
            let keep = match mode {
                PlanningMode::Replan => candidate.as_entity().is_none_or(|entity| {
                    belief_state.pattern_confidence(&TriplePattern::entity_contains(entity)) > 0.1
                }),
                PlanningMode::Propose => action.is_plan_valid(&candidate, mind),
            };
            if !keep {
                continue;
            }

            actions.push(action.to_template_for_target(&candidate, mind));
        }
    }

    actions
}
