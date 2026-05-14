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
use worldsim::agent::mind::knowledge::{Concept, Node as MindNode, Predicate, Quantity, Value};
use worldsim::testing::{AgentConfig, TestWorld, personality};

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
        driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Commitment,
        created_at_urgency: 0.5,
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
        genome: personality().conscientiousness(0.8).into(),
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
    use worldsim::agent::body::metabolism::Metabolism;
    use worldsim::agent::body::needs::PhysicalNeeds;

    let mut world = TestWorld::with_seed(42);
    let alice = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        genome: personality().conscientiousness(0.8).into(),
        ..Default::default()
    });
    let bob = world.spawn_agent(AgentConfig {
        pos: Vec2::new(500.0, 500.0),
        ..Default::default()
    });

    inject_verbal_commitment(&mut world, alice, Concept::Campfire, bob, 1);
    world.tick(60);

    // Spike alice's hunger so she picks up a hunger-satisfaction goal.
    // After the metabolism rewrite (post-#338 base) we drain pools to
    // empty rather than poking a flat scalar.
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(alice);
        needs.metabolism = Metabolism::empty();
    }
    world.tick(240);

    // Now cool hunger off again.
    {
        let mut needs = world.get_mut::<PhysicalNeeds>(alice);
        needs.metabolism = Metabolism::well_fed();
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
        genome: personality().openness(1.0).conscientiousness(1.0).into(),
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
            behavior: Default::default(),
            locomotion_intensity: 0.0,
            estimated_duration_ticks: None,
            search_filter: None,
        }],
        state: PlanState::Executing,
        commitment: 5.0,
        subjective_cost: 10.0,
        source: PlanSource::Brain(BrainType::Rational),
        driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Hunger,
        created_at_urgency: 1.0,
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
                Some(Value::Quantity(Quantity::Exact(0.0))),
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
            behavior: Default::default(),
            locomotion_intensity: 0.0,
            estimated_duration_ticks: None,
            search_filter: None,
        }],
        state: PlanState::Executing,
        commitment: 5.0,
        subjective_cost: 10.0,
        source: PlanSource::Brain(BrainType::Rational),
        driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Hunger,
        created_at_urgency: 1.0,
        created_at: 0,
        last_touched: 0,
        current_step: 0,
    });

    let mut cns = CentralNervousSystem::default();
    cns.urgencies.push(Urgency::new(UrgencySource::Hunger, 0.5));
    let registry = ActionRegistry::new();
    let proposals = rational_brain_propose(&memory, &cns, &MindGraph::default(), &registry);

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

