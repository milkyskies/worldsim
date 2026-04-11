//! Rational brain: deliberate goal-directed planning via GOAP.
//!
//! Reads: PlanMemory, Consciousness, MindGraph, VisibleObjects, CentralNervousSystem, PhysicalNeeds, Personality
//! Writes: PlanMemory (plan generation, commitment ticks, state transitions, eviction), BrainProposal
//! Upstream: cns (current_goal), planner (regressive_plan), mind (MindGraph)
//! Downstream: brains::proposal (winner selection), brains::plan_memory (state machine)

use crate::agent::Agent;
use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{ChannelCapacities, ChannelLoad};
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::brains::plan_memory::{
    HeldPlan, PlanMemory, PlanSource, PlanState, max_plans_for,
};
use crate::agent::brains::proposal::{BrainProposal, BrainType, Intent};
use crate::agent::brains::target_enumeration::enumerate_targets;
use crate::agent::brains::thinking::{ActionTemplate, TriplePattern};
use crate::agent::mind::knowledge::{MindGraph, Predicate, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::constants::brains::rational::{
    EXPLORE_FALLBACK_PRIORITY_MULTIPLIER, IDLE_WANDER_URGENCY, MIN_ALERTNESS_FOR_PLANNING,
};
use crate::world::map::WorldMap;
use bevy::prelude::*;

// Plan commitment — continuous value gating plan execution.
//
// `commitment_tick_delta` + `compute_commit_threshold` run on the rational
// brain's considered plans. Kept deliberately separate from
// `ActivePlanEntry::commitment_strength` in `active_plan.rs`, which tracks
// *post*-execution inertia (a different lifecycle phase — decays when a
// running plan stalls instead of accumulating toward a gate).

/// Baseline per-tick accumulation — exposed so integration tests can reason
/// about how many ticks an agent takes to cross threshold under various
/// personality mixes.
pub const COMMITMENT_BASELINE_PER_TICK: f32 = 0.05;
const COMMITMENT_URGENCY_WEIGHT: f32 = 0.2;
const COMMITMENT_ALONE_BONUS: f32 = 0.3;
const COMMITMENT_ANNOUNCEMENT_BONUS: f32 = 0.5;
const COMMITMENT_NEUROTICISM_PENALTY: f32 = 0.05;
const COMMITMENT_CONSCIENTIOUSNESS_BONUS: f32 = 0.05;
const COMMIT_THRESHOLD_COST_DIVISOR: f32 = 100.0;
const COMMIT_THRESHOLD_MIN: f32 = 0.5;
const COMMIT_THRESHOLD_MAX: f32 = 5.0;
const COMMIT_THRESHOLD_CONSCIENTIOUSNESS_DISCOUNT: f32 = 0.3;

/// Inputs for a single commitment tick. Packaged so the pure function is
/// easy to unit-test without wiring real ECS state.
pub struct CommitmentTickInputs {
    pub urgency: f32,
    pub alone: bool,
    pub announcement_made: bool,
    pub neuroticism: f32,
    pub conscientiousness: f32,
}

/// Pure per-tick commitment delta — no side effects, no ECS access.
pub fn commitment_tick_delta(inputs: &CommitmentTickInputs) -> f32 {
    let alone = if inputs.alone {
        COMMITMENT_ALONE_BONUS
    } else {
        0.0
    };
    let announcement = if inputs.announcement_made {
        COMMITMENT_ANNOUNCEMENT_BONUS
    } else {
        0.0
    };
    COMMITMENT_BASELINE_PER_TICK
        + inputs.urgency.clamp(0.0, 1.0) * COMMITMENT_URGENCY_WEIGHT
        + alone
        + announcement
        - inputs.neuroticism.clamp(0.0, 1.0) * COMMITMENT_NEUROTICISM_PENALTY
        + inputs.conscientiousness.clamp(0.0, 1.0) * COMMITMENT_CONSCIENTIOUSNESS_BONUS
}

/// Derive the commit threshold from the plan's subjective cost and the
/// agent's conscientiousness. Expensive plans need more commitment;
/// conscientious agents have lower thresholds (they commit more readily
/// once they've decided).
pub fn compute_commit_threshold(subjective_cost: f32, conscientiousness: f32) -> f32 {
    let cost_threshold = (subjective_cost / COMMIT_THRESHOLD_COST_DIVISOR)
        .clamp(COMMIT_THRESHOLD_MIN, COMMIT_THRESHOLD_MAX);
    let personality_modifier =
        1.0 - conscientiousness.clamp(0.0, 1.0) * COMMIT_THRESHOLD_CONSCIENTIOUSNESS_DISCOUNT;
    cost_threshold * personality_modifier
}

/// Marker component identifying agents whose cognition is driven by the
/// rational brain. All plan state lives in [`PlanMemory`]; this component
/// carries no fields and only exists so systems can filter agent queries
/// by the presence of a rational brain.
#[derive(Component, Debug, Clone, Copy, Reflect, Default)]
#[reflect(Component)]
pub struct RationalBrain;

/// True when the agent's anatomy can perform an action — every required
/// channel fits under the hard-conflict threshold against an empty load.
///
/// This must match the math `ChannelLoad::would_hard_conflict` uses at
/// arbitration time. If it drifts, the planner will propose infeasible
/// actions that arbitration silently drops, leaving the rational brain
/// stuck with a winning proposal that never starts — see #345, where a
/// wolf's jaw Manipulation (0.4) passed a naive `> 0.0` check and Attack
/// (Manipulation 0.9) got proposed instead of Bite.
fn action_is_anatomically_feasible(
    body_channels: &[crate::agent::actions::channel::ChannelUsage],
    capacities: &ChannelCapacities,
) -> bool {
    let empty = ChannelLoad::new();
    !empty.would_hard_conflict(body_channels, capacities)
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
    mut query: Query<
        (
            Entity,
            &mut PlanMemory,
            &mut crate::agent::brains::proposal::BrainState,
            &mut Consciousness,
            &Transform,
            &VisibleObjects,
            &crate::agent::nervous_system::cns::CentralNervousSystem,
            &MindGraph,
            Option<&Body>,
            &PhysicalNeeds,
            &crate::agent::psyche::personality::Personality,
        ),
        With<RationalBrain>,
    >,
    tick: Res<crate::core::tick::TickCount>,
    ns_config: Res<crate::agent::nervous_system::config::NervousSystemConfig>,
    _world_map: Res<WorldMap>,
    action_registry: Res<crate::agent::actions::ActionRegistry>,
    mut game_log: ResMut<crate::core::GameLog>,
    affordances: Query<(
        &GlobalTransform,
        Option<&crate::agent::affordance::Affordance>,
    )>,
    agents: Query<(), With<Agent>>,
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
        mut plan_memory,
        mut brain_state,
        mut consciousness,
        transform,
        visible,
        cns,
        mind,
        body,
        physical,
        personality,
    ) in query.iter_mut()
    {
        let capacities = ChannelCapacities::compute(body, Some(physical));
        let current_tick = tick.current;

        // 1. Verify every Executing plan: advance completed steps, drop
        //    plans whose preconditions broke, drop plans that have
        //    reached the end of their step list.
        let mut invalid_ids = Vec::new();
        let mut finished_ids = Vec::new();
        let mut explore_found_resources = false;
        for plan in plan_memory.plans.iter_mut() {
            if plan.state != PlanState::Executing {
                continue;
            }
            if let Some(action) = plan.current() {
                if is_step_complete(action, mind) {
                    plan.current_step += 1;
                    plan.last_touched = current_tick;
                }
                if let Some(action) = plan.current()
                    && !are_preconditions_met(action, mind)
                {
                    invalid_ids.push(plan.id);
                    continue;
                }
                if let Some(action) = plan.current()
                    && action.action_type == ActionType::Explore
                {
                    let has_known = mind
                        .query(None, Some(Predicate::Contains), None)
                        .iter()
                        .any(|triple| matches!(&triple.object, Value::Item(_, qty) if *qty > 0));
                    if has_known {
                        explore_found_resources = true;
                        finished_ids.push(plan.id);
                    }
                }
            }
            if plan.is_finished() {
                finished_ids.push(plan.id);
            }
        }
        for id in &invalid_ids {
            plan_memory.remove(*id);
        }
        for id in &finished_ids {
            plan_memory.remove(*id);
        }
        if !invalid_ids.is_empty() || explore_found_resources {
            // update_rational_brain runs before start_actions (data-conflict
            // ordering). Clearing chosen_actions here prevents start_actions
            // from re-starting the stale action this tick.
            brain_state.chosen_actions.clear();
        }

        // 2. Per-tick commitment accumulation for plans still in
        //    consideration (Background / Considering). Executing plans
        //    get a smaller sustain bonus; Suspended plans decay.
        let alone = visible.entities.iter().all(|e| agents.get(*e).is_err());
        // Snapshot the verbal-commitment side of the memory once so we
        // can read "has this concept been announced since T?" while
        // iterating the rest of the plans mutably. Without this snapshot
        // the borrow checker flags the simultaneous iter_mut + iter.
        let verbal_announcements: Vec<(crate::agent::mind::knowledge::Concept, u64)> = plan_memory
            .plans
            .iter()
            .filter(|p| p.source.is_verbal_commitment())
            .filter_map(|p| p.goal.target_concept().map(|c| (c, p.last_touched)))
            .collect();
        for plan in plan_memory.plans.iter_mut() {
            match plan.state {
                PlanState::Background | PlanState::Considering => {
                    let urgency = plan.goal.priority.clamp(0.0, 1.0);
                    // Announcement bonus fires when a background plan's
                    // goal concept matches a verbal-commitment plan this
                    // memory also holds that was refreshed after the
                    // current plan started — surfacing the plan through
                    // conversation accelerates commitment per #329.
                    let announcement_made = plan
                        .goal
                        .target_concept()
                        .map(|concept| {
                            verbal_announcements
                                .iter()
                                .any(|(c, touched)| *c == concept && *touched >= plan.created_at)
                        })
                        .unwrap_or(false);
                    let delta = commitment_tick_delta(&CommitmentTickInputs {
                        urgency,
                        alone,
                        announcement_made,
                        neuroticism: personality.traits.neuroticism,
                        conscientiousness: personality.traits.conscientiousness,
                    });
                    plan.commitment = (plan.commitment + delta).max(0.0);
                    plan.last_touched = current_tick;
                }
                PlanState::Executing => {
                    // Growing commitment while actively running mirrors
                    // the #166 post-execution inertia layer: progressing
                    // plans accumulate resistance to being flip-flopped.
                    plan.commitment =
                        (plan.commitment + EXECUTING_SUSTAIN_PER_TICK).min(MAX_COMMITMENT);
                }
                PlanState::Suspended => {
                    plan.commitment = (plan.commitment - SUSPENDED_DECAY_PER_TICK).max(0.0);
                }
            }
        }

        // 3. State transitions: promote plans upward when commitment
        //    crosses the cost-derived threshold. Stepless plans
        //    (verbal commitments that don't yet have a concrete GOAP
        //    plan) stay pinned in Background — letting them reach
        //    Executing would trigger `is_finished` on an empty step
        //    list and drop them immediately. The brain later
        //    regenerates a concrete plan for the same goal when the
        //    commitment surfaces as the current CNS goal.
        let mut transitions = Vec::new();
        for plan in plan_memory.plans.iter() {
            if plan.steps.is_empty() {
                continue;
            }
            let threshold = compute_commit_threshold(
                plan.subjective_cost,
                personality.traits.conscientiousness,
            );
            match plan.state {
                PlanState::Background
                    if plan.commitment >= threshold * BACKGROUND_PROMOTE_RATIO =>
                {
                    transitions.push((plan.id, PlanState::Considering));
                }
                PlanState::Considering if plan.commitment >= threshold => {
                    transitions.push((plan.id, PlanState::Executing));
                }
                PlanState::Suspended if plan.commitment <= 0.0 => {
                    transitions.push((plan.id, PlanState::Background));
                }
                _ => {}
            }
        }
        for (id, next) in transitions {
            if let Some(plan) = plan_memory.get_mut(id) {
                plan.state = next;
                plan.last_touched = current_tick;
            }
        }

        // 4. Heavy Thinking (Replanning) — only fires on the agent's
        //    staggered thinking tick.
        let should_replan = tick.should_run(entity, ns_config.thinking_interval);
        if !should_replan {
            continue;
        }

        // CONSCIOUSNESS CHECK: Can't plan while asleep
        if consciousness.alertness < MIN_ALERTNESS_FOR_PLANNING {
            continue;
        }

        // Drop held plans whose goals no longer match the current CNS
        // goal family. Plans tied to verbal commitments survive this
        // sweep because their motivation is external to CNS urgencies.
        if let Some(cns_goal) = &cns.current_goal {
            let goal = cns_goal.clone();
            let stale: Vec<_> = plan_memory
                .plans
                .iter()
                .filter(|p| {
                    matches!(p.source, PlanSource::Brain(BrainType::Rational)) && p.goal != goal
                })
                .map(|p| p.id)
                .collect();
            for id in stale {
                plan_memory.remove(id);
            }

            // 5. Form Plan: if no concrete plan already targets this
            //    goal, generate one. A stepless verbal commitment
            //    matching the goal doesn't count as a concrete plan —
            //    it's a reminder, not a ready-to-run sequence — so the
            //    brain still generates real steps for it.
            let has_concrete_plan = plan_memory
                .plans
                .iter()
                .any(|p| p.goal == goal && !p.steps.is_empty());
            if !has_concrete_plan {
                let actions = collect_planning_actions(
                    &action_registry,
                    mind,
                    &affordances,
                    PlanningMode::Generate,
                    &capacities,
                );

                plan_attempts += 1;

                if perf_logging && actions.len() > 20 {
                    let action_names: Vec<String> =
                        actions.iter().map(|a| a.name.clone()).collect();
                    game_log.performance(format!(
                        "[RationalBrain] Ent {:?} planning with {} actions: {:?}",
                        entity,
                        actions.len(),
                        action_names
                    ));
                }

                // GOAP search drains alertness. Curious (high-openness)
                // agents pay less; scale by thinking_interval so fast-brain
                // tests don't burn alertness faster than wallclock seconds.
                let openness_relief = personality.traits.openness
                    * crate::constants::brains::cognition::OPENNESS_PLANNING_RELIEF;
                let interval_scale = ns_config.thinking_interval as f32 / 60.0;
                let plan_drain = crate::constants::brains::rational::PLAN_GENERATION_ALERTNESS_DRAIN
                    * (1.0 - openness_relief) * interval_scale;
                consciousness.alertness = (consciousness.alertness - plan_drain).max(0.0);

                let cost_ctx = crate::agent::brains::planner::PlanCostContext::from_agent(
                    physical,
                    &consciousness,
                    personality,
                );
                if let Some(steps) =
                    crate::agent::brains::planner::regressive_plan(mind, &goal, &actions, &cost_ctx)
                {
                    let agent_pos = transform.translation.truncate();
                    let cost = crate::agent::brains::planner::estimate_plan_cost(
                        &steps, agent_pos, &cost_ctx, mind,
                    );
                    let id = plan_memory.mint_plan_id();
                    // Self-generated goal-directed plans have their urgency
                    // and cost folded into the initial commitment so the
                    // plan's starting state reflects how quickly it should
                    // begin running. Background is reserved for passively
                    // held plans (verbal commitments etc.); goal-directed
                    // plans skip straight into Considering or Executing
                    // depending on how strongly the goal drives them.
                    let threshold =
                        compute_commit_threshold(cost, personality.traits.conscientiousness);
                    // Seed commitment with urgency-weighted boost so urgent
                    // plans cross the threshold immediately and non-urgent
                    // ones still get a head start — matches the pre-#338
                    // "commit same tick" behaviour for hunger/thirst.
                    let initial_commitment = threshold * (0.5 + goal.priority.clamp(0.0, 1.0));
                    let initial_state = if initial_commitment >= threshold {
                        PlanState::Executing
                    } else {
                        PlanState::Considering
                    };
                    plan_memory.insert(HeldPlan {
                        id,
                        goal,
                        steps,
                        state: initial_state,
                        commitment: initial_commitment,
                        subjective_cost: cost,
                        source: PlanSource::Brain(BrainType::Rational),
                        created_at: current_tick,
                        last_touched: current_tick,
                        current_step: 0,
                    });
                }
            }
        } else {
            // No CNS goal: drop any brain-sourced plans that were
            // tracking the previous goal. Verbal commitments stay put.
            let stale: Vec<_> = plan_memory
                .plans
                .iter()
                .filter(|p| matches!(p.source, PlanSource::Brain(BrainType::Rational)))
                .map(|p| p.id)
                .collect();
            for id in stale {
                plan_memory.remove(id);
            }
        }

        // 6. Cognitive load cap: evict the weakest background plans if
        //    we're over capacity. Personality modulates the cap.
        let max = max_plans_for(
            personality.traits.openness,
            personality.traits.conscientiousness,
            personality.traits.neuroticism,
        );
        plan_memory.evict_excess(max);
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

/// Commitment that an Executing plan gains each tick while it continues
/// to run. Rewards sustained progress so another plan can't instantly
/// flip-flop it out via a tiny urgency edge.
const EXECUTING_SUSTAIN_PER_TICK: f32 = 0.05;
/// Commitment that a Suspended plan loses each tick while it waits for
/// its channels to free up. Dropping to zero sends the plan back to
/// Background consideration.
const SUSPENDED_DECAY_PER_TICK: f32 = 0.05;
/// Commitment ceiling — caps the sustain term so Executing plans can't
/// accumulate an unbounded inertia lead over fresh competitors.
const MAX_COMMITMENT: f32 = 10.0;
/// Fraction of the final threshold at which a Background plan becomes
/// Considering. Two-step promotion gives the agent a foreground /
/// background split without a hardcoded dwell timer.
const BACKGROUND_PROMOTE_RATIO: f32 = 0.5;

/// Map a plan's goal to the arbitration `Intent` it should compete on.
/// Returns `None` for goals whose conditions don't match any of the
/// drive predicates the intent enum knows about — caller falls back to
/// the top CNS urgency intent in that case.
fn intent_for_goal(goal: &crate::agent::brains::thinking::Goal) -> Option<Intent> {
    for cond in &goal.conditions {
        let Some(predicate) = cond.predicate else {
            continue;
        };
        let intent = match predicate {
            Predicate::Hunger => Some(Intent::SatisfyHunger),
            Predicate::Thirst => Some(Intent::SatisfyThirst),
            Predicate::Stamina => Some(Intent::SatisfyStamina),
            Predicate::SocialDrive => Some(Intent::SatisfySocial),
            Predicate::Pain => Some(Intent::SatisfyPainRelief),
            // `Contains` goals describe resource acquisition. Map by
            // the concept's edibility / drinkability when known so a
            // food-acquisition plan competes on Hunger and a water
            // plan competes on Thirst.
            Predicate::Contains => match &cond.object {
                Some(Value::Item(concept, _)) => match concept {
                    crate::agent::mind::knowledge::Concept::Apple
                    | crate::agent::mind::knowledge::Concept::Berry
                    | crate::agent::mind::knowledge::Concept::Meat => Some(Intent::SatisfyHunger),
                    crate::agent::mind::knowledge::Concept::Water => Some(Intent::SatisfyThirst),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        if intent.is_some() {
            return intent;
        }
    }
    None
}

/// Emit rational-brain proposals for every plan currently in the
/// Executing state. Arbitration consumes this list and admits as many as
/// body channels allow; rejected proposals trigger Executing → Suspended
/// transitions back in `brain_system`.
///
/// When no plan is executing, falls back to Explore (for hunger/thirst
/// goals with no known target) or Wander (idle). The Wander fallback is
/// gated when the agent is currently in a conversation — emitting it
/// would walk them out of conversation range and collapse the social
/// turn (the 1-tick flicker bug from #330).
pub fn rational_brain_propose(
    plan_memory: &PlanMemory,
    cns: &crate::agent::nervous_system::cns::CentralNervousSystem,
    mind: &MindGraph,
    action_registry: &crate::agent::actions::ActionRegistry,
    in_conversation: bool,
) -> Vec<BrainProposal> {
    let cns_intent = cns
        .urgencies
        .first()
        .map(|u| Intent::from_urgency_source(u.source))
        .unwrap_or(Intent::None);

    let mut out: Vec<BrainProposal> = Vec::new();
    for plan in plan_memory.in_state(PlanState::Executing) {
        let Some(action) = plan.current() else {
            continue;
        };
        if !are_preconditions_met(action, mind) {
            continue;
        }
        let urgency = (plan.goal.priority * 100.0).max(1.0);
        // Per-plan intent, derived from the plan's goal so multiple
        // Executing plans don't all collapse onto the same intent in
        // arbitration's dedup pass. Falls back to the top CNS urgency
        // intent (or `None`) when the goal's conditions don't map to
        // a specific drive.
        let intent = intent_for_goal(&plan.goal).unwrap_or(cns_intent);
        out.push(BrainProposal {
            brain: BrainType::Rational,
            action: action.clone(),
            urgency,
            intent,
            reasoning: format!(
                "Executing plan {:?} step {}: {}",
                plan.id, plan.current_step, action.name
            ),
        });
    }

    if !out.is_empty() {
        return out;
    }

    // No executing plan — fall through to Explore or idle wander.
    if let Some(goal) = &cns.current_goal {
        if matches!(cns_intent, Intent::SatisfyHunger | Intent::SatisfyThirst) {
            let explore_action = action_registry
                .get(ActionType::Explore)
                .map(|a| a.to_template(None))
                .expect("Explore action must be registered");
            return vec![BrainProposal {
                brain: BrainType::Rational,
                action: explore_action,
                urgency: goal.priority * EXPLORE_FALLBACK_PRIORITY_MULTIPLIER * 100.0,
                intent: cns_intent,
                reasoning: "No plan ready — exploring for resources".to_string(),
            }];
        }
        // Non-resource goal with no plan yet: let another brain win.
        return Vec::new();
    }

    // Idle wander suppressed during conversation — Wander rides
    // Locomotion and would walk the agent out of conversation range,
    // collapsing the turn and triggering the #330 flicker.
    if in_conversation {
        return Vec::new();
    }

    let wander_action = action_registry
        .get(ActionType::Wander)
        .map(|a| a.to_template(None))
        .expect("Wander action must be registered");
    vec![BrainProposal {
        brain: BrainType::Rational,
        action: wander_action,
        urgency: IDLE_WANDER_URGENCY,
        intent: Intent::None,
        reasoning: "Nothing to do, wandering".to_string(),
    }]
}

/// Gating policy used by `collect_planning_actions`. Unifies the old
/// belief-confidence filter with each action's own `is_plan_valid`:
/// a candidate is kept if *either* check is satisfied. The
/// belief-confidence path keeps foraging (the agent has a rumour of
/// apples on a tree, confidence 0.3) alive; the is_plan_valid path
/// keeps non-container targets (hunting a deer that doesn't
/// `Contains` anything) alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanningMode {
    Generate,
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
    capacities: &ChannelCapacities,
) -> Vec<ActionTemplate> {
    let mut actions = Vec::new();
    let belief_state = crate::agent::mind::belief_state::BeliefState::new(mind);

    for action in action_registry.all() {
        if !action_is_anatomically_feasible(action.body_channels(), capacities) {
            continue;
        }

        let source = action.target_source();
        for candidate in enumerate_targets(&source, action.action_type(), mind, affordances) {
            let keep = match mode {
                PlanningMode::Generate => {
                    // Keep if either the action's own validity check
                    // passes (covers non-container targets like prey
                    // animals) OR the agent has any non-trivial belief
                    // that the target contains something (covers
                    // rumoured foraging sources).
                    action.is_plan_valid(&candidate, mind)
                        || candidate.as_entity().is_some_and(|entity| {
                            belief_state.pattern_confidence(&TriplePattern::entity_contains(entity))
                                > 0.1
                        })
                }
            };
            if !keep {
                continue;
            }

            actions.push(action.to_template_for_target(&candidate, mind));
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::ActionRegistry;
    use crate::agent::brains::plan_memory::PlanId;
    use crate::agent::brains::thinking::Goal;
    use crate::agent::mind::knowledge::{Concept, Node as MindNode, Value};
    use crate::agent::nervous_system::cns::CentralNervousSystem;
    use crate::agent::nervous_system::urgency::{Urgency, UrgencySource};

    fn template(name: &str, action_type: ActionType) -> ActionTemplate {
        ActionTemplate {
            name: name.to_string(),
            action_type,
            target_entity: None,
            target_position: None,
            preconditions: vec![],
            effects: vec![],
            consumes: vec![],
            base_cost: 1.0,
            locomotion_intensity: action_type.default_locomotion_intensity(),
        }
    }

    fn hunger_goal(priority: f32) -> Goal {
        Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::Contains),
                Some(Value::Item(Concept::Apple, 1)),
            )],
            priority,
        }
    }

    fn cns_with_hunger(priority: f32) -> CentralNervousSystem {
        let mut cns = CentralNervousSystem::default();
        cns.urgencies
            .push(Urgency::new(UrgencySource::Hunger, priority));
        cns.current_goal = Some(hunger_goal(priority));
        cns
    }

    fn executing_plan(memory: &mut PlanMemory, goal: Goal, step: ActionTemplate) -> PlanId {
        let id = memory.mint_plan_id();
        memory.insert(HeldPlan {
            id,
            goal,
            steps: vec![step],
            state: PlanState::Executing,
            commitment: 10.0,
            subjective_cost: 10.0,
            source: PlanSource::Brain(BrainType::Rational),
            created_at: 0,
            last_touched: 0,
            current_step: 0,
        });
        id
    }

    fn test_registry() -> ActionRegistry {
        let mut r = ActionRegistry::default();
        r.register(crate::agent::actions::action::WanderAction);
        r.register(crate::agent::actions::action::ExploreAction);
        r
    }

    #[test]
    fn propose_emits_step_for_executing_plan() {
        let mut memory = PlanMemory::default();
        let cns = cns_with_hunger(1.0);
        executing_plan(
            &mut memory,
            hunger_goal(1.0),
            template("WalkToApple", ActionType::Walk),
        );

        let proposals = rational_brain_propose(
            &memory,
            &cns,
            &MindGraph::default(),
            &test_registry(),
            false,
        );

        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].brain, BrainType::Rational);
        assert_eq!(proposals[0].action.action_type, ActionType::Walk);
        assert!((proposals[0].urgency - 100.0).abs() < 0.01);
    }

    #[test]
    fn propose_returns_empty_for_non_resource_goal_without_plan() {
        let mut cns = CentralNervousSystem::default();
        cns.urgencies.push(Urgency::new(UrgencySource::Social, 0.8));
        cns.current_goal = Some(Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::SocialDrive),
                Some(Value::Int(0)),
            )],
            priority: 0.8,
        });
        let memory = PlanMemory::default();

        let proposals = rational_brain_propose(
            &memory,
            &cns,
            &MindGraph::default(),
            &test_registry(),
            false,
        );

        assert!(
            proposals.is_empty(),
            "rational must defer when the social goal has no executing plan"
        );
    }

    #[test]
    fn propose_explore_fallback_for_hunger_without_plan() {
        let cns = cns_with_hunger(1.0);
        let memory = PlanMemory::default();

        let proposals = rational_brain_propose(
            &memory,
            &cns,
            &MindGraph::default(),
            &test_registry(),
            false,
        );

        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].action.action_type, ActionType::Explore);
        let expected = 1.0 * EXPLORE_FALLBACK_PRIORITY_MULTIPLIER * 100.0;
        assert!((proposals[0].urgency - expected).abs() < 0.01);
    }

    #[test]
    fn propose_idle_wander_when_no_goal_and_no_plan() {
        let cns = CentralNervousSystem::default();
        let memory = PlanMemory::default();

        let proposals = rational_brain_propose(
            &memory,
            &cns,
            &MindGraph::default(),
            &test_registry(),
            false,
        );

        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].action.action_type, ActionType::Wander);
        assert!((proposals[0].urgency - IDLE_WANDER_URGENCY).abs() < 0.01);
    }

    #[test]
    fn propose_emits_multiple_proposals_for_parallel_executing_plans() {
        let mut memory = PlanMemory::default();
        let cns = cns_with_hunger(1.0);
        executing_plan(
            &mut memory,
            hunger_goal(1.0),
            template("Walk", ActionType::Walk),
        );
        executing_plan(
            &mut memory,
            hunger_goal(0.5),
            template("Converse", ActionType::Converse),
        );

        let proposals = rational_brain_propose(
            &memory,
            &cns,
            &MindGraph::default(),
            &test_registry(),
            false,
        );

        assert_eq!(
            proposals.len(),
            2,
            "both Executing plans should surface proposals for arbitration to admit"
        );
    }

    // ─── Plan commitment ──────────────────────────────────────────────────────

    fn neutral_inputs() -> CommitmentTickInputs {
        CommitmentTickInputs {
            urgency: 0.0,
            alone: false,
            announcement_made: false,
            neuroticism: 0.5,
            conscientiousness: 0.5,
        }
    }

    #[test]
    fn commitment_tick_baseline_contributes_slowly() {
        let mut inputs = neutral_inputs();
        inputs.neuroticism = 0.5;
        inputs.conscientiousness = 0.5;
        let delta = commitment_tick_delta(&inputs);
        // baseline 0.05 + 0.5 * 0.05 (consci) - 0.5 * 0.05 (neuro) = 0.05
        assert!(
            (delta - 0.05).abs() < 1e-5,
            "neutral personality + no urgency + not alone → baseline 0.05, got {delta}"
        );
    }

    #[test]
    fn commitment_tick_alone_agent_contributes_more_than_accompanied() {
        let alone = CommitmentTickInputs {
            alone: true,
            ..neutral_inputs()
        };
        let company = CommitmentTickInputs {
            alone: false,
            ..neutral_inputs()
        };
        assert!(
            commitment_tick_delta(&alone) > commitment_tick_delta(&company),
            "solo agent should commit faster than one with company"
        );
    }

    #[test]
    fn commitment_tick_neurotic_commits_slower_than_stoic() {
        let stoic = CommitmentTickInputs {
            neuroticism: 0.0,
            ..neutral_inputs()
        };
        let neurotic = CommitmentTickInputs {
            neuroticism: 1.0,
            ..neutral_inputs()
        };
        assert!(
            commitment_tick_delta(&stoic) > commitment_tick_delta(&neurotic),
            "stoic agent should accumulate commitment faster than neurotic one"
        );
    }

    #[test]
    fn commitment_tick_urgent_plan_commits_fast() {
        let urgent = CommitmentTickInputs {
            urgency: 1.0,
            ..neutral_inputs()
        };
        let calm = CommitmentTickInputs {
            urgency: 0.0,
            ..neutral_inputs()
        };
        assert!(
            commitment_tick_delta(&urgent) > commitment_tick_delta(&calm) + 0.15,
            "full urgency should add at least 0.15 per tick over calm baseline"
        );
    }

    #[test]
    fn commitment_tick_announcement_adds_chunk() {
        let silent = neutral_inputs();
        let announced = CommitmentTickInputs {
            announcement_made: true,
            ..neutral_inputs()
        };
        let bonus = commitment_tick_delta(&announced) - commitment_tick_delta(&silent);
        assert!(
            (bonus - COMMITMENT_ANNOUNCEMENT_BONUS).abs() < 1e-5,
            "announcing should add exactly COMMITMENT_ANNOUNCEMENT_BONUS ({}), got {bonus}",
            COMMITMENT_ANNOUNCEMENT_BONUS
        );
    }

    #[test]
    fn commit_threshold_clamps_cheap_plans_to_minimum() {
        let t = compute_commit_threshold(0.0, 0.5);
        assert!(
            t >= COMMIT_THRESHOLD_MIN * 0.7,
            "cheap-plan threshold must honor the floor (got {t})"
        );
    }

    #[test]
    fn commit_threshold_clamps_expensive_plans_to_maximum() {
        let t = compute_commit_threshold(10_000.0, 0.0);
        assert!(
            t <= COMMIT_THRESHOLD_MAX,
            "expensive-plan threshold must honor the ceiling (got {t})"
        );
    }

    #[test]
    fn commit_threshold_scales_with_cost() {
        let cheap = compute_commit_threshold(50.0, 0.5);
        let expensive = compute_commit_threshold(400.0, 0.5);
        assert!(
            expensive > cheap,
            "expensive plans should require more commitment to execute ({cheap} vs {expensive})"
        );
    }

    #[test]
    fn commit_threshold_conscientious_agent_lower_than_spontaneous() {
        let disciplined = compute_commit_threshold(200.0, 1.0);
        let spontaneous = compute_commit_threshold(200.0, 0.0);
        assert!(
            disciplined < spontaneous,
            "conscientious agent should have a lower threshold ({disciplined} vs {spontaneous})"
        );
    }

    #[test]
    fn uncommitted_background_plan_does_not_propose() {
        let mut memory = PlanMemory::default();
        let cns = cns_with_hunger(1.0);
        let id = memory.mint_plan_id();
        memory.insert(HeldPlan {
            id,
            goal: hunger_goal(1.0),
            steps: vec![template("Walk", ActionType::Walk)],
            state: PlanState::Background,
            commitment: 0.0,
            subjective_cost: 50.0,
            source: PlanSource::Brain(BrainType::Rational),
            created_at: 0,
            last_touched: 0,
            current_step: 0,
        });

        let proposals = rational_brain_propose(
            &memory,
            &cns,
            &MindGraph::default(),
            &test_registry(),
            false,
        );

        // Background plans aren't proposed — Explore fallback fires because
        // hunger is a resource goal with no executing plan yet.
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].action.action_type, ActionType::Explore);
    }

    /// Regression for #345: the planner's feasibility gate must reject
    /// actions whose channel intensity would hard-conflict against an empty
    /// load, not just "the channel exists at all." A wolf has Manipulation
    /// 0.4 from its jaws; Attack requires Manipulation 0.9. The old gate
    /// passed Attack because 0.4 > 0.0, then arbitration rejected it at
    /// runtime, leaving the wolf's rational brain stuck proposing Attack
    /// while Bite was never considered — agent ended up idle.
    #[test]
    fn anatomical_feasibility_rejects_attack_for_wolf() {
        use crate::agent::actions::action::{AttackAction, BiteAction};
        use crate::agent::actions::registry::Action;
        use crate::agent::biology::body::Body;

        let wolf_caps = ChannelCapacities::compute(Some(&Body::wolf()), None);
        assert!(
            !action_is_anatomically_feasible(AttackAction.body_channels(), &wolf_caps),
            "wolf's Manipulation 0.4 should hard-conflict with Attack's 0.9"
        );
        assert!(
            action_is_anatomically_feasible(BiteAction.body_channels(), &wolf_caps),
            "wolf's jaws (Bite 1.0) should comfortably run Bite"
        );
    }

    #[test]
    fn anatomical_feasibility_rejects_bite_for_human() {
        use crate::agent::actions::action::{AttackAction, BiteAction};
        use crate::agent::actions::registry::Action;
        use crate::agent::biology::body::Body;

        let human_caps = ChannelCapacities::compute(Some(&Body::human()), None);
        assert!(
            action_is_anatomically_feasible(AttackAction.body_channels(), &human_caps),
            "human's two arms (Manipulation 1.0) should fit Attack's 0.9"
        );
        assert!(
            !action_is_anatomically_feasible(BiteAction.body_channels(), &human_caps),
            "human has no Bite channel; Bite must be rejected"
        );
    }
}
