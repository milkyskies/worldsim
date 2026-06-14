use worldsim::agent::culture::{Culture, create_cultural_knowledge};
use worldsim::agent::mind::knowledge::{Concept, Node as MindNode, Predicate, Value};

#[test]
fn test_farmer_knowledge() {
    let knowledge = create_cultural_knowledge(Culture::Farmer);

    // Farmers should know AppleTrees produce Apples
    let knows_farming = knowledge.iter().any(|t| {
        t.subject == MindNode::Concept(Concept::AppleTree)
            && t.predicate == Predicate::Produces
            && t.object == Value::Item(Concept::Apple, 1)
    });

    assert!(knows_farming, "Farmer should know AppleTree produces Apple");
}

#[test]
fn test_nomad_knowledge() {
    let knowledge = create_cultural_knowledge(Culture::Nomad);

    // Nomads should NOT know AppleTrees produce Apples (in our simple definition)
    // Actually, create_cultural_knowledge(Nomad) currently only adds Apple IsA Food.

    let knows_farming = knowledge.iter().any(|t| {
        t.subject == MindNode::Concept(Concept::AppleTree) && t.predicate == Predicate::Produces
    });

    assert!(
        !knows_farming,
        "Nomad should NOT know complex farming facts"
    );
}

#[test]
fn test_universal_knowledge() {
    // All cultures share Thing IsA Physical as universal cultural knowledge.
    let farmer_knowledge = create_cultural_knowledge(Culture::Farmer);
    let nomad_knowledge = create_cultural_knowledge(Culture::Nomad);

    let check = |k: &Vec<worldsim::agent::mind::knowledge::Triple>| {
        k.iter().any(|t| {
            t.subject == MindNode::Concept(Concept::Thing)
                && t.predicate == Predicate::IsA
                && t.object == Value::Concept(Concept::Physical)
        })
    };

    assert!(
        check(&farmer_knowledge),
        "Farmer should have universal cultural knowledge"
    );
    assert!(
        check(&nomad_knowledge),
        "Nomad should have universal cultural knowledge"
    );
}
