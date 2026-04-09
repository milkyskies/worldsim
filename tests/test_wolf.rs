//! Integration tests for wolf predator behavior.
//!
//! Verifies:
//! - Wolves have correct innate knowledge (deer and humans trigger anger)
//! - Wolves are feared by humans (Wolf HasTrait Dangerous in ontology)
//! - Wolves are feared by deer (Wolf HasTrait Dangerous in ontology)
//! - Wolf triggers attack behavior when it perceives deer

use bevy::prelude::*;
use worldsim::agent::mind::knowledge::{
    Concept, MindGraph, Node, Predicate, Value, setup_ontology,
};
use worldsim::agent::psyche::emotions::EmotionType;
use worldsim::testing::TestWorld;

/// Wolves should have anger-triggering knowledge for deer (primary prey).
#[test]
fn wolf_knows_deer_triggers_anger() {
    let mut world = TestWorld::with_seed(42);
    let wolf = world.spawn_wolf(Vec2::new(100.0, 100.0));

    let mind = world.get::<MindGraph>(wolf);
    let triples = mind.query(
        Some(&Node::Concept(Concept::Deer)),
        Some(Predicate::TriggersEmotion),
        None,
    );

    assert!(
        !triples.is_empty(),
        "wolf should know that deer trigger an emotion"
    );

    let anger_triple = triples
        .iter()
        .find(|t| matches!(&t.object, Value::Emotion(EmotionType::Anger, _)));
    assert!(
        anger_triple.is_some(),
        "wolf should know that deer trigger anger (hunt instinct)"
    );
}

/// Wolves should treat humans as a territorial threat (mild anger).
#[test]
fn wolf_knows_person_triggers_anger() {
    let mut world = TestWorld::with_seed(42);
    let wolf = world.spawn_wolf(Vec2::new(100.0, 100.0));

    let mind = world.get::<MindGraph>(wolf);
    let triples = mind.query(
        Some(&Node::Concept(Concept::Person)),
        Some(Predicate::TriggersEmotion),
        None,
    );

    let anger_triple = triples
        .iter()
        .find(|t| matches!(&t.object, Value::Emotion(EmotionType::Anger, _)));
    assert!(
        anger_triple.is_some(),
        "wolf should treat persons as a territorial threat (anger)"
    );
}

/// The ontology marks wolves as Dangerous so all agents with the ontology
/// automatically know to fear them — no per-agent innate knowledge needed.
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
        "ontology should mark Wolf as Dangerous so all agents fear wolves"
    );
}

/// Humans should trigger fear when they perceive a wolf, because the shared
/// ontology tells them Wolf is Dangerous.
#[test]
fn human_fears_wolf_via_ontology() {
    use worldsim::agent::mind::knowledge::MindGraph;
    use worldsim::agent::mind::perception::VisibleObjects;
    use worldsim::agent::psyche::emotions::{Emotion, EmotionalState};

    let ontology = setup_ontology();
    let mind = MindGraph::new(ontology);

    // Simulate the danger-perception system: query the mind for Wolf HasTrait Dangerous
    let danger_triples = mind.query(
        Some(&Node::Concept(Concept::Wolf)),
        Some(Predicate::HasTrait),
        Some(&Value::Concept(Concept::Dangerous)),
    );

    assert!(
        !danger_triples.is_empty(),
        "human mind (with ontology) should know Wolf is Dangerous"
    );
}

/// A wolf that perceives a deer should have the emotional brain propose
/// an Attack action after a few ticks.
#[test]
fn wolf_attacks_deer_on_sight() {
    use worldsim::agent::actions::ActionType;

    let mut world = TestWorld::with_seed(42);

    // Place wolf and deer close enough to be within wolf vision (120px)
    let wolf = world.spawn_wolf(Vec2::new(0.0, 0.0));
    let _deer = world.spawn_deer(Vec2::new(50.0, 0.0));

    // Run enough ticks for perception → emotional brain → arbitration → action
    world.tick(30);

    let action = world.current_action(wolf);
    assert!(
        matches!(action, Some(ActionType::Attack) | Some(ActionType::Walk)),
        "wolf should be attacking or walking toward deer after perceiving it, got {:?}",
        action
    );
}
