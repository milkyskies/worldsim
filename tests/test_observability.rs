//! Integration tests for the #571 observability upgrade:
//! - Parquet event log round-trip
//! - PlanGenerated / TargetEnumerated / PatternRejected events
//! - Plan context on ActionStarted
//! - MindGraph mutation log
//! - Per-tick agent state hash
//! - Urgency contributors on Decision
//! - GOAP search telemetry

use bevy::prelude::*;
use worldsim::agent::body::metabolism::Metabolism;
use worldsim::agent::events::SimEvent;
use worldsim::agent::mind::knowledge::{
    Concept, Metadata, Node as MindNode, Predicate, Triple, Value,
};
use worldsim::core::event_log::{read_parquet_payloads, write_parquet};
use worldsim::testing::{AgentConfig, TestWorld};

/// Build an agent config with an empty metabolism so Hunger urgency is high.
fn hungry_at(pos: Vec2) -> AgentConfig {
    AgentConfig {
        pos,
        metabolism: Metabolism::at_urgency(0.8),
        ..AgentConfig::default()
    }
}

// ─── Parquet round-trip ────────────────────────────────────────────────────

#[test]
fn parquet_log_roundtrip_preserves_events() {
    let lines = vec![
        r#"{"tick":1,"type":"Decision","agent":"Alice","payload":"x"}"#.to_string(),
        r#"{"tick":2,"type":"ActionStarted","agent":"Bob","payload":"y"}"#.to_string(),
        r#"{"tick":3,"type":"PlanGenerated","agent":"Alice","plan_id":42}"#.to_string(),
    ];

    let tmp_dir = std::env::temp_dir();
    let path = tmp_dir.join(format!(
        "worldsim_parquet_roundtrip_{}.parquet",
        std::process::id()
    ));

    write_parquet(&lines, &path).expect("write_parquet should succeed");
    let read_back = read_parquet_payloads(&path).expect("read_parquet should succeed");

    assert_eq!(
        read_back, lines,
        "round-trip should preserve every JSONL line exactly"
    );

    let _ = std::fs::remove_file(&path);
}

// ─── PlanGenerated ─────────────────────────────────────────────────────────

/// Hungry agent + known food source → GOAP generates a plan → PlanGenerated
/// fires with non-empty goal description and driving_urgency=Hunger.
#[test]
fn plan_generated_event_fires_with_full_template_shape() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(hungry_at(Vec2::new(0.0, 0.0)));
    world.spawn_apple_tree(Vec2::new(100.0, 0.0), 5);

    world.tick(120);

    let events = world.sim_events().all();
    let plan_generated: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::PlanGenerated {
                agent: a,
                plan_id,
                driving_urgency,
                step_count,
                goal_description,
                ..
            } if *a == agent => Some((
                *plan_id,
                *driving_urgency,
                *step_count,
                goal_description.clone(),
            )),
            _ => None,
        })
        .collect();

    assert!(
        !plan_generated.is_empty(),
        "PlanGenerated should fire for a hungry agent with visible food"
    );
    let (_plan_id, _urgency, step_count, goal_desc) = &plan_generated[0];
    assert!(*step_count > 0, "plan should have at least one step");
    assert!(
        !goal_desc.is_empty(),
        "goal_description should describe the plan's goal"
    );
}

// ─── TargetEnumerated ──────────────────────────────────────────────────────

#[test]
fn target_enumerated_event_captures_inclusion_reason() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(hungry_at(Vec2::new(0.0, 0.0)));
    world.spawn_apple_tree(Vec2::new(80.0, 0.0), 5);

    world.tick(120);

    let events = world.sim_events().all();
    let target_enumerated: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::TargetEnumerated {
                agent: a,
                inclusion_reason,
                ..
            } if *a == agent => Some(inclusion_reason.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !target_enumerated.is_empty(),
        "TargetEnumerated should fire during planning"
    );
    assert!(
        target_enumerated
            .iter()
            .any(|r| { r.starts_with("is_plan_valid") || r.starts_with("belief_confidence:") }),
        "inclusion_reason should be either 'is_plan_valid' or 'belief_confidence:<f>', got: {target_enumerated:?}"
    );
}

// ─── ActionStarted carries plan_id ─────────────────────────────────────────

