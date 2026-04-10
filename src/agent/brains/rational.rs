//! Rational brain: deliberate goal-directed planning via GOAP.
//!
//! Reads: RationalBrain, Consciousness, ItemSlots, MindGraph, VisibleObjects, CentralNervousSystem
//! Writes: RationalBrain (plan/goal), BrainProposal
//! Upstream: cns (current_goal), planner (regressive_plan), mind (MindGraph)
//! Downstream: brains::proposal (winner selection)

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{ChannelCapacities, ChannelLoad};
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
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
        mut consciousness,
        _inventory,
        _transform,
        _visible,
        cns,
        mind,
        body,
        physical,
        personality,
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
    capacities: &ChannelCapacities,
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

        if let Some(plan) = crate::agent::brains::planner::regressive_plan(mind, goal, &actions) {
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
        RationalBrain {
            current_plan: Some(vec![template("FakeStep", ActionType::Idle)]),
            current_goal: goal,
            plan_index: 0,
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
            plan_index: 0,
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
            plan_index: 0,
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
        );

        assert!(
            proposal.is_none(),
            "rational must defer (not propose Explore) when the goal can't be \
             satisfied by exploration; got: {proposal:?}"
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
