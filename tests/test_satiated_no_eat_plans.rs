//! Satiated agents with non-zero Hunger urgency must not generate plans
//! ending in Eat. Without the plan-time satiation filter the rational brain
//! emits `Walk → Harvest → Eat` plans that die at runtime with
//! `AlreadySatiated` and respawn every tick while digestion catches up.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::body::needs::PhysicalNeeds;
use worldsim::agent::brains::plan_memory::PlanMemory;
use worldsim::agent::events::{FailureReason, SimEventKind};
use worldsim::agent::item_slots::ItemSlots;
use worldsim::agent::mind::knowledge::Concept;
use worldsim::agent::nervous_system::cns::CentralNervousSystem;
use worldsim::agent::nervous_system::urgency::{Urgency, UrgencySource};
use worldsim::testing::{AgentConfig, TestWorld};

/// Hold Alice's CNS at `Hunger = 0.3`. The sim's own urgency generator
/// would otherwise zero Hunger out once it observes the full stomach, and
/// the test would trivially pass for the wrong reason.
fn clobber_hunger(world: &mut TestWorld, agent: Entity) {
    let mut cns = world.get_mut::<CentralNervousSystem>(agent);
    cns.urgencies.clear();
    cns.urgencies.push(Urgency::new(UrgencySource::Hunger, 0.3));
}

#[test]
fn satiated_agent_generates_no_eat_plans_despite_hunger_urgency() {
    let mut world = TestWorld::with_seed(42);
    let alice = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });

    // Freeze Alice at "just ate" — stomach full, inventory has food.
    world.get_mut::<PhysicalNeeds>(alice).metabolism = Metabolism::well_fed();
    world.get_mut::<ItemSlots>(alice).add(Concept::Apple, 1);

    // Tick-by-tick so the CNS clobber is invariant across the whole
    // window — not just at the boundaries. Ensures the sim never gets a
    // chance to silently zero Hunger during a tick-block.
    for _ in 0..1000 {
        clobber_hunger(&mut world, alice);
        world.tick(1);
    }

    let hunger_plans = world
        .sim_events()
        .all()
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                SimEventKind::PlanGenerated {
                    driving_urgency: UrgencySource::Hunger,
                    ..
                }
            ) && event.involves(alice)
        })
        .count();

    assert_eq!(
        hunger_plans, 0,
        "rational brain must not generate Hunger plans while the agent is \
         already satiated — the plan-time satiation filter should keep Eat \
         out of the GOAP candidate set"
    );

    let already_satiated_failures = world
        .sim_events()
        .all()
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                SimEventKind::ActionFailed {
                    action: ActionType::Eat,
                    reason: FailureReason::AlreadySatiated { .. },
                    ..
                }
            ) && event.involves(alice)
        })
        .count();

    assert_eq!(
        already_satiated_failures, 0,
        "the execution-layer AlreadySatiated gate must not fire — a \
         properly-filtered planner never proposes the doomed action"
    );

    let memory = world.get::<PlanMemory>(alice);
    let has_hunger_plan_ending_in_eat = memory.plans.iter().any(|p| {
        p.driving_urgency == UrgencySource::Hunger
            && p.steps
                .last()
                .is_some_and(|s| s.action_type == ActionType::Eat)
    });
    assert!(
        !has_hunger_plan_ending_in_eat,
        "PlanMemory must not hold a Hunger plan whose terminal step is Eat \
         while the agent is already satiated"
    );
}
