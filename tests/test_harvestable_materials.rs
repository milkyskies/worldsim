use bevy::prelude::*;
use worldsim::agent::mind::knowledge::{
    Concept, Node as MindNode, Predicate, Value, setup_ontology,
};
use worldsim::testing::TestWorld;

/// WoodLog→Wood and StoneNode→Stone are universal ontology facts, accessible
/// to all agents through their MindGraph's ontology layer.
#[test]
fn ontology_has_wood_log_produces_wood() {
    let ontology = setup_ontology();
    let knows = ontology.triples.iter().any(|t| {
        t.subject == MindNode::Concept(Concept::WoodLog)
            && t.predicate == Predicate::Produces
            && t.object == Value::Item(Concept::Wood, 1)
    });
    assert!(
        knows,
        "Ontology should contain WoodLog→Wood production triple (universal fact)"
    );
}

#[test]
fn ontology_has_stone_node_produces_stone() {
    let ontology = setup_ontology();
    let knows = ontology.triples.iter().any(|t| {
        t.subject == MindNode::Concept(Concept::StoneNode)
            && t.predicate == Predicate::Produces
            && t.object == Value::Item(Concept::Stone, 1)
    });
    assert!(
        knows,
        "Ontology should contain StoneNode→Stone production triple (universal fact)"
    );
}

/// Stone node spawns with the expected stone count in its inventory.
#[test]
fn stone_node_starts_with_stone_inventory() {
    let mut world = TestWorld::with_seed(42);
    let node = world.spawn_stone_node(Vec2::new(50.0, 50.0), 5);
    assert_eq!(world.item_count(node, Concept::Stone), 5);
}

/// Wood log spawns with the expected wood count in its inventory.
#[test]
fn wood_log_starts_with_wood_inventory() {
    let mut world = TestWorld::with_seed(42);
    let log = world.spawn_wood_log(Vec2::new(60.0, 60.0), 4);
    assert_eq!(world.item_count(log, Concept::Wood), 4);
}

/// Stone node with no stones starts empty.
#[test]
fn stone_node_with_zero_stones_is_empty() {
    let mut world = TestWorld::with_seed(42);
    let node = world.spawn_stone_node(Vec2::new(50.0, 50.0), 0);
    assert_eq!(world.item_count(node, Concept::Stone), 0);
}

/// Wood log with no wood starts empty.
#[test]
fn wood_log_with_zero_wood_is_empty() {
    let mut world = TestWorld::with_seed(42);
    let log = world.spawn_wood_log(Vec2::new(60.0, 60.0), 0);
    assert_eq!(world.item_count(log, Concept::Wood), 0);
}