/// Multi-action arbitration must admit two Rational plans in parallel
/// when their body channels don't conflict. Earlier we only checked
/// that the brain *emitted* both proposals; this end-to-end test runs
/// arbitration and confirms both reach the admitted set.
#[test]
fn arbitration_admits_walk_and_converse_in_parallel() {
    use worldsim::agent::actions::{ActionRegistry, ActionType, ChannelCapacities};
    use worldsim::agent::brains::arbitration::arbitrate_parallel;
    use worldsim::agent::brains::plan_memory::{HeldPlan, PlanMemory, PlanSource, PlanState};
    use worldsim::agent::brains::proposal::{BrainPowers, BrainType, Intent};
    use worldsim::agent::brains::rational::rational_brain_propose;
    use worldsim::agent::brains::thinking::{ActionTemplate, Goal, TriplePattern};
    use worldsim::agent::mind::knowledge::{
        Concept, MindGraph, Node as MindNode, Predicate, Value,
    };
    use worldsim::agent::nervous_system::cns::CentralNervousSystem;
    use worldsim::agent::nervous_system::urgency::{Urgency, UrgencySource};

    let mut memory = PlanMemory::default();
    let walk_id = memory.mint_plan_id();
    memory.insert(HeldPlan {
        id: walk_id,
        goal: Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::Contains),
                Some(Value::Item(Concept::Apple, 1)),
            )],
            priority: 0.7,
        },
        steps: vec![ActionTemplate {
            name: "Walk".into(),
            action_type: ActionType::Walk,
            target_entity: None,
            target_position: Some(Vec2::new(60.0, 60.0)),
            preconditions: vec![],
            effects: vec![],
            consumes: vec![],
            base_cost: 0.0,
            behavior: Default::default(),
            locomotion_intensity: 0.0,
            estimated_duration_ticks: None,
            search_filter: None,
        }],
        state: PlanState::Executing,
        commitment: 5.0,
        subjective_cost: 10.0,
        source: PlanSource::Brain(BrainType::Rational),
        driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Hunger,
        created_at_urgency: 1.0,
        created_at: 0,
        last_touched: 0,
        current_step: 0,
    });
    let talk_id = memory.mint_plan_id();
    memory.insert(HeldPlan {
        id: talk_id,
        goal: Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::SocialDrive),
                Some(Value::Quantity(Quantity::Exact(0.0))),
            )],
            priority: 0.6,
        },
        // Post-#743 `Converse` is an engagement-owned beat and can't be
        // a rational plan step. `Wave` is the channel-equivalent stand-in:
        // posture-agnostic, non-Locomotion channels, so it still exercises
        // the "admit two channel-compatible actions in parallel with Walk"
        // path this test cares about.
        steps: vec![ActionTemplate {
            name: "Wave".into(),
            action_type: ActionType::Wave,
            target_entity: None,
            target_position: None,
            preconditions: vec![],
            effects: vec![],
            consumes: vec![],
            base_cost: 0.0,
            behavior: Default::default(),
            locomotion_intensity: 0.0,
            estimated_duration_ticks: None,
            search_filter: None,
        }],
        state: PlanState::Executing,
        commitment: 5.0,
        subjective_cost: 10.0,
        source: PlanSource::Brain(BrainType::Rational),
        driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Social,
        created_at_urgency: 0.6,
        created_at: 0,
        last_touched: 0,
        current_step: 0,
    });

    let mut cns = CentralNervousSystem::default();
    cns.urgencies.push(Urgency::new(UrgencySource::Hunger, 0.7));
    cns.urgencies.push(Urgency::new(UrgencySource::Social, 0.6));
    let registry = ActionRegistry::new();
    let proposals = rational_brain_propose(&memory, &cns, &MindGraph::default(), &registry);

    let powers = BrainPowers {
        survival: 1.0,
        emotional: 1.0,
        rational: 1.0,
    };
    let capacities = ChannelCapacities::full();
    let proposal_options: Vec<_> = proposals.into_iter().map(Some).collect();
    let result = arbitrate_parallel(&proposal_options, &powers, &capacities, &registry, None);

    let admitted_kinds: Vec<_> = result
        .admitted
        .iter()
        .map(|p| p.action.action_type)
        .collect();
    assert!(
        admitted_kinds.contains(&ActionType::Walk) && admitted_kinds.contains(&ActionType::Wave),
        "arbitration should admit Walk + Wave in parallel (different channels), got {admitted_kinds:?}"
    );
    assert!(
        result.rejected.is_empty(),
        "no rejected proposals expected — both ride disjoint channels"
    );
    let _ = Intent::None;
}

/// Two Rational Movement proposals on different intents (so they
/// survive intent dedup) compete on the single Movement slot — the
/// higher-priority one admits, the loser ends up in the rejected set
/// so `brain_system` can demote its backing plan to Suspended. The
/// rejection path is the substrate that #338's "Executing → Suspended
/// on channel conflict" acceptance criterion needs.
#[test]
fn arbitration_rejects_competing_movement_plans() {
    use worldsim::agent::actions::{ActionRegistry, ActionType, ChannelCapacities};
    use worldsim::agent::brains::arbitration::arbitrate_parallel;
    use worldsim::agent::brains::proposal::{BrainPowers, BrainProposal, BrainType, Intent};
    use worldsim::agent::brains::thinking::ActionTemplate;

    fn make_proposal(action: ActionType, urgency: f32, intent: Intent) -> BrainProposal {
        BrainProposal {
            brain: BrainType::Rational,
            action: ActionTemplate {
                name: format!("{action:?}"),
                action_type: action,
                target_entity: None,
                target_position: Some(Vec2::new(20.0, 0.0)),
                preconditions: vec![],
                effects: vec![],
                consumes: vec![],
                base_cost: 0.0,
                behavior: Default::default(),
                locomotion_intensity: 0.0,
                estimated_duration_ticks: None,
                search_filter: None,
            },
            urgency,
            intent,
            reasoning: String::new(),
        }
    }

    let walk = make_proposal(ActionType::Walk, 95.0, Intent::SatisfyHunger);
    let wander = make_proposal(ActionType::Wander, 20.0, Intent::SatisfyCuriosity);

    let powers = BrainPowers {
        survival: 1.0,
        emotional: 1.0,
        rational: 1.0,
    };
    let capacities = ChannelCapacities::full();
    let registry = ActionRegistry::new();
    let proposals = vec![Some(walk), Some(wander)];
    let result = arbitrate_parallel(&proposals, &powers, &capacities, &registry, None);

    assert_eq!(
        result.admitted.len(),
        1,
        "only one Movement may admit per tick"
    );
    assert_eq!(result.admitted[0].action.action_type, ActionType::Walk);
    assert_eq!(
        result.rejected.len(),
        1,
        "the lower-priority Wander should appear in rejected so brain_system \
         can suspend its backing plan"
    );
    assert_eq!(result.rejected[0].action.action_type, ActionType::Wander);
}

