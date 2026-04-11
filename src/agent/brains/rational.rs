//! Rational brain: deliberate goal-directed planning via GOAP.
//!
//! Reads: RationalBrain, Consciousness, ItemSlots, MindGraph, VisibleObjects, CentralNervousSystem, Commitments
//! Writes: RationalBrain (plan/goal/commitment), BrainProposal
//! Upstream: cns (current_goal), planner (regressive_plan), mind (MindGraph)
//! Downstream: brains::proposal (winner selection)

use crate::agent::Agent;
use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{ChannelCapacities, ChannelLoad};
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::brains::proposal::{BrainProposal, BrainType, Intent};
use crate::agent::brains::target_enumeration::enumerate_targets;
use crate::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
use crate::agent::commitment::Commitments;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::constants::brains::rational::{
    EXPLORE_FALLBACK_PRIORITY_MULTIPLIER, IDLE_WANDER_URGENCY, MIN_ALERTNESS_FOR_PLANNING,
    PLAN_CONTINUATION_URGENCY,
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

#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct RationalBrain {
    #[reflect(ignore)]
    pub current_plan: Option<Vec<ActionTemplate>>,
    #[reflect(ignore)]
    pub current_goal: Option<Goal>,
    #[reflect(ignore)]
    pub plan_index: usize,
    /// Continuous commitment accumulated while considering the current plan.
    /// Crosses `compute_commit_threshold` to gate execution.
    pub commitment: f32,
    /// Subjective cost of the current plan — feeds the commit threshold.
    pub subjective_cost: f32,
    /// Tick when the current plan was generated. `None` when no plan.
    pub plan_started_at: Option<u64>,
    /// True once commitment crosses threshold. Stays true until plan clears.
    pub plan_committed: bool,
}

impl RationalBrain {
    /// Reset all plan + commitment state. Called when a plan clears for any
    /// reason (finished, invalidated, goal changed).
    pub fn clear_plan(&mut self) {
        self.current_plan = None;
        self.plan_index = 0;
        self.commitment = 0.0;
        self.subjective_cost = 0.0;
        self.plan_started_at = None;
        self.plan_committed = false;
    }
}

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

