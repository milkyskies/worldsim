#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use worldsim::agent::actions::action::harvest::HarvestAction;
    use worldsim::agent::actions::registry::Action;
    use worldsim::agent::mind::knowledge::{
        Concept, MindGraph, Node, Ontology, Predicate, Triple, Value,
    };

    #[test]
    fn test_harvest_knowledge_constraints() {
        // Setup Ontology (Shared Knowledge)
        let mut triples = Vec::new();
        triples.push(Triple::new(
            Node::Concept(Concept::Apple),
            Predicate::IsA,
            Value::Concept(Concept::Food),
        ));
        triples.push(Triple::new(
            Node::Concept(Concept::Berry),
            Predicate::IsA,
            Value::Concept(Concept::Food),
        ));
        triples.push(Triple::new(
            Node::Concept(Concept::Stone),
            Predicate::IsA,
            Value::Concept(Concept::Resource),
        ));

        let ontology = Ontology {
            triples: Arc::new(triples),
            trait_cache: Arc::new(HashMap::new()),
            parent_cache: Arc::new(HashMap::new()),
        };
        // We'd need to build caches if we used them, but simple queries might work without
        // Actually is_plan_valid uses mind.is_a(Concept::Food) which might rely on cache or query.
        // MindGraph::is_a relies on Ontology or local triples.
        // Let's assume MindGraph query works for IsA if triple exists.

        // Setup Agent Minds
        let mut deer_mind = MindGraph::new(ontology.clone());
        let mut human_mind = MindGraph::new(ontology.clone());

        // Define Entities using from_bits for dummy values
        let apple_tree = Entity::from_bits(1);
        let berry_bush = Entity::from_bits(2);
        let rock = Entity::from_bits(3);
        let nothing = Entity::from_bits(4);

        // Populate Knowledge

        // Human knows everything
        human_mind.assert(Triple::new(
            Node::Entity(apple_tree),
            Predicate::Produces,
            Value::Item(Concept::Apple, 1),
        ));
        human_mind.assert(Triple::new(
            Node::Entity(berry_bush),
            Predicate::Produces,
            Value::Item(Concept::Berry, 1),
        ));
        human_mind.assert(Triple::new(
            Node::Entity(rock),
            Predicate::Produces,
            Value::Item(Concept::Stone, 1),
        ));

        // Deer ONLY knows about berries
        deer_mind.assert(Triple::new(
            Node::Entity(berry_bush),
            Predicate::Produces,
            Value::Item(Concept::Berry, 1),
        ));
        // Deer fails to know AppleTree produces Apple

        let harvest = HarvestAction;

        // Test Human
        assert!(
            harvest.is_plan_valid(Some(apple_tree), &human_mind),
            "Human should harvest AppleTree"
        );
        assert!(
            harvest.is_plan_valid(Some(berry_bush), &human_mind),
            "Human should harvest BerryBush"
        );
        assert!(
            harvest.is_plan_valid(Some(rock), &human_mind),
            "Human should harvest Rock (Resource)"
        );
        assert!(
            !harvest.is_plan_valid(Some(nothing), &human_mind),
            "Human should NOT harvest Empty entity"
        );

        // Test Deer
        assert!(
            !harvest.is_plan_valid(Some(apple_tree), &deer_mind),
            "Deer should NOT harvest AppleTree (Unknown)"
        );
        assert!(
            harvest.is_plan_valid(Some(berry_bush), &deer_mind),
            "Deer should harvest BerryBush"
        );
        assert!(
            !harvest.is_plan_valid(Some(rock), &deer_mind),
            "Deer should NOT harvest Rock (Unknown)"
        );
    }
}
