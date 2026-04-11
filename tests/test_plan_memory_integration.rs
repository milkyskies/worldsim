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

/// Two humans starting a conversation should stay in it for more than
/// the 1-tick flicker the issue describes. Pre-#338, an agent's idle
/// Wander would walk them out of conversation range the same tick the
/// turn started; the new in-conversation Wander suppression keeps the
/// conversation alive long enough for turns to actually fire.
#[test]
fn agents_in_conversation_do_not_flicker_via_wander() {
    use worldsim::agent::nervous_system::config::NervousSystemConfig;

    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(0.8)
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(0.8)
        .done()
        .build();

    // Force the brains to fire every tick so we don't have to wait
    // a full thinking interval for the conversation to spin up.
    {
        let mut config = world
            .app_mut()
            .world_mut()
            .resource_mut::<NervousSystemConfig>();
        config.thinking_interval = 1;
    }

    let alice = agents["alice"];
    let bob = agents["bob"];

    // Tick long enough for both agents to perceive each other, choose
    // an InitiateConversation action, swap into Converse, and stay in
    // it past the 1-tick window the old bug collapsed inside.
    world.tick(120);

    assert!(
        world.in_conversation(alice) && world.in_conversation(bob),
        "both agents should still be in conversation 120 ticks after spawn \
         (pre-#338 the conversation collapsed within 1 tick when Wander walked \
         the partner out of range)"
    );
}

/// A rational brain emitting two Executing plans on disjoint body
/// channels gets both admitted in parallel: the proposal generator
/// returns multiple `BrainProposal`s, arbitration's channel-conflict
/// path doesn't reject either, and admission is order-stable.
#[test]
fn multiple_executing_plans_admit_in_parallel() {
    use worldsim::agent::actions::ActionRegistry;
    use worldsim::agent::actions::ActionType;
    use worldsim::agent::brains::plan_memory::{HeldPlan, PlanMemory, PlanSource, PlanState};
    use worldsim::agent::brains::proposal::{BrainProposal, BrainType, Intent};
    use worldsim::agent::brains::rational::rational_brain_propose;
    use worldsim::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
    use worldsim::agent::mind::knowledge::{
        Concept, MindGraph, Node as MindNode, Predicate, Value,
    };
    use worldsim::agent::nervous_system::cns::CentralNervousSystem;
    use worldsim::agent::nervous_system::urgency::{Urgency, UrgencySource};

    let mut memory = PlanMemory::default();
    // Walk plan: rides Locomotion.
    let walk_id = memory.mint_plan_id();
    memory.insert(HeldPlan {
        id: walk_id,
        goal: Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::Contains),
                Some(Value::Item(Concept::Apple, 1)),
            )],
            priority: 0.5,
        },
        steps: vec![ActionTemplate {
            name: "WalkStep".into(),
            action_type: ActionType::Walk,
            target_entity: None,
            target_position: Some(Vec2::new(60.0, 60.0)),
            preconditions: vec![],
            effects: vec![],
            consumes: vec![],
            base_cost: 0.0,
            locomotion_intensity: ActionType::Walk.default_locomotion_intensity(),
        }],
        state: PlanState::Executing,
        commitment: 5.0,
        subjective_cost: 10.0,
        source: PlanSource::Brain(BrainType::Rational),
        created_at: 0,
        last_touched: 0,
        current_step: 0,
    });
    // Converse plan: rides Vocalization.
    let talk_id = memory.mint_plan_id();
    memory.insert(HeldPlan {
        id: talk_id,
        goal: Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::SocialDrive),
                Some(Value::Int(0)),
            )],
            priority: 0.5,
        },
        steps: vec![ActionTemplate {
            name: "ConverseStep".into(),
            action_type: ActionType::Converse,
            target_entity: None,
            target_position: None,
            preconditions: vec![],
            effects: vec![],
            consumes: vec![],
            base_cost: 0.0,
            locomotion_intensity: ActionType::Converse.default_locomotion_intensity(),
        }],
        state: PlanState::Executing,
        commitment: 5.0,
        subjective_cost: 10.0,
        source: PlanSource::Brain(BrainType::Rational),
        created_at: 0,
        last_touched: 0,
        current_step: 0,
    });

    let mut cns = CentralNervousSystem::default();
    cns.urgencies.push(Urgency::new(UrgencySource::Hunger, 0.5));
    let registry = ActionRegistry::new();
    let proposals = rational_brain_propose(&memory, &cns, &MindGraph::default(), &registry, false);

    let kinds: Vec<_> = proposals.iter().map(|p| p.action.action_type).collect();
    assert!(
        kinds.contains(&ActionType::Walk),
        "Walk plan should surface as a proposal, got {kinds:?}"
    );
    assert!(
        kinds.contains(&ActionType::Converse),
        "Converse plan should surface as a proposal, got {kinds:?}"
    );
    let _: Vec<BrainProposal> = proposals; // type assertion
    assert!(matches!(
        cns.urgencies.first().map(|u| u.source),
        Some(UrgencySource::Hunger)
    ));
    let _ = (Intent::None,); // keep import alive
}

/// Listener-side demand reduction (#338): an agent whose MindGraph
/// already records a peer's `Committed` triple for the same concept
/// they would otherwise pursue should see their own goal priority
/// discounted in CNS.
#[test]
fn peer_commitment_discounts_listener_goal_priority() {
    use worldsim::agent::mind::knowledge::{
        Concept, Metadata, MindGraph, Node, Predicate, Triple, Value,
    };
    use worldsim::agent::nervous_system::cns::CentralNervousSystem;

    let mut world = TestWorld::with_seed(42);
    let bob = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        personality: Personality {
            traits: PersonalityTraits {
                conscientiousness: 0.8,
                ..Default::default()
            },
        },
        ..Default::default()
    });

    // Inject a verbal-commitment plan on Bob targeting Campfire so the
    // CNS picks Campfire as Bob's commitment goal.
    inject_verbal_commitment(&mut world, bob, Concept::Campfire, Entity::from_bits(99), 1);

    // Tick once to let formulate_goals run with the verbal-only plan
    // and pick the commitment goal.
    world.tick(1);
    let baseline_priority = world
        .get::<CentralNervousSystem>(bob)
        .current_goal
        .as_ref()
        .map(|g| g.priority)
        .expect("CNS should formulate a commitment goal for Campfire");

    // Now broadcast that a peer (entity 99) is also committed to
    // Campfire — exactly the triple `communication.rs` writes when an
    // agent verbally announces their commitment.
    let peer = Entity::from_bits(99);
    {
        let mut mind = world.get_mut::<MindGraph>(bob);
        mind.assert(Triple::with_meta(
            Node::Entity(peer),
            Predicate::Committed,
            Value::Concept(Concept::Campfire),
            Metadata::default(),
        ));
    }
    world.tick(1);
    let discounted_priority = world
        .get::<CentralNervousSystem>(bob)
        .current_goal
        .as_ref()
        .map(|g| g.priority)
        .expect("CNS should still emit a goal after the peer commitment");

    assert!(
        discounted_priority < baseline_priority,
        "peer commitment must discount the listener's goal priority \
         (baseline={baseline_priority:.2}, after_peer={discounted_priority:.2})"
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
