//! Verifies that when the brain specifies a target position for a movement
//! action, the execution system honours that position rather than silently
//! discarding or overwriting it.

use bevy::prelude::*;
use worldsim::agent::TargetPosition;
use worldsim::agent::actions::ActionType;
use worldsim::agent::actions::registry::ActiveActions;
use worldsim::agent::brains::plan_memory::{HeldPlan, PlanMemory, PlanSource, PlanState};
use worldsim::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
use worldsim::agent::mind::knowledge::{Concept, Node as MindNode, Predicate, Value};
use worldsim::testing::{AgentConfig, TestWorld};

fn make_walk_template(target: Vec2) -> ActionTemplate {
    ActionTemplate {
        name: "Walk".into(),
        action_type: ActionType::Walk,
        target_entity: None,
        target_position: Some(target),
        preconditions: vec![],
        effects: vec![],
        consumes: vec![],
        base_cost: 1.0,
        locomotion_intensity: ActionType::Walk.default_locomotion_intensity(),
    }
}

#[test]
fn brain_walk_target_is_used_by_execution() {
    let brain_target = Vec2::new(200.0, 200.0);

    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });

    // Inject a high-priority Executing plan whose current step is a Walk
    // targeting `brain_target`. Use `VerbalCommitment` so the rational
    // brain's every-tick stale-plan sweep (#424) doesn't nuke it for
    // not matching the CNS current_goal. The rational brain surfaces
    // Executing-plan steps as proposals every tick; arbitration admits
    // the Walk and `start_actions` reads it out into `TargetPosition`.
    let goal = Goal {
        conditions: vec![TriplePattern::new(
            Some(MindNode::Self_),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Apple, 1)),
        )],
        priority: 1.0,
    };
    {
        let mut memory = world
            .app_mut()
            .world_mut()
            .get_mut::<PlanMemory>(agent)
            .expect("agent should have PlanMemory");
        let id = memory.mint_plan_id();
        memory.insert(HeldPlan {
            id,
            goal,
            steps: vec![make_walk_template(brain_target)],
            state: PlanState::Executing,
            commitment: 10.0,
            subjective_cost: 10.0,
            source: PlanSource::VerbalCommitment {
                promised_to: Entity::from_bits(1),
                agreement_tick: 0,
            },
            created_at: 0,
            last_touched: 0,
            current_step: 0,
        });
    }

    // One tick is enough: arbitration surfaces the plan's Walk step as
    // the winning proposal, and `start_actions` reads the target into
    // `TargetPosition`.
    world.tick(1);

    let target_pos = world
        .app()
        .world()
        .get::<TargetPosition>(agent)
        .expect("agent should have TargetPosition");

    assert_eq!(
        target_pos.0,
        Some(brain_target),
        "execution system should use the brain's Walk target, not discard it"
    );

    let is_walking = world
        .app()
        .world()
        .get::<ActiveActions>(agent)
        .map(|a| a.contains(ActionType::Walk))
        .unwrap_or(false);

    assert!(is_walking, "Walk action should be running after one tick");
}