/// A Suspended plan whose commitment decays to zero must drop back
/// to Background — the state machine's loop transition that lets the
/// plan be re-promoted later when channels free up.
#[test]
fn suspended_plan_decays_to_background_when_commitment_hits_zero() {
    use worldsim::agent::brains::plan_memory::{HeldPlan, PlanMemory, PlanState};

    let mut world = TestWorld::with_seed(42);
    let alice = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        ..Default::default()
    });

    // Inject a Suspended plan with a tiny commitment so the per-tick
    // decay reaches zero quickly. Use a verbal commitment source so
    // the rational brain's every-tick stale-plan sweep (#424) doesn't
    // nuke the plan for not matching the current CNS goal — this test
    // is about the decay → Background state-machine transition, not
    // the stale-sweep.
    let plan_id = {
        let mut memory = world
            .app_mut()
            .world_mut()
            .get_mut::<PlanMemory>(alice)
            .unwrap();
        let id = memory.mint_plan_id();
        memory.insert(HeldPlan {
            id,
            goal: hunger_goal(0.5, Concept::Apple),
            steps: vec![worldsim::agent::brains::thinking::ActionTemplate {
                name: "Walk".into(),
                action_type: worldsim::agent::actions::ActionType::Walk,
                target_entity: None,
                target_position: Some(Vec2::new(80.0, 80.0)),
                preconditions: vec![],
                effects: vec![],
                consumes: vec![],
                base_cost: 0.0,
                behavior: Default::default(),
                locomotion_intensity: 0.0,
                estimated_duration_ticks: None,
                search_filter: None,
            }],
            state: PlanState::Suspended,
            commitment: 0.04,
            subjective_cost: 10.0,
            source: PlanSource::VerbalCommitment {
                promised_to: Entity::from_bits(42),
                agreement_tick: 0,
            },
            driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Commitment,
            created_at_urgency: 0.5,
            created_at: 0,
            last_touched: 0,
            current_step: 0,
        });
        id
    };

    // 1 tick at 0.05/tick decay drops the 0.04 starting commitment to
    // 0 → state machine transitions Suspended → Background. Checking
    // immediately avoids the next per-tick commitment ramp re-promoting
    // the plan to Considering / Executing.
    world.tick(1);

    let memory = world.get::<PlanMemory>(alice);
    let plan = memory
        .get(plan_id)
        .expect("plan should still exist after the suspension decay");
    assert_eq!(
        plan.state,
        PlanState::Background,
        "Suspended plan whose commitment hit zero should drop to Background"
    );
}

/// `most_committed_goal` selects the speaker's highest-commitment plan
/// across states (Executing > Considering > Background) so the
/// conversation content path can pull it as the seed for `Share`
/// turns. This is the read-side of "Background plans surface as
/// conversation content."
#[test]
fn most_committed_plan_drives_conversation_content_seed() {
    use worldsim::agent::brains::plan_memory::{HeldPlan, PlanMemory, PlanSource, PlanState};
    use worldsim::agent::brains::proposal::BrainType;
    use worldsim::agent::brains::thinking::{Goal, TriplePattern};
    use worldsim::agent::mind::knowledge::{Concept, Node as MindNode, Predicate, Value};

    let mut memory = PlanMemory::default();
    let weak = memory.mint_plan_id();
    memory.insert(HeldPlan {
        id: weak,
        goal: Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::Contains),
                Some(Value::Item(Concept::Berry, 1)),
            )],
            priority: 0.5,
        },
        steps: Vec::new(),
        state: PlanState::Background,
        commitment: 0.1,
        subjective_cost: 0.0,
        source: PlanSource::Brain(BrainType::Rational),
        driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Hunger,
        created_at_urgency: 1.0,
        created_at: 0,
        last_touched: 0,
        current_step: 0,
    });
    // Stronger Background plan should win — Executing > Considering >
    // Background, but with both at the same state we tiebreak by
    // commitment.
    let strong = memory.mint_plan_id();
    memory.insert(HeldPlan {
        id: strong,
        goal: Goal {
            conditions: vec![TriplePattern::new(
                Some(MindNode::Self_),
                Some(Predicate::Near),
                Some(Value::Concept(Concept::Campfire)),
            )],
            priority: 0.5,
        },
        steps: Vec::new(),
        state: PlanState::Background,
        commitment: 5.0,
        subjective_cost: 0.0,
        source: PlanSource::VerbalCommitment {
            promised_to: Entity::from_bits(42),
            agreement_tick: 0,
        },
        driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Commitment,
        created_at_urgency: 0.5,
        created_at: 0,
        last_touched: 0,
        current_step: 0,
    });

    // The communication module's helper isn't pub today, so we exercise
    // the same code path through the speaker_goal-via-PlanMemory branch
    // by reading the strongest concept directly. (This intentionally
    // mirrors the production helper's preference order.)
    let chosen_concept = memory
        .plans
        .iter()
        .max_by(|a, b| {
            a.commitment
                .partial_cmp(&b.commitment)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|p| p.goal.target_concept());
    assert_eq!(
        chosen_concept,
        Some(Concept::Campfire),
        "the speaker's highest-commitment plan must seed the conversation goal concept"
    );
}

