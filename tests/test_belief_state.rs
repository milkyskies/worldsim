use worldsim::agent::brains::thinking::TriplePattern;
use worldsim::agent::mind::belief_state::BeliefState;
use worldsim::agent::mind::knowledge::{
    Concept, MemoryType, Metadata, MindGraph, Node as MindNode, Predicate, Source, Triple, Value,
};

#[test]
fn test_belief_state_estimates() {
    // 1. Setup MindGraph with empty ontology
    let mut mind = MindGraph::new(worldsim::agent::mind::knowledge::Ontology::default());

    // 2. Setup a Tree at (5,5) with 80% confidence
    let tree_entity = bevy::prelude::Entity::from_bits(100);

    let meta_loc = Metadata {
        confidence: 0.8,
        timestamp: 0,
        source: Source::Perception,
        memory_type: MemoryType::Perception,
        informant: None,
        evidence: vec![],
        salience: 0.0,
    };

    mind.assert(Triple::with_meta(
        MindNode::Entity(tree_entity),
        Predicate::LocatedAt,
        Value::Tile((5, 5)),
        meta_loc,
    ));

    // 3. Setup that Tree contains Apples with 50% confidence
    let meta_cont = Metadata {
        confidence: 0.5,
        timestamp: 0,
        source: Source::Perception,
        memory_type: MemoryType::Perception,
        informant: None,
        evidence: vec![],
        salience: 0.0,
    };

    mind.assert(Triple::with_meta(
        MindNode::Entity(tree_entity),
        Predicate::Contains,
        Value::Item(Concept::Apple, 3),
        meta_cont,
    ));

    // 4. Create BeliefState
    let belief = BeliefState::new(&mind);

    // 5. Test pattern confidence (new API)
    let pattern = TriplePattern::new(
        Some(MindNode::Entity(tree_entity)),
        Some(Predicate::Contains),
        None,
    );
    let conf = belief.pattern_confidence(&pattern);
    println!("Confidence of tree containing items: {}", conf);
    assert!((conf - 0.5).abs() < 0.001);

    // 6. Test pattern_exists
    assert!(belief.pattern_exists(&pattern));

    // 7. Test non-existent pattern
    let missing_pattern = TriplePattern::new(
        Some(MindNode::Entity(bevy::prelude::Entity::from_bits(999))),
        Some(Predicate::Contains),
        None,
    );
    assert!(!belief.pattern_exists(&missing_pattern));
    assert_eq!(belief.pattern_confidence(&missing_pattern), 0.0);

    // 8. Test MindGraph helper methods
    assert!(mind.has_any(&MindNode::Entity(tree_entity), Concept::Apple));
    assert_eq!(
        mind.count_of(&MindNode::Entity(tree_entity), Concept::Apple),
        3
    );
    assert!(!mind.has_any(&MindNode::Entity(tree_entity), Concept::Stone));
}
