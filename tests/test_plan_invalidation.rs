//! Verifies that when a plan step's preconditions fail mid-cycle, the execution
//! system stops re-proposing the stale action immediately — not deferred to the
//! next thinking interval (up to 60 ticks later).

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::brains::proposal::{BrainPowers, BrainState, BrainType};
use worldsim::agent::brains::rational::RationalBrain;
use worldsim::agent::brains::thinking::{ActionTemplate, TriplePattern};
use worldsim::agent::mind::knowledge::{Node, Predicate};
use worldsim::agent::nervous_system::config::NervousSystemConfig;
use worldsim::testing::{AgentConfig, TestWorld};

fn failing_harvest_template() -> ActionTemplate {
    // Entity::from_bits(1) is a placeholder entity that does not exist in the world.
    // Any MindGraph query for it returns empty, so the precondition always fails.
    let absent_entity = Entity::from_bits(1);
    let precondition = TriplePattern::new(
        Some(Node::Entity(absent_entity)),
        Some(Predicate::Contains),
        None,
    );
    ActionTemplate {
        name: "Harvest".into(),
        action_type: ActionType::Harvest,
        target_entity: Some(absent_entity),
        target_position: None,
        preconditions: vec![precondition],
        effects: vec![],
        consumes: vec![],
        base_cost: 1.0,
        locomotion_intensity: 0.0,
    }
}

#[test]
fn plan_invalidation_clears_stale_chosen_actions_immediately() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());

    // Push the thinking interval far out so the test is not sensitive to
    // which tick the entity's thinking fires — plan verification must clear
    // chosen_actions without waiting for the next thinking cycle.
    world
        .app_mut()
        .world_mut()
        .resource_mut::<NervousSystemConfig>()
        .thinking_interval = 10_000;

    // Advance past tick 0 before injecting state. At tick 0, entity 0's thinking
    // interval satisfies (0 + entity_id=0) % interval == 0 for ALL intervals,
    // which would fire three_brains_system and overwrite chosen_actions immediately.
    world.tick(1);

    let stale = failing_harvest_template();

    // Inject a stale plan with a failing precondition and the matching stale
    // BrainState so the execution system would otherwise re-propose Harvest.
    {
        let world_mut = world.app_mut().world_mut();

        let mut rb = world_mut
            .get_mut::<RationalBrain>(agent)
            .expect("agent should have RationalBrain");
        rb.current_plan = Some(vec![stale.clone()]);
        rb.plan_index = 0;

        let mut bs = world_mut
            .get_mut::<BrainState>(agent)
            .expect("agent should have BrainState");
        bs.chosen_actions = vec![stale];
        bs.winner = Some(BrainType::Rational);
        bs.powers = BrainPowers {
            survival: 0.0,
            emotional: 0.0,
            rational: 1.0,
        };
    }

    // Sanity: state is set before ticking.
    assert!(
        world
            .app()
            .world()
            .get::<BrainState>(agent)
            .map(|bs| !bs.chosen_actions.is_empty())
            .unwrap_or(false),
        "chosen_actions should be non-empty before tick"
    );

    // One tick: plan verification fires (every tick), detects failing precondition.
    // chosen_actions must be cleared in this same tick, not after the next
    // thinking interval (10 000 ticks away).
    world.tick(1);

    let still_proposing = world
        .app()
        .world()
        .get::<BrainState>(agent)
        .map(|bs| !bs.chosen_actions.is_empty())
        .unwrap_or(false);

    assert!(
        !still_proposing,
        "chosen_actions should be cleared immediately when plan precondition fails, \
         not deferred to the next thinking interval"
    );

    let plan_cleared = world
        .app()
        .world()
        .get::<RationalBrain>(agent)
        .map(|rb| rb.current_plan.is_none())
        .unwrap_or(true);

    assert!(
        plan_cleared,
        "plan should also be cleared after precondition failure"
    );
}
