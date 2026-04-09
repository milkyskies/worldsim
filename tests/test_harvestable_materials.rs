use bevy::prelude::*;
use worldsim::agent::culture::{Culture, create_cultural_knowledge};
use worldsim::agent::mind::knowledge::{Concept, Node as MindNode, Predicate, Value};
use worldsim::testing::TestWorld;

/// All cultures share universal knowledge about stone and wood nodes.
#[test]
fn all_cultures_know_stone_node_produces_stone() {
    for culture in [
        Culture::Nomad,
        Culture::Farmer,
        Culture::Hunter,
        Culture::Gatherer,
    ] {
        let knowledge = create_cultural_knowledge(culture);
        let knows = knowledge.iter().any(|t| {
            t.subject == MindNode::Concept(Concept::StoneNode)
                && t.predicate == Predicate::Produces
                && t.object == Value::Item(Concept::Stone, 1)
        });
        assert!(knows, "{culture:?} should know StoneNode produces Stone");
    }
}

/// All cultures share universal knowledge about wood logs.
#[test]
fn all_cultures_know_wood_log_produces_wood() {
    for culture in [
        Culture::Nomad,
        Culture::Farmer,
        Culture::Hunter,
        Culture::Gatherer,
    ] {
        let knowledge = create_cultural_knowledge(culture);
        let knows = knowledge.iter().any(|t| {
            t.subject == MindNode::Concept(Concept::WoodLog)
                && t.predicate == Predicate::Produces
                && t.object == Value::Item(Concept::Wood, 1)
        });
        assert!(knows, "{culture:?} should know WoodLog produces Wood");
    }
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
