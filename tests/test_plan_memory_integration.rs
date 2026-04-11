//! Integration tests for multi-plan cognitive memory (#338).
//!
//! Covers the behaviours the issue calls out as requirements that are
//! only observable at the full ECS level: holding plans across multiple
//! brain cycles, verbal commitments surviving urgency spikes, and the
//! cognitive-load cap trimming excess background plans.

use bevy::prelude::*;
use worldsim::agent::brains::plan_memory::{
    HeldPlan, PlanMemory, PlanSource, PlanState, max_plans_for,
};
use worldsim::agent::brains::proposal::BrainType;
use worldsim::agent::brains::thinking::{Goal, TriplePattern};
use worldsim::agent::mind::knowledge::{Concept, Node as MindNode, Predicate, Value};
use worldsim::agent::psyche::personality::{Personality, PersonalityTraits};
use worldsim::testing::{AgentConfig, TestWorld};

fn hunger_goal(priority: f32, concept: Concept) -> Goal {
    Goal {
        conditions: vec![TriplePattern::new(
            Some(MindNode::Self_),
            Some(Predicate::Contains),
            Some(Value::Item(concept, 1)),
        )],
        priority,
    }
}

fn inject_verbal_commitment(
    world: &mut TestWorld,
    agent: Entity,
    concept: Concept,
    promised_to: Entity,
    tick: u64,
) {
    let mut memory = world
        .app_mut()
        .world_mut()
        .get_mut::<PlanMemory>(agent)
        .expect("agent must have PlanMemory");
    let id = memory.mint_plan_id();
    memory.insert(HeldPlan {
        id,
        goal: hunger_goal(0.5, concept),
        steps: Vec::new(),
        state: PlanState::Background,
        commitment: 0.2,
        subjective_cost: 40.0,
        source: PlanSource::VerbalCommitment {
            promised_to,
            agreement_tick: tick,
        },
        created_at: tick,
        last_touched: tick,
        current_step: 0,
    });
}

/// The cognitive-load cap from PlanMemory is personality-modulated.
/// Spawning an agent with known traits and asserting the cap numerically
/// keeps the eviction formula honest if anyone retunes the constants.
#[test]
fn cognitive_load_cap_respects_personality() {
    let spontaneous = max_plans_for(0.0, 0.0, 1.0);
    let balanced = max_plans_for(0.5, 0.5, 0.5);
    let disciplined = max_plans_for(1.0, 1.0, 0.0);

    assert!(disciplined > balanced);
    assert!(balanced >= spontaneous);
    assert!(
        disciplined >= 6,
        "a highly open + conscientious agent should hold at least 6 plans, got {disciplined}"
    );
}

/// A verbal commitment plan injected into the agent's memory must
/// survive multiple brain cycles even when no urgency drives it — the
/// whole point of persisting commitments across distractions.
#[test]
fn verbal_commitment_persists_across_thinking_cycles() {
    let mut world = TestWorld::with_seed(42);
    let alice = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        hunger: 10.0,
        personality: Personality {
            traits: PersonalityTraits {
                conscientiousness: 0.8,
                ..Default::default()
            },
        },
        ..Default::default()
    });
    let bob = world.spawn_agent(AgentConfig {
        pos: Vec2::new(500.0, 500.0),
        ..Default::default()
    });

    // Record a verbal commitment on Alice's PlanMemory targeting
    // `Campfire`. With low hunger there's nothing urgent to compete
    // with it, so the verbal plan should still be in memory a few
    // thinking cycles later.
    inject_verbal_commitment(&mut world, alice, Concept::Campfire, bob, 1);

    // Tick through several thinking cycles (thinking_interval = 60).
    world.tick(240);

    let memory = world.get::<PlanMemory>(alice);
    let has_verbal = memory.plans.iter().any(|p| {
        matches!(p.source, PlanSource::VerbalCommitment { .. })
            && p.goal.target_concept() == Some(Concept::Campfire)
    });
    assert!(
        has_verbal,
        "verbal commitment to Campfire must survive 240 ticks of idle time"
    );
}