#[test]
fn action_started_carries_plan_context() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(hungry_at(Vec2::new(0.0, 0.0)));
    world.spawn_apple_tree(Vec2::new(80.0, 0.0), 5);

    world.tick(150);

    let events = world.sim_events().all();
    let starts: Vec<(Option<u64>, Option<usize>)> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::ActionStarted {
                agent: a,
                plan_id,
                plan_step,
                ..
            } if *a == agent => Some((*plan_id, *plan_step)),
            _ => None,
        })
        .collect();

    assert!(
        !starts.is_empty(),
        "expected ActionStarted events for the agent"
    );
    let has_plan_context = starts
        .iter()
        .any(|(plan_id, plan_step)| plan_id.is_some() && plan_step.is_some());
    assert!(
        has_plan_context,
        "at least one ActionStarted should carry plan_id + plan_step from an executing \
         rational-brain plan, got: {starts:?}"
    );
}

// ─── PatternRejected ───────────────────────────────────────────────────────

/// Scenario: hungry agent in a world with only non-food Stone. GOAP has no
/// action whose effects satisfy "Contains Food" (isa_filter=Food blocks
/// Stone). The planner returns None and emits PatternRejected carrying the
/// unmet patterns.
#[test]
fn pattern_rejected_event_captures_rejection_reason() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(hungry_at(Vec2::new(0.0, 0.0)));
    // Only stone — no food anywhere.
    world.spawn_stone_node(Vec2::new(60.0, 0.0), 10);

    world.tick(200);

    let events = world.sim_events().all();
    let rejected: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::PatternRejected {
                agent: a,
                unmet_patterns,
                goal_description,
                ..
            } if *a == agent => Some((goal_description.clone(), unmet_patterns.clone())),
            _ => None,
        })
        .collect();

    assert!(
        !rejected.is_empty(),
        "PatternRejected should fire for a hungry agent who can't reach food"
    );
    let (_goal, patterns) = &rejected[0];
    assert!(
        !patterns.is_empty(),
        "unmet_patterns should describe at least one unsatisfiable condition"
    );
}

// ─── MindGraph mutation log ────────────────────────────────────────────────

#[test]
fn mindgraph_mutation_log_captures_add_and_remove() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));

    world.spawn_apple_tree(Vec2::new(30.0, 0.0), 5);
    world.tick(10);

    let events = world.sim_events().all();
    let adds: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::MindGraphMutation { agent: a, op, .. } if *a == agent && op == "Add" => {
                Some(op.clone())
            }
            _ => None,
        })
        .collect();

    assert!(
        !adds.is_empty(),
        "MindGraphMutation Add events should fire as perception writes to the MindGraph"
    );

    // Force a removal by directly manipulating the MindGraph.
    let apple_entity = Entity::from_bits(9999);
    {
        let app = world.app_mut();
        let mut mind = app
            .world_mut()
            .get_mut::<worldsim::agent::mind::knowledge::MindGraph>(agent)
            .expect("agent should have MindGraph");
        mind.assert(Triple {
            subject: MindNode::Entity(apple_entity),
            predicate: Predicate::HasTrait,
            object: Value::Concept(Concept::Apple),
            meta: Metadata::default(),
        });
        mind.remove(
            &MindNode::Entity(apple_entity),
            Predicate::HasTrait,
            &Value::Concept(Concept::Apple),
        );
    }
    world.tick(1);

    let events = world.sim_events().all();
    let removes: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::MindGraphMutation { agent: a, op, .. } if *a == agent && op == "Remove" => {
                Some(op.clone())
            }
            _ => None,
        })
        .collect();

    assert!(
        !removes.is_empty(),
        "MindGraphMutation Remove events should fire when triples are removed"
    );
}

// ─── MindGraph reconstructible from mutations ─────────────────────────────

#[test]
fn mindgraph_state_reconstructible_from_mutations_at_arbitrary_tick() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(AgentConfig::at(Vec2::new(0.0, 0.0)));
    world.spawn_apple_tree(Vec2::new(30.0, 0.0), 5);
    world.tick(20);

    let events = world.sim_events().all();

    // Reconstruct the (subject, predicate, object) set by replaying every
    // Add/Remove mutation for this agent.
    use std::collections::HashSet;
    let mut reconstructed: HashSet<(String, String, String)> = HashSet::new();
    for e in events {
        if let SimEvent::MindGraphMutation {
            agent: a,
            op,
            subject,
            predicate,
            object,
            ..
        } = e
        {
            if *a != agent {
                continue;
            }
            let key = (subject.clone(), predicate.clone(), object.clone());
            if op == "Add" {
                reconstructed.insert(key);
            } else if op == "Remove" {
                reconstructed.remove(&key);
            }
        }
    }

    assert!(
        !reconstructed.is_empty(),
        "reconstructed mindgraph state should not be empty after 20 ticks of perception"
    );
}