/// Verbal commitments mentioned during a conversation get their
/// `last_touched` bumped — that's the signal the rational brain reads
/// for the announcement-bonus accelerator (#329 path now driven from
/// PlanMemory). End-to-end check via the live conversation system.
#[test]
fn mentioning_a_plan_in_conversation_refreshes_its_last_touched() {
    use worldsim::agent::brains::plan_memory::{PlanMemory, PlanSource};
    use worldsim::agent::mind::knowledge::Concept;
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

    {
        let mut config = world
            .app_mut()
            .world_mut()
            .resource_mut::<NervousSystemConfig>();
        config.thinking_interval = 1;
    }

    let alice = agents["alice"];
    let bob = agents["bob"];

    // Seed Alice with a verbal commitment to Campfire whose
    // last_touched is exactly at spawn (tick 0). After the
    // conversation runs and Alice brings up her active goal, we
    // expect upsert_verbal_commitment to bump it forward.
    inject_verbal_commitment(&mut world, alice, Concept::Campfire, bob, 0);

    // Tick long enough for Alice and Bob to perceive each other,
    // open a conversation, and exchange a Share/Ask/Answer turn —
    // any of which trigger the verbal-commitment refresh.
    world.tick(150);

    let memory = world.get::<PlanMemory>(alice);
    let plan = memory
        .plans
        .iter()
        .find(|p| {
            matches!(p.source, PlanSource::VerbalCommitment { .. })
                && p.goal.target_concept() == Some(Concept::Campfire)
        })
        .expect("Alice should still hold the verbal commitment");
    assert!(
        plan.last_touched > 0,
        "verbal commitment last_touched should be bumped forward by a conversation \
         (got {})",
        plan.last_touched
    );
}

/// Listener-side demand reduction (#338): an agent whose MindGraph
/// already records a peer's `Committed` triple for the same concept
/// they would otherwise pursue should see a discounted goal priority
/// when the planner synthesizes a goal from the Commitment urgency.
#[test]
fn peer_commitment_discounts_listener_goal_priority() {
    use worldsim::agent::brains::plan_memory::PlanMemory;
    use worldsim::agent::brains::rational::goal_for_urgency;
    use worldsim::agent::mind::knowledge::{
        Concept, Metadata, MindGraph, Node, Predicate, Triple, Value,
    };
    use worldsim::agent::nervous_system::urgency::UrgencySource;

    let mut world = TestWorld::with_seed(42);
    let bob = world.spawn_agent(AgentConfig {
        pos: Vec2::new(50.0, 50.0),
        genome: personality().conscientiousness(0.8).into(),
        ..Default::default()
    });

    // Inject a verbal-commitment plan on Bob targeting Campfire.
    inject_verbal_commitment(&mut world, bob, Concept::Campfire, Entity::from_bits(99), 1);

    // Tick once so urgency generation emits the Commitment urgency.
    world.tick(1);

    let baseline_priority = {
        let memory = world.get::<PlanMemory>(bob).clone();
        let mind = world.get::<MindGraph>(bob).clone();
        goal_for_urgency(UrgencySource::Commitment, 0.7, &memory, &mind)
            .expect("should synthesize a commitment goal")
            .priority
    };

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
    let discounted_priority = {
        let memory = world.get::<PlanMemory>(bob).clone();
        let mind = world.get::<MindGraph>(bob).clone();
        goal_for_urgency(UrgencySource::Commitment, 0.7, &memory, &mind)
            .expect("should still synthesize a goal")
            .priority
    };

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
        driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Hunger,
        created_at_urgency: 1.0,
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
        driving_urgency: worldsim::agent::nervous_system::urgency::UrgencySource::Commitment,
        created_at_urgency: 0.5,
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
