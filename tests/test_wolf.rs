//! Integration tests for wolf predator behavior.
//!
//! Verifies:
//! - Wolves have correct innate knowledge (prey recognition, danger awareness)
//! - No hardcoded emotion triggers — behavior emerges from drives and knowledge
//! - Wolves are feared by humans (Wolf HasTrait Dangerous in innate person knowledge)
//! - Pack bonding is established at spawn

use bevy::prelude::*;
use worldsim::agent::actions::ActionType;
use worldsim::agent::mind::knowledge::{
    Concept, Metadata, MindGraph, Node, Predicate, Source, Triple, Value, setup_ontology,
};
use worldsim::agent::psyche::emotions::{EmotionType, EmotionalState};
use worldsim::testing::{AgentConfig, TestWorld};

/// Wolves should know how to extract meat from deer intrinsically: deer is
/// prey, deer produces meat, and meat is food (the last fact lives in the
/// shared ontology). Together these triples give a wolf's rational planner
/// the full chain from "I'm hungry" to "kill that deer."
#[test]
fn wolf_knows_deer_yields_meat() {
    let mut world = TestWorld::with_seed(42);
    let wolf = world.spawn_wolf(Vec2::new(100.0, 100.0));

    let mind = world.get::<MindGraph>(wolf);

    // Direct innate knowledge: deer is prey.
    let prey = mind.query(
        Some(&Node::Concept(Concept::Deer)),
        Some(Predicate::HasTrait),
        Some(&Value::Concept(Concept::Prey)),
    );
    assert!(
        !prey.is_empty(),
        "wolf should know Deer HasTrait Prey intrinsically"
    );

    // Direct innate knowledge: deer produces meat.
    let produces = mind.query(
        Some(&Node::Concept(Concept::Deer)),
        Some(Predicate::Produces),
        Some(&Value::Item(Concept::Meat, 1)),
    );
    assert!(
        !produces.is_empty(),
        "wolf should know Deer Produces Meat intrinsically"
    );

    // Universal ontology fact: meat is food. The wolf inherits this through
    // its shared ontology rather than asserting it itself.
    assert!(
        mind.is_a(&Node::Concept(Concept::Meat), Concept::Food),
        "wolf's ontology should classify Meat IsA Food"
    );

    // Regression: the old `(Deer, IsA, Food)` category-error triple is gone.
    let category_error = mind.query(
        Some(&Node::Concept(Concept::Deer)),
        Some(Predicate::IsA),
        Some(&Value::Concept(Concept::Food)),
    );
    assert!(
        category_error.is_empty(),
        "wolf must not assert the category-error triple Deer IsA Food — \
         a live deer is not food, meat is food"
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

/// Wolves should know campfires are dangerous intrinsically (fire-fear).
/// Threat assessment is generic over `(concept, HasTrait, Dangerous)`, so this
/// triple alone is enough to make wolves treat visible campfires as threats.
#[test]
fn wolf_knows_campfire_is_dangerous() {
    let mut world = TestWorld::with_seed(42);
    let wolf = world.spawn_wolf(Vec2::new(100.0, 100.0));

    let mind = world.get::<MindGraph>(wolf);
    let triples = mind.query(
        Some(&Node::Concept(Concept::Campfire)),
        Some(Predicate::HasTrait),
        Some(&Value::Concept(Concept::Dangerous)),
    );

    assert!(
        !triples.is_empty(),
        "wolf should have innate fire-fear (Campfire HasTrait Dangerous)"
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

/// Humans should know Wolf is Dangerous via innate person knowledge,
/// so they will trigger fear when they perceive a wolf.
#[test]
fn human_fears_wolf_via_innate_knowledge() {
    let mut world = TestWorld::with_seed(42);
    let human = world.spawn_agent(AgentConfig::at(Vec2::ZERO));

    let mind = world.get::<MindGraph>(human);

    let danger_triples = mind.query(
        Some(&Node::Concept(Concept::Wolf)),
        Some(Predicate::HasTrait),
        Some(&Value::Concept(Concept::Dangerous)),
    );

    assert!(
        !danger_triples.is_empty(),
        "human mind should know Wolf is Dangerous via innate person knowledge"
    );
}

/// Regression for #222: an agent that *knows* wolves are dangerous (via either
/// shared ontology or experiential personal knowledge) but cannot currently
/// perceive a wolf must NOT enter a fear/Flee state. Stale or abstract knowledge
/// alone is not a present threat.
#[test]
fn agent_does_not_flee_from_abstract_wolf_danger_knowledge() {
    let mut world = TestWorld::with_seed(42);

    // Inject a high-salience experiential triple on top of the shared ontology
    // entry, mirroring the issue's reproduction recipe.
    let knowledge = vec![Triple::with_meta(
        Node::Concept(Concept::Wolf),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        Metadata {
            source: Source::Experienced,
            confidence: 1.0,
            salience: 0.9,
            ..Metadata::default()
        },
    )];

    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(200.0, 200.0),
        knowledge,
        ..AgentConfig::default()
    });

    // No wolf entity exists in this scenario.
    world.tick(120);

    let emotions = world.get::<EmotionalState>(agent);
    let fear: f32 = emotions
        .active_emotions
        .iter()
        .filter(|e| e.emotion_type == EmotionType::Fear)
        .map(|e| e.intensity)
        .sum();
    let action = world.current_action(agent).unwrap_or(ActionType::Idle);

    assert!(
        fear < 0.05,
        "agent with no perceived wolf must not accumulate Fear from abstract knowledge \
         (got fear={fear:.2}, current action={action:?})"
    );
    assert_ne!(
        action,
        ActionType::Flee,
        "agent must not enter Flee when no dangerous entity is perceived"
    );
}

/// Regression for #222: even when another *non-dangerous* agent is in vision
/// range (so VisibleObjects has entries), the threat-assessment system must not
/// confuse abstract wolf-danger knowledge with a perceived wolf and panic.
#[test]
fn agent_does_not_flee_from_wolf_knowledge_when_only_humans_are_visible() {
    let mut world = TestWorld::with_seed(42);

    let knowledge = vec![Triple::with_meta(
        Node::Concept(Concept::Wolf),
        Predicate::HasTrait,
        Value::Concept(Concept::Dangerous),
        Metadata {
            source: Source::Experienced,
            confidence: 1.0,
            salience: 0.9,
            ..Metadata::default()
        },
    )];

    let alice = world.spawn_agent(AgentConfig {
        pos: Vec2::new(200.0, 200.0),
        knowledge,
        ..AgentConfig::default()
    });
    let _bob = world.spawn_agent(AgentConfig {
        pos: Vec2::new(210.0, 200.0),
        ..AgentConfig::default()
    });

    world.tick(120);

    let emotions = world.get::<EmotionalState>(alice);
    let fear: f32 = emotions
        .active_emotions
        .iter()
        .filter(|e| e.emotion_type == EmotionType::Fear)
        .map(|e| e.intensity)
        .sum();
    let action = world.current_action(alice).unwrap_or(ActionType::Idle);

    assert!(
        fear < 0.05,
        "alice must not feel Fear when only bob (a Person) is visible \
         (got fear={fear:.2}, current action={action:?})"
    );
    assert_ne!(
        action,
        ActionType::Flee,
        "alice must not flee when no Wolf entity is perceived"
    );
}

/// Companion to the #222 regressions: a *real* visible wolf must still trigger
/// fear. The fix for #222 must not regress the in-vision case.
#[test]
fn agent_feels_fear_when_a_wolf_is_actually_visible() {
    let mut world = TestWorld::with_seed(42);

    let agent = world.spawn_agent(AgentConfig {
        pos: Vec2::new(200.0, 200.0),
        ..AgentConfig::default()
    });
    // Within human Vision range (100 px).
    let _wolf = world.spawn_wolf(Vec2::new(220.0, 200.0));

    world.tick(120);

    let emotions = world.get::<EmotionalState>(agent);
    let fear: f32 = emotions
        .active_emotions
        .iter()
        .filter(|e| e.emotion_type == EmotionType::Fear)
        .map(|e| e.intensity)
        .sum();

    assert!(
        fear > 0.1,
        "agent should feel Fear when a wolf is in vision range (got fear={fear:.2})"
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