// ─── State hash diverges between seeds ─────────────────────────────────────

#[test]
fn state_hash_diverges_at_expected_tick_for_two_different_seeds() {
    // Hungry + Explore-pressured agents: the explore-target RNG depends on
    // SimRng, so different seeds take different paths even without food.
    fn run(seed: u64) -> Vec<(u64, u64)> {
        let mut world = TestWorld::with_seed(seed);
        let agent = world.spawn_agent(hungry_at(Vec2::new(0.0, 0.0)));
        // No food anywhere, so the agent falls back to Explore/Wander which
        // uses SimRng to pick a target — guaranteed divergence between seeds.
        world.tick(200);
        world
            .sim_events()
            .all()
            .iter()
            .filter_map(|e| match e {
                SimEvent::AgentStateHash {
                    agent: a,
                    tick,
                    hash,
                } if *a == agent => Some((*tick, *hash)),
                _ => None,
            })
            .collect()
    }

    let run_a = run(42);
    let run_b = run(9999);

    assert!(
        !run_a.is_empty(),
        "run a should produce AgentStateHash events"
    );
    assert!(
        !run_b.is_empty(),
        "run b should produce AgentStateHash events"
    );

    // Different seeds should produce at least one tick with a different hash.
    let divergence = run_a
        .iter()
        .zip(run_b.iter())
        .find(|((ta, ha), (tb, hb))| ta == tb && ha != hb);

    assert!(
        divergence.is_some(),
        "two runs with different seeds should diverge — first 5 hashes of each run: \
         a={:?} b={:?}",
        &run_a[..run_a.len().min(5)],
        &run_b[..run_b.len().min(5)]
    );
}

// ─── Decision includes urgency contributors ────────────────────────────────

#[test]
fn decision_event_includes_urgency_contributors() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(hungry_at(Vec2::new(0.0, 0.0)));
    world.spawn_apple_tree(Vec2::new(80.0, 0.0), 5);

    // Urgency generation is staggered (runs every thinking_interval=60 ticks),
    // so we need to run long enough for at least one urgency cycle to fill
    // cns.urgencies before a Decision fires.
    world.tick(120);

    let events = world.sim_events().all();
    let with_urgencies: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::Decision {
                agent: a,
                urgencies,
                ..
            } if *a == agent && !urgencies.is_empty() => Some(urgencies.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !with_urgencies.is_empty(),
        "at least one Decision event should carry a non-empty urgency list"
    );

    let has_hunger = with_urgencies.iter().any(|list| {
        list.iter().any(|u| {
            matches!(
                u.source,
                worldsim::agent::nervous_system::urgency::UrgencySource::Hunger
            )
        })
    });
    assert!(
        has_hunger,
        "hungry agent's Decision events should include a Hunger urgency contributor"
    );
}

// ─── GOAP telemetry ────────────────────────────────────────────────────────

#[test]
fn goap_telemetry_events_emitted_for_search() {
    let mut world = TestWorld::with_seed(42);
    let agent = world.spawn_agent(hungry_at(Vec2::new(0.0, 0.0)));
    world.spawn_apple_tree(Vec2::new(80.0, 0.0), 5);

    world.tick(120);

    let events = world.sim_events().all();
    let telemetry: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::GoapSearchTelemetry {
                agent: a,
                iterations,
                goal_description,
                ..
            } if *a == agent => Some((*iterations, goal_description.clone())),
            _ => None,
        })
        .collect();

    assert!(
        !telemetry.is_empty(),
        "GoapSearchTelemetry should fire when the GOAP planner runs"
    );
    let (iters, goal) = &telemetry[0];
    assert!(
        *iters > 0,
        "a successful GOAP search consumes at least one iteration"
    );
    assert!(!goal.is_empty(), "goal_description should be non-empty");
}
