use worldsim::agent::mind::knowledge::{
    Concept, Node as MindNode, Predicate, Value, setup_ontology,
};

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
