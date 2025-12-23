use crate::agent::actions::ActionType;
use crate::agent::body::needs::Consciousness;
use crate::agent::brains::proposal::{BrainProposal, BrainType};
use crate::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
use crate::agent::inventory::Inventory;
use crate::agent::mind::knowledge::{MindGraph, Node, Predicate, Value};
use crate::agent::mind::perception::VisibleObjects;
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
        &Consciousness,
        &Inventory,
        &Transform,
        &VisibleObjects,
        &crate::agent::nervous_system::cns::CentralNervousSystem,
        &MindGraph,
    )>,
    tick: Res<crate::core::tick::TickCount>,
    ns_config: Res<crate::agent::nervous_system::config::NervousSystemConfig>,
    world_map: Res<WorldMap>,
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

    for (entity, mut brain, consciousness, _inventory, transform, _visible, cns, mind) in
        query.iter_mut()
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
        }

        // 2. Heavy Thinking (Replanning)
        if !tick.should_run(entity, ns_config.thinking_interval) {
            continue;
        }

        // CONSCIOUSNESS CHECK: Can't plan while asleep
        if consciousness.alertness < 0.3 {
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
            let mut actions = Vec::new();

            // Generate actions from registry based on target type
            for action in action_registry.all() {
                use crate::agent::actions::TargetType;

                match action.target_type() {
                    TargetType::None => {
                        // Global actions: Sleep, Eat, Wander, Idle, Explore
                        actions.push(action.to_template(None, None));
                    }
                    TargetType::Entity => {
                        // Entity-targeted actions: Harvest, Introduce, Talk

                        // 1. Find resource targets (Harvest, Attack)
                        let known_resources = mind.query(None, Some(Predicate::Contains), None);
                        let mut processed_entities = std::collections::HashSet::new();

                        for triple in known_resources {
                            if let Node::Entity(target_entity) = triple.subject {
                                if processed_entities.contains(&target_entity) {
                                    continue;
                                }

                                if let Ok((target_transform, maybe_affordance)) =
                                    affordances.get(target_entity)
                                {
                                    processed_entities.insert(target_entity);

                                    let vis_pos = target_transform.translation().truncate();

                                    // Check if this entity affords this action type
                                    if let Some(affordance) = maybe_affordance
                                        && affordance.action_type == action.action_type()
                                    {
                                        // Calculate confidence and cost
                                        let belief_state =
                                            crate::agent::mind::belief_state::BeliefState::new(
                                                mind,
                                            );
                                        let pattern = TriplePattern::new(
                                            Some(Node::Entity(target_entity)),
                                            Some(Predicate::Contains),
                                            None,
                                        );
                                        let confidence = belief_state.pattern_confidence(&pattern);

                                        if confidence > 0.1 {
                                            actions.push(
                                                action.to_template(
                                                    Some(target_entity),
                                                    Some(vis_pos),
                                                ),
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // 2. Find social targets (Introduce, Talk) - other perceived agents
                        if action.action_type() == ActionType::Introduce
                            || action.action_type() == ActionType::Talk
                        {
                            // Query for perceived people (set by social_perception system)
                            let perceived_people = mind.query(
                                None,
                                Some(Predicate::IsA),
                                Some(&Value::Concept(
                                    crate::agent::mind::knowledge::Concept::Person,
                                )),
                            );

                            for triple in perceived_people {
                                if let Node::Entity(agent_entity) = triple.subject {
                                    // Skip self
                                    if agent_entity == entity {
                                        continue;
                                    }

                                    if processed_entities.contains(&agent_entity) {
                                        continue;
                                    }

                                    // Get position from affordances query
                                    if let Ok((target_transform, _)) = affordances.get(agent_entity)
                                    {
                                        processed_entities.insert(agent_entity);
                                        let vis_pos = target_transform.translation().truncate();

                                        // For Introduce: prefer strangers
                                        // For Talk: prefer known people
                                        let is_stranger = mind
                                            .query(
                                                Some(&Node::Entity(agent_entity)),
                                                Some(Predicate::Knows),
                                                Some(&Value::Boolean(true)),
                                            )
                                            .is_empty(); // invert: empty = stranger

                                        let should_add = match action.action_type() {
                                            ActionType::Introduce => is_stranger,
                                            ActionType::Talk => !is_stranger,
                                            _ => true,
                                        };

                                        if should_add {
                                            actions.push(
                                                action
                                                    .to_template(Some(agent_entity), Some(vis_pos)),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    TargetType::Position => {
                        // Position-targeted actions: Walk
                        // Usually generated implicitly by regressive planner, skip for now
                    }
                }
            }

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

            if let Some(plan) = crate::agent::brains::planner::regressive_plan(
                mind,
                goal,
                &actions,
                &action_registry,
            ) {
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
    _inventory: &Inventory, // Kept but unused
    transform: &Transform,
    mind: &MindGraph,
    visible: &crate::agent::mind::perception::VisibleObjects,
    world_map: &WorldMap,
    action_registry: &crate::agent::actions::ActionRegistry,
    affordances: &Query<(
        &GlobalTransform,
        Option<&crate::agent::affordance::Affordance>,
    )>,
) -> Option<BrainProposal> {
    if let Some(plan) = &brain.current_plan
        && brain.plan_index < plan.len()
    {
        let action = &plan[brain.plan_index];
        // Re-verify preconditions before proposing (e.g., still have food to eat?)
        if are_preconditions_met(action, mind) {
            return Some(BrainProposal {
                brain: BrainType::Rational,
                action: action.clone(),
                urgency: 30.0,
                reasoning: format!("Continuing plan step {}: {}", brain.plan_index, action.name),
            });
        }
        // Preconditions no longer met - fall through to replan
    }

    if let Some(goal) = &cns.current_goal {
        let mut actions = Vec::new();
        let agent_pos = transform.translation.truncate();

        // GENERIC: Generate actions from registry based on target type
        for action in action_registry.all() {
            use crate::agent::actions::TargetType;

            match action.target_type() {
                TargetType::None => {
                    actions.push(action.to_template(None, None));
                }
                TargetType::Entity => {
                    // Look for affordances
                    let known_resources = mind.query(None, Some(Predicate::Contains), None);
                    let mut processed_entities = std::collections::HashSet::new();

                    for triple in known_resources {
                        if let Node::Entity(entity) = triple.subject {
                            if processed_entities.contains(&entity) {
                                continue;
                            }

                            if let Ok((vis_transform, maybe_affordance)) = affordances.get(entity) {
                                processed_entities.insert(entity);
                                let vis_pos = vis_transform.translation().truncate();

                                // Check 1: Does the entity physically afford this action?
                                // If the entity has an Affordance component, it must match.
                                // If it has NO Affordance component, we assume it doesn't afford anything (unless implicit?)
                                // For now, let's assume specific actions require specific affordances.
                                if let Some(affordance) = maybe_affordance {
                                    if affordance.action_type != action.action_type() {
                                        continue;
                                    }
                                } else {
                                    // No affordance component - assume it doesn't support interaction actions
                                    // (Unless we want to allow actions on everything, but that's risky)
                                    continue;
                                }

                                // Check 2: Does our KNOWLEDGE allow this action? (is_plan_valid)
                                if !action.is_plan_valid(Some(entity), mind) {
                                    continue;
                                }

                                // If we passed checks, calculate cost and propose
                                let dist = agent_pos.distance(vis_pos);
                                let mut template = action.to_template(Some(entity), Some(vis_pos));
                                template.base_cost += dist;
                                actions.push(template);
                            }
                        }
                    }
                }
                TargetType::Position => {}
            }
        }

        if let Some(plan) =
            crate::agent::brains::planner::regressive_plan(mind, goal, &actions, action_registry)
        {
            if let Some(first_action) = plan.first() {
                return Some(BrainProposal {
                    brain: BrainType::Rational,
                    action: first_action.clone(),
                    urgency: goal.priority,
                    reasoning: format!("New plan for goal: {:?}", goal.conditions),
                });
            } else {
                let wander_action = action_registry
                    .get(ActionType::Wander)
                    .map(|a| a.to_template(None, None))
                    .expect("Wander action must be registered");
                return Some(BrainProposal {
                    brain: BrainType::Rational,
                    action: wander_action,
                    urgency: 5.0,
                    reasoning: "Goal already satisfied, wandering".to_string(),
                });
            }
        }

        // EPISTEMIC: Before exploring, try asking a nearby known agent
        // Check if any agents are visible that we know (introduced to)
        for visible_entity in visible.entities.iter() {
            // Check if this is a known agent
            let is_known = !mind
                .query(
                    Some(&Node::Entity(*visible_entity)),
                    Some(Predicate::Knows),
                    Some(&Value::Boolean(true)),
                )
                .is_empty();

            if is_known {
                // Get their position for the action template
                if let Ok((target_transform, _)) = affordances.get(*visible_entity) {
                    let vis_pos = target_transform.translation().truncate();

                    // Determine what we need based on goal
                    let needed_concept = goal.conditions.iter().find_map(|cond| {
                        if let Some(Value::Item(concept, _)) = &cond.object {
                            Some(*concept)
                        } else {
                            None
                        }
                    });

                    if let (Some(talk_action), Some(concept)) =
                        (action_registry.get(ActionType::Talk), needed_concept)
                    {
                        let mut template =
                            talk_action.to_template(Some(*visible_entity), Some(vis_pos));
                        template.topic =
                            Some(crate::agent::mind::conversation::Topic::Location(concept));
                        // No content - we're asking, not sharing

                        return Some(BrainProposal {
                            brain: BrainType::Rational,
                            action: template,
                            urgency: goal.priority * 0.5,
                            reasoning: format!("Asking {:?} about {:?}", visible_entity, concept),
                        });
                    }
                }
            }
        }

        // Fallback: Explore to find resources ourselves
        let explore_action = action_registry
            .get(ActionType::Explore)
            .map(|a| a.to_template(None, None))
            .expect("Explore action must be registered");

        return Some(BrainProposal {
            brain: BrainType::Rational,
            action: explore_action,
            urgency: goal.priority * 0.3,
            reasoning: "Can't plan - exploring for resources".to_string(),
        });
    }

    let wander_action = action_registry
        .get(ActionType::Wander)
        .map(|a| a.to_template(None, None))
        .expect("Wander action must be registered");
    Some(BrainProposal {
        brain: BrainType::Rational,
        action: wander_action,
        urgency: 10.0,
        reasoning: "Nothing to do, wandering".to_string(),
    })
}