/// Check if action's preconditions are still met.
///
/// Also rejects plan steps whose goal tile has been marked `Unreachable`
/// since the plan was formed. Walks generated by `generate_implicit_walk`
/// declare a `(Self_, LocatedAt, Tile(target))` effect; if the belief
/// updater has since recorded that target tile as unreachable (from an
/// `ActionOutcome::Failed { PathBlocked }`), the stored plan must be
/// invalidated so the agent replans against the updated knowledge
/// instead of re-emitting the blocked walk every tick.
fn are_preconditions_met(action: &ActionTemplate, mind: &MindGraph) -> bool {
    // Walk step pointing at a tile the agent now believes is unreachable
    // is immediately invalid — no point retrying it until the belief ages out.
    if action.action_type == ActionType::Walk {
        for effect in &action.effects {
            if effect.predicate == Predicate::LocatedAt
                && let Value::Tile(tile) = &effect.object
                && mind.has_trait(&Node::Tile(*tile), Concept::Unreachable)
            {
                return false;
            }
        }
    }

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
        &mut Consciousness,
        &ItemSlots,
        &Transform,
        &VisibleObjects,
        &crate::agent::nervous_system::cns::CentralNervousSystem,
        &MindGraph,
        Option<&Body>,
        &PhysicalNeeds,
        &crate::agent::psyche::personality::Personality,
        Option<&Commitments>,
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
        mut brain,
        mut brain_state,
        mut consciousness,
        _inventory,
        transform,
        visible,
        cns,
        mind,
        body,
        physical,
        personality,
        commitments,
    ) in query.iter_mut()
    {
        let capacities = ChannelCapacities::compute(body, Some(physical));
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
            brain.clear_plan();
        } else if plan_finished || plan_invalid {
            if plan_invalid {
                brain.current_goal = None;
                // update_rational_brain runs before start_actions (data-conflict ordering).
                // Clearing chosen_actions here prevents start_actions from re-starting the
                // stale action this tick and on subsequent ticks until three_brains_system
                // next fires and re-populates the list with a valid proposal.
                brain_state.chosen_actions.clear();
            }
            brain.clear_plan();
        }

        // Commitment runs every frame so the gate stays responsive to
        // conditions that flip between thinking cycles (becoming alone,
        // sharing the plan in conversation). Positioned before the
        // `should_replan` gate so non-thinking ticks still accumulate.
        if brain.current_plan.is_some() && !brain.plan_committed {
            let urgency = cns
                .current_goal
                .as_ref()
                .map(|g| g.priority.clamp(0.0, 1.0))
                .unwrap_or(0.0);
            let alone = visible.entities.iter().all(|e| agents.get(*e).is_err());
            // Credit an announcement only when the agent has verbally
            // committed to *this plan's* concept since the plan started —
            // matching on the concept avoids spurious credit from unrelated
            // chit-chat about standing commitments.
            let plan_goal_concept = brain.current_goal.as_ref().and_then(Goal::target_concept);
            let announcement_made = match (commitments, plan_goal_concept, brain.plan_started_at) {
                (Some(c), Some(concept), Some(started)) => c
                    .active
                    .iter()
                    .any(|entry| entry.goal == concept && entry.committed_at >= started),
                _ => false,
            };
            let delta = commitment_tick_delta(&CommitmentTickInputs {
                urgency,
                alone,
                announcement_made,
                neuroticism: personality.traits.neuroticism,
                conscientiousness: personality.traits.conscientiousness,
            });
            brain.commitment = (brain.commitment + delta).max(0.0);
            let threshold = compute_commit_threshold(
                brain.subjective_cost,
                personality.traits.conscientiousness,
            );
            if brain.commitment >= threshold {
                brain.plan_committed = true;
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
                brain.clear_plan();
            }
        } else if brain.current_goal.is_some() {
            brain.current_goal = None;
            brain.clear_plan();
        }

        let plan_valid = brain
            .current_plan
            .as_ref()
            .is_some_and(|p| brain.plan_index < p.len());

        if !plan_valid {
            brain.clear_plan();
        }

        // 3. Form Plan
        if brain.current_plan.is_none() && brain.current_goal.is_some() {
            let goal = brain.current_goal.as_ref().unwrap();
            let actions = collect_planning_actions(
                &action_registry,
                mind,
                &affordances,
                PlanningMode::Replan,
                &capacities,
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

            // Plan generation drains alertness. GOAP search is cognitively
            // expensive; curious (high-openness) agents enjoy it so pay less.
            // Scaled by thinking_interval so fast-brain tests don't burn
            // alertness faster than a wallclock second's worth.
            let openness_relief = personality.traits.openness
                * crate::constants::brains::cognition::OPENNESS_PLANNING_RELIEF;
            let interval_scale = ns_config.thinking_interval as f32 / 60.0;
            let plan_drain = crate::constants::brains::rational::PLAN_GENERATION_ALERTNESS_DRAIN
                * (1.0 - openness_relief)
                * interval_scale;
            consciousness.alertness = (consciousness.alertness - plan_drain).max(0.0);

            let cost_ctx = crate::agent::brains::planner::PlanCostContext::from_agent(
                physical,
                &consciousness,
                personality,
                tick.current,
            );
            if let Some(plan) =
                crate::agent::brains::planner::regressive_plan(mind, goal, &actions, &cost_ctx)
            {
                let agent_pos = transform.translation.truncate();
                let cost = crate::agent::brains::planner::estimate_plan_cost(
                    &plan, agent_pos, &cost_ctx, mind,
                );
                brain.current_plan = Some(plan);
                brain.plan_index = 0;
                brain.commitment = 0.0;
                brain.subjective_cost = cost;
                brain.plan_started_at = Some(tick.current);
                brain.plan_committed = false;
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
    capacities: &ChannelCapacities,
    cost_ctx: &crate::agent::brains::planner::PlanCostContext,
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
            // Uncommitted plans defer so other brains can win arbitration.
            if !brain.plan_committed {
                return None;
            }
            // Plan continuation urgency tracks the goal that drove the plan, so it
            // sits on the same scale as Survival's `urgency * 100` proposals. The
            // constant fallback only kicks in if the goal cleared mid-plan.
            let urgency = cns
                .current_goal
                .as_ref()
                .map(|g| g.priority * 100.0)
                .unwrap_or(PLAN_CONTINUATION_URGENCY);
            return Some(BrainProposal {
                brain: BrainType::Rational,
                action: action.clone(),
                urgency,
                intent: goal_intent,
                reasoning: format!("Continuing plan step {}: {}", brain.plan_index, action.name),
            });
        }
        // Preconditions no longer met - fall through to replan
    }

    if let Some(goal) = &cns.current_goal {
        let agent_pos = transform.translation.truncate();
        let mut actions = collect_planning_actions(
            action_registry,
            mind,
            affordances,
            PlanningMode::Propose,
            capacities,
        );

        // Proposal path: bias selection toward closer targets so the agent
        // doesn't grab a far-away tree when a near one will do.
        for template in actions.iter_mut() {
            if let Some(pos) = template.target_position {
                template.base_cost += agent_pos.distance(pos);
            }
        }

        if let Some(plan) =
            crate::agent::brains::planner::regressive_plan(mind, goal, &actions, cost_ctx)
        {
            if let Some(first_action) = plan.first() {
                return Some(BrainProposal {
                    brain: BrainType::Rational,
                    action: first_action.clone(),
                    urgency: goal.priority * 100.0,
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

        // Fallback: Explore to find resources ourselves — but only for goals
        // exploration could plausibly satisfy. A social or pain-relief goal
        // can't be solved by wandering the map; proposing Explore there would
        // dedup against (and outscore, post-units-fix) the Emotional brain's
        // own answer for the same intent.
        // TODO(#46): reintroduce epistemic ask via CommunicationPlugin
        if !matches!(goal_intent, Intent::SatisfyHunger | Intent::SatisfyThirst) {
            return None;
        }

        let explore_action = action_registry
            .get(ActionType::Explore)
            .map(|a| a.to_template(None))
            .expect("Explore action must be registered");

        return Some(BrainProposal {
            brain: BrainType::Rational,
            action: explore_action,
            urgency: goal.priority * EXPLORE_FALLBACK_PRIORITY_MULTIPLIER * 100.0,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::ActionRegistry;
    use crate::agent::brains::thinking::Goal;
    use crate::agent::nervous_system::cns::CentralNervousSystem;
    use crate::agent::nervous_system::urgency::{Urgency, UrgencySource};
    use bevy::ecs::system::SystemState;

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

    fn cns_with_goal(priority: f32, source: UrgencySource) -> CentralNervousSystem {
        let mut cns = CentralNervousSystem::default();
        cns.urgencies.push(Urgency::new(source, priority));
        cns.current_goal = Some(Goal {
            conditions: vec![],
            priority,
        });
        cns
    }

    fn brain_with_plan(goal: Option<Goal>) -> RationalBrain {
        // `plan_committed: true` so the continuation path proposes — the
        // commitment deferral gate would otherwise short-circuit to `None`.
        RationalBrain {
            current_plan: Some(vec![template("FakeStep", ActionType::Idle)]),
            current_goal: goal,
            plan_committed: true,
            ..Default::default()
        }
    }

    /// Calls `rational_brain_propose` against the given brain/cns. Constructs the
    /// throwaway Bevy state (world, query, registry) needed to satisfy the signature.
    fn propose(brain: &RationalBrain, cns: &CentralNervousSystem) -> BrainProposal {
        let mut world = World::new();
        let mut state: SystemState<
            Query<(
                &GlobalTransform,
                Option<&crate::agent::affordance::Affordance>,
            )>,
        > = SystemState::new(&mut world);
        let affordances = state.get(&world);

        let registry = {
            let mut r = ActionRegistry::default();
            r.register(crate::agent::actions::action::WanderAction);
            r.register(crate::agent::actions::action::ExploreAction);
            r
        };
        let inventory = ItemSlots::agent_carry();
        let transform = Transform::default();
        let mind = MindGraph::default();
        let visible = crate::agent::mind::perception::VisibleObjects::default();
        let world_map = WorldMap::new(64, 64);

        let capacities = ChannelCapacities::full();
        let cost_ctx = crate::agent::brains::planner::PlanCostContext::neutral();
        rational_brain_propose(
            brain,
            cns,
            &inventory,
            &transform,
            &mind,
            &visible,
            &world_map,
            &registry,
            &affordances,
            &capacities,
            &cost_ctx,
        )
        .expect("rational brain should always produce a proposal")
    }

    #[test]
    fn plan_continuation_urgency_scales_with_current_goal_priority() {
        let cns = cns_with_goal(1.0, UrgencySource::Hunger);
        let brain = brain_with_plan(Some(cns.current_goal.clone().unwrap()));

        let proposal = propose(&brain, &cns);

        assert_eq!(proposal.brain, BrainType::Rational);
        assert!(
            (proposal.urgency - 100.0).abs() < 0.01,
            "expected urgency ~100 for goal priority 1.0, got {}",
            proposal.urgency
        );
    }

    #[test]
    fn plan_continuation_urgency_halves_when_goal_priority_halves() {
        let cns = cns_with_goal(0.5, UrgencySource::Hunger);
        let brain = brain_with_plan(Some(cns.current_goal.clone().unwrap()));

        let proposal = propose(&brain, &cns);

        assert!(
            (proposal.urgency - 50.0).abs() < 0.01,
            "expected urgency ~50 for goal priority 0.5, got {}",
            proposal.urgency
        );
    }

    /// Plan-continuation urgency must sit on the same numerical scale as Survival's
    /// `urgency * 100` proposals — otherwise dedup and cross-intent comparisons skew
    /// against Rational.
    #[test]
    fn plan_continuation_urgency_matches_survival_scale_for_same_drive() {
        let priority = 0.9;
        let cns = cns_with_goal(priority, UrgencySource::Hunger);
        let brain = brain_with_plan(Some(cns.current_goal.clone().unwrap()));

        let proposal = propose(&brain, &cns);
        let survival_equivalent = priority * 100.0;

        assert!(
            (proposal.urgency - survival_equivalent).abs() < 0.01,
            "rational plan-continuation urgency ({}) must match Survival scale ({})",
            proposal.urgency,
            survival_equivalent
        );
    }

    /// If the goal cleared mid-plan, fall back to the constant rather than reading a
    /// stale priority. The constant only exists for this edge case.
    #[test]
    fn plan_continuation_falls_back_to_constant_when_goal_cleared() {
        let cns = CentralNervousSystem::default(); // no current_goal
        let brain = brain_with_plan(None);

        let proposal = propose(&brain, &cns);

        assert!(
            (proposal.urgency - PLAN_CONTINUATION_URGENCY).abs() < 0.01,
            "expected fallback constant ({}) when goal is cleared, got {}",
            PLAN_CONTINUATION_URGENCY,
            proposal.urgency
        );
    }

    /// Explore fallback (no plan, planner failed) must also be on Survival's scale.
    /// Without `* 100`, an unsatisfiable hunger goal would propose explore at 0.3 instead
    /// of 30 — losing every dedup against any other proposal on the same intent.
    #[test]
    fn explore_fallback_urgency_uses_survival_scale() {
        // No plan and an unsatisfiable goal — drives the explore-fallback branch
        // (planner can't reach the goal with the registered Wander/Explore actions).
        let priority = 1.0;
        let unsatisfiable = Goal {
            conditions: vec![TriplePattern::new(
                Some(crate::agent::mind::knowledge::Node::Self_),
                Some(crate::agent::mind::knowledge::Predicate::Hunger),
                Some(crate::agent::mind::knowledge::Value::Int(0)),
            )],
            priority,
        };
        let mut cns = CentralNervousSystem::default();
        cns.urgencies
            .push(Urgency::new(UrgencySource::Hunger, priority));
        cns.current_goal = Some(unsatisfiable.clone());
        let brain = RationalBrain {
            current_plan: None,
            current_goal: Some(unsatisfiable),
            ..Default::default()
        };

        let proposal = propose(&brain, &cns);

        assert_eq!(proposal.action.action_type, ActionType::Explore);
        let expected = priority * EXPLORE_FALLBACK_PRIORITY_MULTIPLIER * 100.0;
        assert!(
            (proposal.urgency - expected).abs() < 0.01,
            "explore-fallback urgency should be {} (priority * mult * 100), got {}",
            expected,
            proposal.urgency
        );
    }

    /// Rational must NOT propose Explore for non-resource intents like Social.
    /// Exploring the map can't satisfy a social drive; proposing Explore here
    /// would dedup against (and post-units-fix outscore) the Emotional brain's
    /// own answer for the same intent, breaking conversation initiation.
    #[test]
    fn no_explore_fallback_when_intent_cannot_be_satisfied_by_exploration() {
        let priority = 0.8;
        let unsatisfiable = Goal {
            conditions: vec![TriplePattern::new(
                Some(crate::agent::mind::knowledge::Node::Self_),
                Some(crate::agent::mind::knowledge::Predicate::SocialDrive),
                Some(crate::agent::mind::knowledge::Value::Int(0)),
            )],
            priority,
        };
        let mut cns = CentralNervousSystem::default();
        cns.urgencies
            .push(Urgency::new(UrgencySource::Social, priority));
        cns.current_goal = Some(unsatisfiable.clone());
        let brain = RationalBrain {
            current_plan: None,
            current_goal: Some(unsatisfiable),
            ..Default::default()
        };

        // Build the same setup as `propose()` but tolerate `None`.
        let mut world = World::new();
        let mut state: SystemState<
            Query<(
                &GlobalTransform,
                Option<&crate::agent::affordance::Affordance>,
            )>,
        > = SystemState::new(&mut world);
        let affordances = state.get(&world);
        let mut registry = ActionRegistry::default();
        registry.register(crate::agent::actions::action::WanderAction);
        registry.register(crate::agent::actions::action::ExploreAction);
        let inventory = ItemSlots::agent_carry();
        let transform = Transform::default();
        let mind = MindGraph::default();
        let visible = crate::agent::mind::perception::VisibleObjects::default();
        let world_map = WorldMap::new(64, 64);

        let capacities = ChannelCapacities::full();
        let cost_ctx = crate::agent::brains::planner::PlanCostContext::neutral();
        let proposal = rational_brain_propose(
            &brain,
            &cns,
            &inventory,
            &transform,
            &mind,
            &visible,
            &world_map,
            &registry,
            &affordances,
            &capacities,
            &cost_ctx,
        );

        assert!(
            proposal.is_none(),
            "rational must defer (not propose Explore) when the goal can't be \
             satisfied by exploration; got: {proposal:?}"
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
    fn rational_propose_defers_when_plan_uncommitted() {
        let cns = cns_with_goal(1.0, UrgencySource::Hunger);
        let brain = RationalBrain {
            current_plan: Some(vec![template("WalkStep", ActionType::Walk)]),
            current_goal: cns.current_goal.clone(),
            plan_index: 0,
            plan_committed: false,
            ..Default::default()
        };

        // Build the full propose setup by hand so we can tolerate `None`.
        let mut world = World::new();
        let mut state: SystemState<
            Query<(
                &GlobalTransform,
                Option<&crate::agent::affordance::Affordance>,
            )>,
        > = SystemState::new(&mut world);
        let affordances = state.get(&world);
        let mut registry = ActionRegistry::default();
        registry.register(crate::agent::actions::action::WanderAction);
        registry.register(crate::agent::actions::action::ExploreAction);
        let inventory = ItemSlots::agent_carry();
        let transform = Transform::default();
        let mind = MindGraph::default();
        let visible = crate::agent::mind::perception::VisibleObjects::default();
        let world_map = WorldMap::new(64, 64);
        let capacities = ChannelCapacities::full();
        let cost_ctx = crate::agent::brains::planner::PlanCostContext::neutral();

        let proposal = rational_brain_propose(
            &brain,
            &cns,
            &inventory,
            &transform,
            &mind,
            &visible,
            &world_map,
            &registry,
            &affordances,
            &capacities,
            &cost_ctx,
        );

        assert!(
            proposal.is_none(),
            "uncommitted plan must not propose — got {proposal:?}"
        );
    }

    /// Idle wander (no plan, no goal) should still propose, untouched by the units fix.
    #[test]
    fn idle_wander_proposal_unchanged_by_urgency_units_fix() {
        let cns = CentralNervousSystem::default();
        let brain = RationalBrain::default();

        let proposal = propose(&brain, &cns);

        assert_eq!(proposal.action.action_type, ActionType::Wander);
        assert!(
            (proposal.urgency - IDLE_WANDER_URGENCY).abs() < 0.01,
            "idle wander urgency should equal IDLE_WANDER_URGENCY"
        );
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
