use crate::agent::mind::knowledge::{
    Concept, MemoryType, Metadata, Node, Ontology, Predicate, Source, Triple, Value,
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
///
/// Universal production facts (WoodLog→Wood, StoneNode→Stone) are now in the
/// shared ontology and no longer seeded here — agents access them via the
/// ontology layer in their MindGraph. Only culture-differentiating knowledge
/// (AppleTree→Apple for Farmers/Gatherers, BerryBush→Berry for Gatherers) is
/// seeded here as shared cultural knowledge with Source::Cultural metadata.
pub fn create_cultural_knowledge(culture: Culture, _ontology: &Ontology) -> Vec<Triple> {
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

    // Material resources — Wood and Stone production facts are in the
    // base ontology (setup_ontology), so agents find them there.
    // Only classification triples are seeded here.
    add(c(Wood), IsA, v(Resource));
    add(c(Stone), IsA, v(Resource));

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

    // ─── Universal recipe knowledge (all cultures know these) ───

    // Campfire: Wood(3) → provides Safety, Warmth, Light
    add(
        c(Campfire),
        Requires,
        Value::Item(
            Wood,
            crate::constants::actions::build::CAMPFIRE_WOOD_REQUIRED,
        ),
    );
    add(c(Campfire), Provides, v(Warmth));
    add(c(Campfire), Provides, v(Safety));
    add(c(Campfire), Provides, v(Light));
    add(
        c(Campfire),
        BuildTime,
        Value::Int(crate::constants::actions::build::CAMPFIRE_DURATION_TICKS as i32),
    );

    // Lean-to shelter: Wood(5) + LargeLeaves(2) → provides Safety
    add(
        c(LeanTo),
        Requires,
        Value::Item(
            Wood,
            crate::constants::actions::build::LEAN_TO_WOOD_REQUIRED,
        ),
    );
    add(c(LeanTo), Provides, v(Safety));
    add(
        c(LeanTo),
        BuildTime,
        Value::Int(crate::constants::actions::build::LEAN_TO_DURATION_TICKS as i32),
    );

    // ─── Specific Cultural Knowledge ───

    match culture {
        Culture::Nomad => {
            // Nomads know about movement and minimal gathering
            add(c(Apple), IsA, v(Food));
            add(c(Water), IsA, v(Resource));
        }
        Culture::Farmer => {
            // Farmers know trees produce apples. HasTrait Harvestable is
            // inherited from Plant→Harvestable in the ontology.
            add(c(AppleTree), Produces, Value::Item(Apple, 1));
            add(c(Apple), IsA, v(Food));
            add(c(AppleTree), RegenerationRate, Value::Float(10.0));
        }
        Culture::Hunter => {
            // Hunters know animals are food (not fully implemented yet)
            add(c(Animal), IsA, v(Food));
            add(c(Animal), HasTrait, v(Harvestable));
        }
        Culture::Gatherer => {
            // Gatherers know diverse plants - both apples and berries.
            // HasTrait Harvestable for AppleTree and BerryBush is inherited
            // from Plant→Harvestable in the ontology.
            add(c(Apple), IsA, v(Food));
            add(c(AppleTree), Produces, Value::Item(Apple, 1));
            add(c(Berry), IsA, v(Food));
            add(c(BerryBush), Produces, Value::Item(Berry, 1));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::setup_ontology;

    fn ontology() -> Ontology {
        setup_ontology()
    }

    #[test]
    fn gatherer_knows_berrybush_produces_berry() {
        let triples = create_cultural_knowledge(Culture::Gatherer, &ontology());
        let has_it = triples.iter().any(|t| {
            t.subject == Node::Concept(Concept::BerryBush)
                && t.predicate == Predicate::Produces
                && t.object == Value::Item(Concept::Berry, 1)
        });
        assert!(
            has_it,
            "Gatherer culture should know BerryBush produces Berry"
        );
    }

    #[test]
    fn nomad_does_not_know_berrybush_produces_berry() {
        let triples = create_cultural_knowledge(Culture::Nomad, &ontology());
        let has_it = triples.iter().any(|t| {
            t.subject == Node::Concept(Concept::BerryBush)
                && t.predicate == Predicate::Produces
                && t.object == Value::Item(Concept::Berry, 1)
        });
        assert!(
            !has_it,
            "Nomad culture should not know BerryBush produces Berry"
        );
    }

    #[test]
    fn farmer_does_not_know_berrybush_produces_berry() {
        let triples = create_cultural_knowledge(Culture::Farmer, &ontology());
        let has_it = triples.iter().any(|t| {
            t.subject == Node::Concept(Concept::BerryBush) && t.predicate == Predicate::Produces
        });
        assert!(
            !has_it,
            "Farmer culture should not know BerryBush produces anything"
        );
    }

    #[test]
    fn no_duplicate_woodlog_produces_triples() {
        let ontology = ontology();
        // WoodLog→Wood is in the ontology; culture should not re-add it.
        for culture in [
            Culture::Nomad,
            Culture::Farmer,
            Culture::Hunter,
            Culture::Gatherer,
        ] {
            let triples = create_cultural_knowledge(culture, &ontology);
            let count = triples
                .iter()
                .filter(|t| {
                    t.subject == Node::Concept(Concept::WoodLog)
                        && t.predicate == Predicate::Produces
                })
                .count();
            assert_eq!(
                count, 0,
                "{culture:?} culture should not redefine WoodLog production (it's in the ontology)"
            );
        }
    }

    #[test]
    fn no_duplicate_stonenode_produces_triples() {
        let ontology = ontology();
        // StoneNode→Stone is in the ontology; culture should not re-add it.
        for culture in [
            Culture::Nomad,
            Culture::Farmer,
            Culture::Hunter,
            Culture::Gatherer,
        ] {
            let triples = create_cultural_knowledge(culture, &ontology);
            let count = triples
                .iter()
                .filter(|t| {
                    t.subject == Node::Concept(Concept::StoneNode)
                        && t.predicate == Predicate::Produces
                })
                .count();
            assert_eq!(
                count, 0,
                "{culture:?} culture should not redefine StoneNode production (it's in the ontology)"
            );
        }
    }

    #[test]
    fn no_hastraitharvestable_duplicates_in_culture() {
        // HasTrait Harvestable for AppleTree and BerryBush is inherited from
        // Plant in the ontology. No culture should manually re-add these.
        let ontology = ontology();
        for culture in [
            Culture::Nomad,
            Culture::Farmer,
            Culture::Hunter,
            Culture::Gatherer,
        ] {
            let triples = create_cultural_knowledge(culture, &ontology);
            for concept in [Concept::AppleTree, Concept::BerryBush] {
                let has_explicit = triples.iter().any(|t| {
                    t.subject == Node::Concept(concept)
                        && t.predicate == Predicate::HasTrait
                        && t.object == Value::Concept(Concept::Harvestable)
                });
                assert!(
                    !has_explicit,
                    "{culture:?} culture has redundant ({}:?, HasTrait, Harvestable) — inherited from Plant",
                    format!("{concept:?}")
                );
            }
        }
    }

    #[test]
    fn ontology_has_woodlog_and_stonenode_produces() {
        let ontology = ontology();
        let has_wood = ontology.triples.iter().any(|t| {
            t.subject == Node::Concept(Concept::WoodLog)
                && t.predicate == Predicate::Produces
                && t.object == Value::Item(Concept::Wood, 1)
        });
        let has_stone = ontology.triples.iter().any(|t| {
            t.subject == Node::Concept(Concept::StoneNode)
                && t.predicate == Predicate::Produces
                && t.object == Value::Item(Concept::Stone, 1)
        });
        assert!(
            has_wood,
            "Ontology should have WoodLog→Wood production triple"
        );
        assert!(
            has_stone,
            "Ontology should have StoneNode→Stone production triple"
        );
    }

    #[test]
    fn appletree_inherits_harvestable_from_plant_in_ontology() {
        let ontology = ontology();
        // AppleTree IsA Plant, Plant HasTrait Harvestable → AppleTree inherits
        // Harvestable through the ontology's IsA chain.
        assert!(
            ontology.has_trait(Concept::AppleTree, Concept::Harvestable),
            "AppleTree should inherit Harvestable from Plant via the ontology"
        );
        assert!(
            ontology.has_trait(Concept::BerryBush, Concept::Harvestable),
            "BerryBush should inherit Harvestable from Plant via the ontology"
        );
    }
}
