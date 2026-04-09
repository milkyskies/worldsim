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

/// The emotional brain should propose Attack when a wolf's mind contains a
/// visible deer entity (simulating what perception writes). Tests the mechanism
/// directly rather than the full simulation chain.
#[test]
fn wolf_emotional_brain_proposes_attack_for_visible_deer() {
    use worldsim::agent::actions::{ActionRegistry, ActionType};
    use worldsim::agent::brains::emotional::emotional_brain_propose;
    use worldsim::agent::mind::knowledge::{Metadata, Node, Triple, Value};
    use worldsim::agent::mind::perception::VisibleObjects;
    use worldsim::agent::psyche::emotions::EmotionalState;

    // Get a wolf mind via the public TestWorld API so it has innate knowledge applied.
    let mut world = TestWorld::with_seed(42);
    let wolf = world.spawn_wolf(Vec2::new(0.0, 0.0));
    let wolf_mind = world.get::<MindGraph>(wolf).clone();

    // Simulate what the perception system writes when a deer entity is observed.
    let mut mind = wolf_mind;
    let deer_entity = bevy::ecs::entity::Entity::from_bits(1);
    mind.assert(Triple::with_meta(
        Node::Entity(deer_entity),
        Predicate::IsA,
        Value::Concept(Concept::Deer),
        Metadata::perception(0),
    ));

    let mut visible = VisibleObjects::default();
    visible.entities.push(deer_entity);

    let emotions = EmotionalState::default();
    let mut registry = ActionRegistry::default();
    registry.register(worldsim::agent::actions::action::AttackAction);
    registry.register(worldsim::agent::actions::action::WalkAction);
    registry.register(worldsim::agent::actions::action::FleeAction);

    let proposal = emotional_brain_propose(&emotions, &mind, &visible, &registry);

    assert!(
        proposal.is_some(),
        "emotional brain should propose an action when wolf sees deer"
    );
    assert_eq!(
        proposal.unwrap().action.action_type,
        ActionType::Attack,
        "wolf should want to attack a visible deer"
    );
}