/// A verbal commitment must also survive an unrelated urgent plan
/// spiking and being satisfied. Hunger burst → commitment should still
/// be there at the end.
#[test]
fn verbal_commitment_survives_unrelated_urgency_spike() {
    let mut world = TestWorld::with_seed(42);
    let alice = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        hunger: 5.0,
        personality: Personality {
            traits: PersonalityTraits {
                conscientiousness: 0.8,
                ..Default::default()
            },
        },
        ..Default::default()
    });
    let bob = world.spawn_agent(AgentConfig {
        pos: Vec2::new(500.0, 500.0),
        ..Default::default()
    });

    inject_verbal_commitment(&mut world, alice, Concept::Campfire, bob, 1);
    world.tick(60);

    // Spike alice's hunger so she picks up a hunger-satisfaction goal.
    {
        let mut needs = world.get_mut::<worldsim::agent::body::needs::PhysicalNeeds>(alice);
        needs.hunger = 95.0;
    }
    world.tick(240);

    // Now cool hunger off and keep ticking.
    {
        let mut needs = world.get_mut::<worldsim::agent::body::needs::PhysicalNeeds>(alice);
        needs.hunger = 10.0;
    }
    world.tick(240);

    let memory = world.get::<PlanMemory>(alice);
    let has_verbal = memory.plans.iter().any(|p| {
        matches!(p.source, PlanSource::VerbalCommitment { .. })
            && p.goal.target_concept() == Some(Concept::Campfire)
    });
    assert!(
        has_verbal,
        "verbal commitment must survive a hunger urgency spike and recovery"
    );
}

/// An agent can hold multiple background plans at once up to the
/// cognitive-load cap.
#[test]
fn agent_holds_multiple_background_plans_simultaneously() {
    let mut world = TestWorld::with_seed(42);
    let alice = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        personality: Personality {
            traits: PersonalityTraits {
                openness: 1.0,
                conscientiousness: 1.0,
                ..Default::default()
            },
        },
        ..Default::default()
    });
    let bob = world.spawn_agent(AgentConfig {
        pos: Vec2::new(500.0, 500.0),
        ..Default::default()
    });

    // Seed three verbal commitments to different concepts.
    inject_verbal_commitment(&mut world, alice, Concept::Apple, bob, 1);
    inject_verbal_commitment(&mut world, alice, Concept::Berry, bob, 2);
    inject_verbal_commitment(&mut world, alice, Concept::Campfire, bob, 3);

    // One tick is enough — nothing in the pipeline drops Background
    // plans on insertion order alone.
    world.tick(1);

    let memory = world.get::<PlanMemory>(alice);
    let background = memory.count_state(PlanState::Background);
    assert!(
        background >= 3,
        "alice should hold all three verbal commitment plans in Background, got {background}"
    );
}

/// Cognitive-load eviction drops the weakest Background plan when the
/// cap is exceeded. Verbal commitments are protected relative to
/// self-generated Background plans.
#[test]
fn eviction_protects_verbal_commitments() {
    let mut mem = PlanMemory::default();

    // One strong self-generated Background plan
    let strong = mem.mint_plan_id();
    mem.insert(HeldPlan {
        id: strong,
        goal: hunger_goal(0.5, Concept::Apple),
        steps: Vec::new(),
        state: PlanState::Background,
        commitment: 5.0,
        subjective_cost: 0.0,
        source: PlanSource::Brain(BrainType::Rational),
        created_at: 0,
        last_touched: 0,
        current_step: 0,
    });
    // One weak verbal commitment
    let weak = mem.mint_plan_id();
    mem.insert(HeldPlan {
        id: weak,
        goal: hunger_goal(0.5, Concept::Campfire),
        steps: Vec::new(),
        state: PlanState::Background,
        commitment: 0.1,
        subjective_cost: 0.0,
        source: PlanSource::VerbalCommitment {
            promised_to: Entity::from_bits(42),
            agreement_tick: 0,
        },
        created_at: 0,
        last_touched: 0,
        current_step: 0,
    });

    let evicted = mem.evict_excess(1);
    assert_eq!(evicted, vec![strong]);
    assert!(
        mem.get(weak).is_some(),
        "the verbal commitment must survive eviction when a self-generated plan is available"
    );
}
