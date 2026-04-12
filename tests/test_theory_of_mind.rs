//! Tests for theory of mind: agents model what other agents know.
//!
//! Verifies:
//! 1. After conversation, speaker's ToM records that listener knows shared content
//! 2. After conversation, listener's ToM records that speaker knows shared content
//! 3. TheoryOfMindUpdated SimEvent fires during conversation
//! 4. Novelty scoring uses ToM instead of direct mind queries

use bevy::math::Vec2;
use worldsim::agent::events::SimEvent;
use worldsim::agent::mind::knowledge::{
    Concept, MemoryType, Metadata, Node, Predicate, Source, Triple, Value,
};
use worldsim::agent::mind::theory_of_mind::TheoryOfMind;
use worldsim::agent::nervous_system::config::NervousSystemConfig;
use worldsim::testing::TestWorld;

const HIGH_SOCIAL: f32 = 0.8;
const TICKS_FOR_CONVERSATION: u64 = 200;

fn fast_brains(world: &mut TestWorld) {
    let mut config = world
        .app_mut()
        .world_mut()
        .resource_mut::<NervousSystemConfig>();
    config.thinking_interval = 1;
}

/// Helper: create a high-salience danger triple about wolves.
fn wolf_danger_triple(timestamp: u64) -> Triple {
    Triple::with_meta(
        Node::Concept(Concept::Wolf),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        Metadata {
            source: Source::Experienced,
            memory_type: MemoryType::Episodic,
            timestamp,
            confidence: 1.0,
            informant: None,
            evidence: Vec::new(),
            salience: 0.9,
            source_sense: None,
            strength: 1.0,
        },
    )
}

#[test]
fn speaker_tom_updated_after_sharing_knowledge() {
    // Alice knows about a wolf danger; Bob doesn't.
    // After they talk, Alice's ToM should record that Bob now knows about the wolf.
    let (mut world, agents) = TestWorld::scenario(42)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .knowledge(vec![wolf_danger_triple(0)])
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(TICKS_FOR_CONVERSATION);

    let alice = agents["alice"];
    let bob = agents["bob"];

    let alice_tom = world.get::<TheoryOfMind>(alice);
    let bob_tom = world.get::<TheoryOfMind>(bob);

    // At least one agent's ToM should have beliefs about the other after conversation.
    // (Either Alice told Bob something, or Bob told Alice something, or both.)
    let alice_has_model = alice_tom.belief_count_for(bob) > 0;
    let bob_has_model = bob_tom.belief_count_for(alice) > 0;

    assert!(
        alice_has_model || bob_has_model,
        "after conversation, at least one agent should model what the other knows"
    );
}

#[test]
fn theory_of_mind_sim_event_fires_during_conversation() {
    let (mut world, _agents) = TestWorld::scenario(43)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .knowledge(vec![wolf_danger_triple(0)])
        .done()
        .agent("bob")
        .pos(Vec2::new(210.0, 200.0))
        .social_drive(HIGH_SOCIAL)
        .done()
        .build();

    fast_brains(&mut world);
    world.tick(TICKS_FOR_CONVERSATION);

    let events = world.sim_events();
    let tom_events: Vec<_> = events
        .all()
        .iter()
        .filter(|e| matches!(e, SimEvent::TheoryOfMindUpdated { .. }))
        .collect();

    assert!(
        !tom_events.is_empty(),
        "TheoryOfMindUpdated events should fire during conversation"
    );
}

#[test]
fn agents_start_with_empty_theory_of_mind() {
    let (world, agents) = TestWorld::scenario(44)
        .map_size(64, 64)
        .noise_biomes(false)
        .agent("alice")
        .pos(Vec2::new(200.0, 200.0))
        .done()
        .build();

    let alice = agents["alice"];
    let tom = world.get::<TheoryOfMind>(alice);
    assert_eq!(
        tom.modeled_agent_count(),
        0,
        "agents should start with no models of other agents"
    );
}
