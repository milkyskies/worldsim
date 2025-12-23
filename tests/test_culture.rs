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
    // Both should know eating
    let farmer_knowledge = create_cultural_knowledge(Culture::Farmer);
    let nomad_knowledge = create_cultural_knowledge(Culture::Nomad);

    let check = |k: &Vec<worldsim::agent::mind::knowledge::Triple>| {
        k.iter().any(|t| {
            t.subject == MindNode::Action(worldsim::agent::actions::ActionType::Eat)
                && t.predicate == Predicate::Satisfies
        })
    };

    assert!(check(&farmer_knowledge), "Farmer should know eating");
    assert!(check(&nomad_knowledge), "Nomad should know eating");
}
