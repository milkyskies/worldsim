//! Verifies that when a plan step's preconditions fail mid-cycle, the execution
//! system stops re-proposing the stale action immediately — not deferred to the
//! next thinking interval. Post-#424 arbitration runs every tick so a failed
//! plan is both removed from memory and replaced in BrainState the same tick
//! the invalidation fires.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::brains::plan_memory::{
    HeldPlan, PlanAbandonReason, PlanMemory, PlanSource, PlanState,
};
use worldsim::agent::brains::proposal::{BrainPowers, BrainState, BrainType};
use worldsim::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
use worldsim::agent::events::SimEventKind;
use worldsim::agent::mind::knowledge::{Node, Predicate};
use worldsim::agent::nervous_system::config::NervousSystemConfig;
use worldsim::agent::nervous_system::urgency::UrgencySource;
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
        behavior: Default::default(),
        target_entity: Some(absent_entity),
        target_position: None,
        preconditions: vec![precondition],
        effects: vec![],
        consumes: vec![],
        base_cost: 1.0,
        locomotion_intensity: 0.0,
        estimated_duration_ticks: None,
        search_filter: None,
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

    // Inject a stale Executing plan with a failing precondition and the
    // matching stale BrainState so the execution system would otherwise
    // re-propose Harvest.
    {
        let world_mut = world.app_mut().world_mut();

        let mut memory = world_mut
            .get_mut::<PlanMemory>(agent)
            .expect("agent should have PlanMemory");
        let id = memory.mint_plan_id();
        memory.insert(HeldPlan {
            id,
            goal: Goal {
                conditions: Vec::new(),
                priority: 1.0,
            },
            steps: vec![stale.clone()],
            state: PlanState::Executing,
            commitment: 10.0,
            subjective_cost: 0.0,
            source: PlanSource::Brain(BrainType::Rational),
            driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Hunger,
            created_at_urgency: 1.0,
            created_at: 0,
            last_touched: 0,
            current_step: 0,
        });

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

    // One tick: plan verification fires (every tick), detects failing precondition,
    // removes the plan. Arbitration — also every tick post-#424 — sees no
    // Rational plan in Executing and the BrainState.chosen_actions list no
    // longer contains the stale Harvest. (The slot may be filled by whatever
    // other brain wins this tick; the invariant that matters is "the invalid
    // action is gone.")
    world.tick(1);

    let still_proposing_stale = world
        .app()
        .world()
        .get::<BrainState>(agent)
        .map(|bs| {
            bs.chosen_actions
                .iter()
                .any(|a| a.action_type == ActionType::Harvest)
        })
        .unwrap_or(false);

    assert!(
        !still_proposing_stale,
        "the stale Harvest action must be gone from BrainState.chosen_actions \
         in the same tick the precondition failure was detected, not deferred \
         to the next thinking interval"
    );

    let plan_cleared = world
        .app()
        .world()
        .get::<PlanMemory>(agent)
        .map(|memory| memory.in_state(PlanState::Executing).next().is_none())
        .unwrap_or(true);

    assert!(
        plan_cleared,
        "the broken Executing plan should be removed from PlanMemory \
         after precondition failure"
    );

    let abandoned = world.sim_events().all().iter().any(|e| {
        matches!(
            &e.kind,
            SimEventKind::PlanAbandoned {
                reason: PlanAbandonReason::PreconditionsUnmet,
                ..
            }
        )
    });

    assert!(
        abandoned,
        "PlanAbandoned with PreconditionsUnmet must fire when the verify \
         pass drops a plan whose current step's preconditions broke"
    );
}

/// Stale-plan sweep must emit PlanAbandoned(DrivingUrgencyStale) when a
/// plan's driving urgency has decayed below the relative-fraction cutoff
/// but the plan has not yet made progress (so the looser engaged cutoff
/// does not apply).
#[test]
fn retain_sweep_emits_plan_abandoned_for_stale_urgency() {
    let mut world = TestWorld::with_seed(7);
    let agent = world.spawn_agent(AgentConfig::default());

    world
        .app_mut()
        .world_mut()
        .resource_mut::<NervousSystemConfig>()
        .thinking_interval = 1;

    world.tick(1);

    // Inject an Executing Rational plan whose driving_urgency is
    // `Thirst` and whose `created_at_urgency` is a high value the CNS
    // never reports (Thirst is not spawned at 0.9 by default). That
    // mismatch triggers the relative-decay drop path.
    {
        let world_mut = world.app_mut().world_mut();
        let mut memory = world_mut
            .get_mut::<PlanMemory>(agent)
            .expect("agent should have PlanMemory");
        let id = memory.mint_plan_id();
        memory.insert(HeldPlan {
            id,
            goal: Goal {
                conditions: Vec::new(),
                priority: 1.0,
            },
            // One harmless step — an action the agent will not be chosen to
            // execute. The plan just needs to survive until the retain
            // sweep runs.
            steps: vec![ActionTemplate {
                name: "Idle".into(),
                action_type: ActionType::Idle,
                behavior: Default::default(),
                target_entity: None,
                target_position: None,
                preconditions: vec![],
                effects: vec![],
                consumes: vec![],
                base_cost: 0.0,
                locomotion_intensity: 0.0,
                estimated_duration_ticks: None,
                search_filter: None,
            }],
            state: PlanState::Executing,
            commitment: 1.0,
            subjective_cost: 0.0,
            source: PlanSource::Brain(BrainType::Rational),
            driving_urgency: UrgencySource::Thirst,
            // Creation-time urgency much higher than current CNS value,
            // so the relative rule drops the plan immediately.
            created_at_urgency: 0.9,
            created_at: 0,
            last_touched: 0,
            current_step: 0,
        });
    }

    world.tick(2);

    let abandoned_stale = world.sim_events().all().iter().any(|e| {
        matches!(
            &e.kind,
            SimEventKind::PlanAbandoned {
                driving_urgency: UrgencySource::Thirst,
                reason: PlanAbandonReason::DrivingUrgencyStale
                    | PlanAbandonReason::DrivingUrgencyMissing,
                ..
            }
        )
    });

    assert!(
        abandoned_stale,
        "retain sweep must emit PlanAbandoned with a urgency-based \
         reason when a Rational plan's driving urgency drops below the \
         relative cutoff"
    );
}
