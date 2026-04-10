//! Integration tests for goal commitment (#63).
//!
//! Verifies:
//! - Commitments override urgency-driven goals in the CNS when their priority is higher
//! - Conscientiousness modulates the committed goal's priority
//! - Verbal commitments flow through the conversation broadcast pipeline:
//!   speaker writes to their own Commitments AND listeners receive the
//!   `(speaker, Committed, X)` triple as Hearsay

use bevy::math::Vec2;
use worldsim::agent::brains::rational::RationalBrain;
use worldsim::agent::commitment::Commitments;
use worldsim::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use worldsim::agent::nervous_system::cns::CentralNervousSystem;
use worldsim::agent::nervous_system::config::NervousSystemConfig;
use worldsim::agent::psyche::personality::Personality;
use worldsim::testing::{AgentConfig, TestWorld};

const HIGH_SOCIAL: f32 = 0.8;

// ─── CNS goal formulation ────────────────────────────────────────────────────

/// A reliable (high conscientiousness) agent with an active commitment should
/// pick the committed goal when there's no urgent need competing.
#[test]
fn commitment_drives_cns_goal_when_no_strong_urgency() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));

    {
        let mut personality = world.get_mut::<Personality>(agent);
        personality.traits.conscientiousness = 1.0;
    }
    {
        let mut commitments = world.get_mut::<Commitments>(agent);
        commitments.add(Concept::Campfire, 0);
    }

    // Tick once so formulate_goals runs.
    world.tick(1);

    let cns = world.get::<CentralNervousSystem>(agent);
    let goal = cns
        .current_goal
        .as_ref()
        .expect("CNS should have a goal after commitment is made");

    // The committed goal pattern is (Self_, Contains, Item(Campfire, 1)).
    let has_campfire_target = goal
        .conditions
        .iter()
        .any(|pattern| matches!(pattern.object, Some(Value::Item(Concept::Campfire, _))));
    assert!(
        has_campfire_target,
        "committed goal should target Campfire; got {:?}",
        goal.conditions
    );
}

/// A reliable agent's commitment priority should exceed a flaky agent's
/// commitment priority for the same goal.
#[test]
fn high_conscientiousness_produces_stronger_committed_goal() {
    use worldsim::agent::commitment::Commitment;

    let reliable = Commitment::new(Concept::Campfire, 0);
    let flaky = Commitment::new(Concept::Campfire, 0);

    assert!(
        reliable.priority(1.0) > flaky.priority(0.0),
        "reliable ({}) should outweigh flaky ({})",
        reliable.priority(1.0),
        flaky.priority(0.0)
    );
}

// ─── Decay integration ───────────────────────────────────────────────────────

/// After many ticks without refresh, a low-conscientiousness agent's
/// commitment decays below the retention threshold and is forgotten.
#[test]
fn flaky_agent_forgets_commitment_over_time() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));

    {
        let mut personality = world.get_mut::<Personality>(agent);
        personality.traits.conscientiousness = 0.0;
    }
    {
        let mut commitments = world.get_mut::<Commitments>(agent);
        commitments.add(Concept::Campfire, 0);
    }

    // Decay rate at 0.0 conscientiousness is 0.002/tick; min threshold is 0.05.
    // Need ~(1.0 - 0.05) / 0.002 = 475 ticks to forget.
    world.tick(600);

    let commitments = world.get::<Commitments>(agent);
    assert!(
        commitments.active.is_empty(),
        "flaky agent should have forgotten the commitment after 600 ticks"
    );
}

// ─── Conversation trigger ────────────────────────────────────────────────────

/// When a speaker with an active commitment shares during conversation, the
/// listener should receive the `(speaker, Committed, Concept)` triple as
/// Hearsay in their MindGraph.
#[test]
fn listener_learns_speakers_commitment_via_conversation() {
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    // Force fast thinking so the conversation spins up within the test budget.
    {
        let mut config = world
            .app_mut()
            .world_mut()
            .resource_mut::<NervousSystemConfig>();
        config.thinking_interval = 1;
    }

    let alice = agents["alice"];
    let bob = agents["bob"];

    // Seed Alice's commitments directly. The CNS goal-formulation path
    // converts this into a brain-level goal each tick, and when Alice
    // picks `Intent::Share` she'll broadcast a `(Self, Committed, Campfire)`
    // triple to Bob.
    {
        let mut personality = world.get_mut::<Personality>(alice);
        personality.traits.conscientiousness = 1.0;
    }
    {
        let mut commitments = world.get_mut::<Commitments>(alice);
        commitments.add(Concept::Campfire, 0);
    }

    // Tick until the conversation has had several turns — plenty of time
    // for Alice to pick Intent::Share at least once.
    world.tick(300);

    // Bob's MindGraph should contain a (alice, Committed, Campfire) triple.
    let bob_mind = world.get::<MindGraph>(bob);
    let committed_triples = bob_mind.query(
        Some(&Node::Entity(alice)),
        Some(Predicate::Committed),
        Some(&Value::Concept(Concept::Campfire)),
    );
    assert!(
        !committed_triples.is_empty(),
        "Bob should know Alice committed to Campfire after hearing her share it"
    );
}

/// Sanity-check the commitment-through-CNS path: when an agent has a
/// commitment, the rational brain should end up with a build-style goal
/// after a single tick (proving the commitment → CNS → brain flow works).
#[test]
fn commitment_propagates_through_cns_to_rational_brain() {
    let mut world = TestWorld::with_seed(0);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(100.0, 100.0)));

    // Force fast thinking.
    {
        let mut config = world
            .app_mut()
            .world_mut()
            .resource_mut::<NervousSystemConfig>();
        config.thinking_interval = 1;
    }
    {
        let mut personality = world.get_mut::<Personality>(agent);
        personality.traits.conscientiousness = 1.0;
    }
    {
        let mut commitments = world.get_mut::<Commitments>(agent);
        commitments.add(Concept::Campfire, 0);
    }

    world.tick(2);

    let brain = world.get::<RationalBrain>(agent);
    let goal = brain
        .current_goal
        .as_ref()
        .expect("rational brain should have a goal derived from the commitment");
    let has_campfire = goal
        .conditions
        .iter()
        .any(|p| matches!(p.object, Some(Value::Item(Concept::Campfire, _))));
    assert!(
        has_campfire,
        "rational brain goal should target Campfire; got {:?}",
        goal.conditions
    );
}
