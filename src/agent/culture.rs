use crate::agent::mind::knowledge::{
    Concept, MemoryType, Metadata, Node, Predicate, Source, Triple, Value,
};
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default)]
pub enum Culture {
    #[default]
    Nomad,
    Farmer,
    Hunter,
    Gatherer,
}

impl Culture {
    pub fn description(&self) -> &'static str {
        match self {
            Culture::Nomad => "Wanderers who know basic survival",
            Culture::Farmer => "Settlers who know how to grow food",
            Culture::Hunter => "Hunters who know how to track and kiil",
            Culture::Gatherer => "Foragers who know which plants are safe",
        }
    }
}

/// Generates a set of innate knowledge triples based on the given culture.
pub fn create_cultural_knowledge(culture: Culture) -> Vec<Triple> {
    let mut triples = Vec::new();
    let current_time = 0; // Innate knowledge exists from the beginning

    // Helper macro for cleaner insertion
    let mut add = |s: Node, p: Predicate, o: Value| {
        triples.push(Triple::with_meta(
            s,
            p,
            o,
            Metadata {
                source: Source::Cultural,
                memory_type: MemoryType::Cultural,
                timestamp: current_time,
                confidence: 0.8, // Cultural knowledge is trusted but not verified personally
                salience: 0.5,
                ..Default::default()
            },
        ));
    };

    use Concept::*;
    use Predicate::*;
    let c = |con: Concept| Node::Concept(con);
    let v = |con: Concept| Value::Concept(con);

    // ─── Universal Cultural Knowledge (All cultures know this) ───

    // Physiological needs
    add(c(Thing), IsA, v(Physical)); // Basic ontology grounded
    add(
        Node::Action(crate::agent::actions::ActionType::Eat),
        Satisfies,
        Value::Concept(Concept::Thing),
    ); // Placeholder, refined below

    // Better action semantics
    // (Eat, Satisfies, Hunger)
    add(
        Node::Action(crate::agent::actions::ActionType::Eat),
        Satisfies,
        Value::Concept(Concept::Thing),
    ); // Wait, Hunger isn't a Concept yet? It's a predicate.
    // We need to express "Eat action satisfies Hunger stat".
    // Our Value enum has Entity, Concept, Action...
    // Let's check Predicate::Hunger. It's (Self, Hunger, Int).
    // The previous design doc said (Eat, Satisfies, Hunger).
    // Maybe we need a specific node for "Hunger"?
    // For now, let's assume agents just "know" to eat food.

    // "Food is Edible"
    add(c(Food), HasTrait, v(Edible));

    // ─── Specific Cultural Knowledge ───

    match culture {
        Culture::Nomad => {
            // Nomads know about movement and minimal gathering
            add(c(Apple), IsA, v(Food));
            add(c(Water), IsA, v(Resource));
        }
        Culture::Farmer => {
            // Farmers know trees produce apples
            add(c(AppleTree), Produces, Value::Item(Apple, 1));
            add(c(AppleTree), HasTrait, v(Harvestable)); // Can harvest from trees
            add(c(Apple), IsA, v(Food));
            add(c(AppleTree), RegenerationRate, Value::Float(10.0));
        }
        Culture::Hunter => {
            // Hunters know animals are food (not fully implemented yet)
            add(c(Animal), IsA, v(Food));
            add(c(Animal), HasTrait, v(Harvestable));
        }
        Culture::Gatherer => {
            // Gatherers know diverse plants - both apples and berries
            add(c(Apple), IsA, v(Food));
            add(c(AppleTree), Produces, Value::Item(Apple, 1));
            add(c(AppleTree), HasTrait, v(Harvestable)); // Can harvest from trees
            add(c(Berry), IsA, v(Food));
            add(c(BerryBush), Produces, Value::Item(Berry, 1));
            add(c(BerryBush), HasTrait, v(Harvestable)); // Can harvest from bushes
        }
    }

    triples
}

// Helper to convert ActionType to Node
impl From<crate::agent::actions::ActionType> for Node {
    fn from(action: crate::agent::actions::ActionType) -> Self {
        Node::Action(action)
    }
}
