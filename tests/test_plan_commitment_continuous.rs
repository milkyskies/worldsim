//! Integration tests for continuous plan commitment.
//!
//! Commitment accumulates while a plan is being considered and only crosses
//! the threshold after several ticks. Until then, the rational brain defers
//! so arbitration can pick something else.

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::brains::rational::{
    COMMITMENT_BASELINE_PER_TICK, CommitmentTickInputs, RationalBrain, commitment_tick_delta,
    compute_commit_threshold,
};
use worldsim::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
use worldsim::agent::commitment::Commitments;
use worldsim::agent::mind::knowledge::{Concept, Node as MindNode, Predicate, Value};
use worldsim::agent::psyche::personality::{Personality, PersonalityTraits};
use worldsim::testing::{AgentConfig, TestWorld};

fn fake_walk_step() -> ActionTemplate {
    ActionTemplate {
        name: "FakeWalk".to_string(),
        action_type: ActionType::Walk,
        target_entity: None,
        target_position: Some(Vec2::new(32.0, 0.0)),
        preconditions: vec![],
        effects: vec![],
        consumes: vec![],
        base_cost: 0.0,
        locomotion_intensity: ActionType::Walk.default_locomotion_intensity(),
    }
}

/// Inject a hunger-style plan whose goal targets the `Apple` concept so
/// the commitment announcement path (which matches on the goal's concept)
/// can fire.
fn inject_plan(world: &mut TestWorld, agent: Entity, cost: f32) {
    let current_tick = world.current_tick();
    let mut brain = world
        .app_mut()
        .world_mut()
        .get_mut::<RationalBrain>(agent)
        .unwrap();
    brain.current_plan = Some(vec![fake_walk_step()]);
    brain.current_goal = Some(Goal {
        conditions: vec![TriplePattern::new(
            Some(MindNode::Self_),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Apple, 1)),
        )],
        priority: 0.5,
    });
    brain.plan_index = 0;
    brain.commitment = 0.0;
    brain.subjective_cost = cost;
    brain.plan_started_at = Some(current_tick);
    brain.plan_committed = false;
}

fn commitment(world: &TestWorld, agent: Entity) -> f32 {
    world
        .app()
        .world()
        .get::<RationalBrain>(agent)
        .unwrap()
        .commitment
}

fn is_committed(world: &TestWorld, agent: Entity) -> bool {
    world
        .app()
        .world()
        .get::<RationalBrain>(agent)
        .unwrap()
        .plan_committed
}

/// A freshly generated plan starts uncommitted and only crosses the
/// threshold after multiple ticks of consideration.
#[test]
fn solo_agent_commits_to_plan_within_a_handful_of_ticks() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        personality: Personality {
            traits: PersonalityTraits {
                conscientiousness: 0.5,
                neuroticism: 0.5,
                ..Default::default()
            },
        },
        ..Default::default()
    });

    inject_plan(&mut world, agent, 20.0);

    assert!(!is_committed(&world, agent), "plan must start uncommitted");

    // Tick once and confirm commitment has moved off zero.
    world.tick(1);
    assert!(
        commitment(&world, agent) > 0.0,
        "commitment should accumulate after a single tick, got {}",
        commitment(&world, agent)
    );

    // Tick a handful more and confirm threshold gets crossed.
    world.tick(20);
    assert!(
        is_committed(&world, agent),
        "solo agent should have committed after ~20 ticks (commitment={}, threshold={})",
        commitment(&world, agent),
        compute_commit_threshold(20.0, 0.5),
    );
}

/// Anxious agents take more ticks to commit than stoic ones in identical
/// conditions — neuroticism penalises commitment buildup.
#[test]
fn neurotic_agent_commits_slower_than_stoic() {
    let mut world = TestWorld::with_seed(42);

    let stoic = world.spawn_agent(AgentConfig {
        pos: Vec2::new(10.0, 10.0),
        personality: Personality {
            traits: PersonalityTraits {
                neuroticism: 0.0,
                conscientiousness: 0.5,
                ..Default::default()
            },
        },
        ..Default::default()
    });
    let neurotic = world.spawn_agent(AgentConfig {
        pos: Vec2::new(100.0, 100.0),
        personality: Personality {
            traits: PersonalityTraits {
                neuroticism: 1.0,
                conscientiousness: 0.5,
                ..Default::default()
            },
        },
        ..Default::default()
    });

    inject_plan(&mut world, stoic, 100.0);
    inject_plan(&mut world, neurotic, 100.0);

    world.tick(10);

    let stoic_commitment = commitment(&world, stoic);
    let neurotic_commitment = commitment(&world, neurotic);
    assert!(
        stoic_commitment > neurotic_commitment,
        "stoic should outpace neurotic on identical plans \
         (stoic={stoic_commitment}, neurotic={neurotic_commitment})"
    );
}

/// Sharing the plan in conversation must accelerate the sharer's
/// commitment. We simulate the conversation outcome by inserting a
/// `Commitments` entry for the plan's goal concept dated after the plan
/// started — this is exactly what `communication.rs` writes when the
/// agent's Share/Ask/Answer turn references the active goal.
#[test]
fn announced_plan_commits_faster_than_silent_plan() {
    let mut world = TestWorld::with_seed(42);

    let silent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(10.0, 10.0),
        personality: Personality {
            traits: PersonalityTraits {
                conscientiousness: 0.5,
                neuroticism: 0.5,
                ..Default::default()
            },
        },
        ..Default::default()
    });
    let announced = world.spawn_agent(AgentConfig {
        pos: Vec2::new(100.0, 100.0),
        personality: Personality {
            traits: PersonalityTraits {
                conscientiousness: 0.5,
                neuroticism: 0.5,
                ..Default::default()
            },
        },
        ..Default::default()
    });

    inject_plan(&mut world, silent, 100.0);
    inject_plan(&mut world, announced, 100.0);

    // Record a verbal commitment on the announcer's `Commitments` that
    // lands AFTER the plan's start tick. `update_rational_brain` should
    // read this and apply the announcement bonus to that agent only.
    let now = world.current_tick();
    let mut commitments = world
        .app_mut()
        .world_mut()
        .get_mut::<Commitments>(announced)
        .expect("agent should have Commitments component");
    commitments.add(Concept::Apple, now);

    world.tick(1);

    let silent_commitment = commitment(&world, silent);
    let announced_commitment = commitment(&world, announced);
    assert!(
        announced_commitment > silent_commitment,
        "announced plan must accumulate commitment faster than silent one \
         (silent={silent_commitment}, announced={announced_commitment})"
    );
}

/// Pure-function sanity check of the commitment math: the documented
/// "solo + neutral personality" scenario from the issue description
/// crosses a minimum threshold in roughly five ticks.
#[test]
fn documented_solo_scenario_crosses_minimum_threshold_in_a_few_ticks() {
    let inputs = CommitmentTickInputs {
        urgency: 0.0,
        alone: true,
        announcement_made: false,
        neuroticism: 0.5,
        conscientiousness: 0.5,
    };
    let delta = commitment_tick_delta(&inputs);
    assert!(
        delta > COMMITMENT_BASELINE_PER_TICK,
        "alone bonus must lift per-tick delta above the baseline"
    );

    let threshold = compute_commit_threshold(50.0, 0.5);
    let mut commitment = 0.0;
    let mut ticks = 0;
    while commitment < threshold && ticks < 50 {
        commitment += delta;
        ticks += 1;
    }
    assert!(
        ticks <= 10,
        "solo neutral agent should cross a minimum threshold within ~10 ticks (took {ticks})"
    );
}
