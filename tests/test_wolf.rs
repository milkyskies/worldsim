//! Integration tests for wolf predator behavior.
//!
//! Verifies:
//! - Wolves have correct innate knowledge (prey recognition, danger awareness)
//! - No hardcoded emotion triggers — behavior emerges from drives and knowledge
//! - Wolves are feared by humans (Wolf HasTrait Dangerous in shared ontology)
//! - Pack bonding is established at spawn

use bevy::prelude::*;
use worldsim::agent::mind::knowledge::{
    Concept, MindGraph, Node, Predicate, Value, setup_ontology,
};
use worldsim::testing::TestWorld;

/// Wolves should recognize deer as food intrinsically.
#[test]
fn wolf_knows_deer_is_food() {
    let mut world = TestWorld::with_seed(42);
    let wolf = world.spawn_wolf(Vec2::new(100.0, 100.0));

    let mind = world.get::<MindGraph>(wolf);
    let triples = mind.query(
        Some(&Node::Concept(Concept::Deer)),
        Some(Predicate::IsA),
        Some(&Value::Concept(Concept::Food)),
    );

    assert!(
        !triples.is_empty(),
        "wolf should have intrinsic knowledge that Deer IsA Food"
    );
}

/// Wolves should know humans are dangerous intrinsically.
#[test]
fn wolf_knows_humans_are_dangerous() {
    let mut world = TestWorld::with_seed(42);
    let wolf = world.spawn_wolf(Vec2::new(100.0, 100.0));

    let mind = world.get::<MindGraph>(wolf);
    let triples = mind.query(
        Some(&Node::Concept(Concept::Person)),
        Some(Predicate::HasTrait),
        Some(&Value::Concept(Concept::Dangerous)),
    );

    assert!(
        !triples.is_empty(),
        "wolf should have innate wariness toward humans (Person HasTrait Dangerous)"
    );
}

/// No hardcoded emotion triggers — wolf behavior emerges from drives and knowledge.
#[test]
fn wolf_has_no_triggers_emotion_triples() {
    let mut world = TestWorld::with_seed(42);
    let wolf = world.spawn_wolf(Vec2::new(100.0, 100.0));

    let mind = world.get::<MindGraph>(wolf);
    let triples = mind.query(None, Some(Predicate::TriggersEmotion), None);

    assert!(
        triples.is_empty(),
        "wolf MindGraph must contain no TriggersEmotion triples — emotions emerge from outcome processing, not trigger scripts"
    );
}

/// The shared ontology marks wolves as Dangerous so all agents automatically
/// know to be cautious around them — no per-agent innate knowledge needed.
#[test]
fn ontology_marks_wolf_as_dangerous() {
    let ontology = setup_ontology();

    let triples = ontology.triples.iter().filter(|t| {
        t.subject == Node::Concept(Concept::Wolf)
            && t.predicate == Predicate::HasTrait
            && t.object == Value::Concept(Concept::Dangerous)
    });

    assert!(
        triples.count() > 0,
        "shared ontology should mark Wolf as Dangerous so all agents fear wolves"
    );
}

/// Humans should trigger fear when they perceive a wolf because the shared
/// ontology tells them Wolf is Dangerous.
#[test]
fn human_fears_wolf_via_ontology() {
    let ontology = setup_ontology();
    let mind = MindGraph::new(ontology);

    let danger_triples = mind.query(
        Some(&Node::Concept(Concept::Wolf)),
        Some(Predicate::HasTrait),
        Some(&Value::Concept(Concept::Dangerous)),
    );

    assert!(
        !danger_triples.is_empty(),
        "human mind (with shared ontology) should know Wolf is Dangerous"
    );
}

/// Wolves spawned as a pack should have mutual friend bonds.
#[test]
fn wolf_pack_bonds_established_at_spawn() {
    let mut world = TestWorld::with_seed(42);
    let wolves = world.spawn_wolf_pack(&[Vec2::new(40.0, 40.0), Vec2::new(50.0, 50.0)]);
    let (wolf_a, wolf_b) = (wolves[0], wolves[1]);

    let mind_a = world.get::<MindGraph>(wolf_a);
    let trust = mind_a.query(Some(&Node::Entity(wolf_b)), Some(Predicate::Trust), None);
    assert!(
        !trust.is_empty(),
        "wolf_a should have a Trust triple for wolf_b (pack bond)"
    );

    let mind_b = world.get::<MindGraph>(wolf_b);
    let friend = mind_b.query(
        Some(&Node::Entity(wolf_a)),
        Some(Predicate::IsA),
        Some(&Value::Concept(Concept::Friend)),
    );
    assert!(
        !friend.is_empty(),
        "wolf_b should know wolf_a as a Friend (mutual pack bond)"
    );
}
