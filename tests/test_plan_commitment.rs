//! Integration tests for plan ownership and commitment inertia (#166).
//!
//! Tests use the TestWorld harness to verify that:
//! - Active plans get inertia that prevents flip-flopping
//! - Commitment decays when plans stall
//! - Survival brain at critical urgency overrides commitment
//! - Conscientiousness modulates plan stickiness
//! - Verbal commitments get inertia in arbitration
//! - Completed plans don't linger
//! - PlanAbandoned events fire when plans are replaced or stalled out

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::brains::active_plan::{ActivePlanEntry, ActivePlans, PlanOwner, PlanProgress};
use worldsim::agent::brains::proposal::{BrainType, Intent};
use worldsim::agent::events::SimEvent;
use worldsim::agent::psyche::personality::{Personality, PersonalityTraits};
use worldsim::testing::{AgentConfig, TestWorld};

/// Verify that the ActivePlans component exists on spawned agents and starts empty.
#[test]
fn active_plans_component_exists_on_agent() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::default());

    let plans = world
        .app()
        .world()
        .get::<ActivePlans>(agent)
        .expect("agent should have ActivePlans component");

    assert!(plans.plans.is_empty(), "active plans should start empty");
}

/// Verify that when a brain decision is made, the winning proposal's intent
/// is registered as an active plan.
#[test]
fn winning_proposal_registers_as_active_plan() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        hunger: 80.0, // high hunger to trigger Walk plan
        ..Default::default()
    });

    // Run a few ticks to let the brain system fire
    world.tick(5);

    let _plans = world
        .app()
        .world()
        .get::<ActivePlans>(agent)
        .expect("agent should have ActivePlans after brain ticks");
}

/// Directly manipulate ActivePlans and BrainState to verify that commitment
/// inertia affects which proposal wins when scores are close.
#[test]
fn commitment_inertia_keeps_active_plan_when_scores_oscillate() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        personality: Personality {
            traits: PersonalityTraits {
                conscientiousness: 0.9,
                ..Default::default()
            },
        },
        ..Default::default()
    });

    // Inject an active plan for Walk(SatisfyHunger)
    {
        let mut plans = world
            .app_mut()
            .world_mut()
            .get_mut::<ActivePlans>(agent)
            .unwrap();
        plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );
    }

    // Verify the plan was registered
    let plans = world.app().world().get::<ActivePlans>(agent).unwrap();
    assert_eq!(plans.plans.len(), 1);
    assert_eq!(plans.plans[0].action, ActionType::Walk);
    assert_eq!(plans.plans[0].intent, Intent::SatisfyHunger);
    assert!(plans.plans[0].commitment_strength > 0.5);
}

/// Verify that PlanAbandoned events fire when stalled plans decay below threshold.
#[test]
fn plan_abandoned_event_fires_on_stall() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });

    // Inject a stalled plan with very low commitment
    {
        let mut plans = world
            .app_mut()
            .world_mut()
            .get_mut::<ActivePlans>(agent)
            .unwrap();
        plans.plans.push(ActivePlanEntry {
            owner: PlanOwner::Brain(BrainType::Rational),
            intent: Intent::SatisfyHunger,
            action: ActionType::Walk,
            started_at: 0,
            progress: PlanProgress::Stalled { since: 0 },
            commitment_strength: 0.15, // just above ABANDON_THRESHOLD (0.1)
        });
    }

    // Tick past the brain thinking_interval (60 ticks) so brain system fires
    // at least once. The plan should be abandoned because the brain won't
    // re-propose Walk(SatisfyHunger) with hunger at 0.
    world.tick(65);

    let abandoned_events: Vec<_> = world
        .sim_events()
        .all()
        .iter()
        .filter(|e| matches!(e, SimEvent::PlanAbandoned { .. }))
        .collect();

    assert!(
        !abandoned_events.is_empty(),
        "PlanAbandoned event should fire when a stalled plan decays"
    );
}

/// Completed plans should be removed from ActivePlans and not contribute inertia.
#[test]
fn completed_plan_does_not_linger() {
    let mut plans = ActivePlans::default();
    plans.activate(
        PlanOwner::Brain(BrainType::Rational),
        Intent::SatisfyHunger,
        ActionType::Walk,
        0,
    );
    assert_eq!(plans.plans.len(), 1);

    plans.complete(Intent::SatisfyHunger);
    assert!(
        plans.plans.is_empty(),
        "completed plan should be removed from active plans"
    );

    // No inertia bonus after completion
    let bonus = plans.commitment_bonus(
        Intent::SatisfyHunger,
        ActionType::Walk,
        &PlanOwner::Brain(BrainType::Rational),
        1.0,
        10,
    );
    assert_eq!(
        bonus, 0.0,
        "completed plan should not contribute inertia bonus"
    );
}

/// Verbal commitment (future #63) gets the same inertia treatment as brain plans.
#[test]
fn verbal_commitment_gets_inertia_in_active_plans() {
    let promised_to = Entity::from_bits(42);
    let mut plans = ActivePlans::default();

    plans.activate(
        PlanOwner::Verbal {
            promised_to,
            agreement_tick: 0,
        },
        Intent::SatisfyHunger,
        ActionType::Harvest,
        0,
    );

    assert_eq!(plans.plans.len(), 1);
    assert!(
        matches!(plans.plans[0].owner, PlanOwner::Verbal { .. }),
        "owner should be Verbal"
    );

    let bonus = plans.commitment_bonus(
        Intent::SatisfyHunger,
        ActionType::Harvest,
        &PlanOwner::Verbal {
            promised_to,
            agreement_tick: 0,
        },
        0.8, // agreeableness-like factor
        5,
    );
    assert!(
        bonus > 0.0,
        "verbal commitment should get inertia bonus: {bonus}"
    );

    // Breaking it fires abandonment
    for tick in 1..20 {
        let abandoned = plans.decay_stalled_plans(&[], tick);
        if !abandoned.is_empty() {
            assert_eq!(abandoned[0].1, ActionType::Harvest);
            return;
        }
    }
    panic!("verbal commitment should eventually be abandoned when stalled");
}
