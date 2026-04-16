use crate::agent::mind::knowledge::{
    Concept, MemoryType, Metadata, Node, Predicate, Quantity, Source, Triple, Value,
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

/// Generates culture-specific innate knowledge with `Source::Cultural` metadata.
///
/// Universal facts (IsA hierarchy, Plant HasTrait Harvestable, WoodLog→Wood,
/// StoneNode→Stone) live in `setup_ontology` and are accessible to every agent
/// through the ontology layer. Only culture-differentiating knowledge lands
/// here — e.g. AppleTree→Apple for Farmers/Gatherers, BerryBush→Berry for
/// Gatherers, recipes for buildable structures.
pub fn create_cultural_knowledge(culture: Culture) -> Vec<Triple> {
    let mut triples = Vec::new();
    let current_time = 0;

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

    add(c(Thing), IsA, v(Physical));

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
        Value::Quantity(Quantity::Exact(
            crate::constants::actions::build::CAMPFIRE_DURATION_TICKS as f32,
        )),
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
        Value::Quantity(Quantity::Exact(
            crate::constants::actions::build::LEAN_TO_DURATION_TICKS as f32,
        )),
    );

    // ─── Specific Cultural Knowledge ───

    match culture {
        Culture::Nomad => {
            // Nomads know about movement and minimal gathering
            add(c(Apple), IsA, v(Food));
            add(c(Water), IsA, v(Resource));
        }
        Culture::Farmer => {
            add(c(AppleTree), Produces, Value::Item(Apple, 1));
            add(c(Apple), IsA, v(Food));
            add(
                c(AppleTree),
                RegenerationRate,
                Value::Quantity(Quantity::Exact(10.0)),
            );
        }
        Culture::Hunter => {
            // Hunters know deer are huntable prey that yield meat.
            // (Meat IsA Food is universal in the ontology.)
            add(c(Deer), HasTrait, v(Prey));
            add(c(Deer), Produces, Value::Item(Meat, 1));
        }
        Culture::Gatherer => {
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

    fn contains(triples: &[Triple], subject: Node, predicate: Predicate, object: Value) -> bool {
        triples
            .iter()
            .any(|t| t.subject == subject && t.predicate == predicate && t.object == object)
    }

    const ALL_CULTURES: [Culture; 4] = [
        Culture::Nomad,
        Culture::Farmer,
        Culture::Hunter,
        Culture::Gatherer,
    ];

    #[test]
    fn gatherer_knows_berrybush_produces_berry() {
        let triples = create_cultural_knowledge(Culture::Gatherer);
        assert!(contains(
            &triples,
            Node::Concept(Concept::BerryBush),
            Predicate::Produces,
            Value::Item(Concept::Berry, 1),
        ));
    }

    #[test]
    fn nomad_does_not_know_berrybush_produces_berry() {
        let triples = create_cultural_knowledge(Culture::Nomad);
        assert!(!contains(
            &triples,
            Node::Concept(Concept::BerryBush),
            Predicate::Produces,
            Value::Item(Concept::Berry, 1),
        ));
    }

    #[test]
    fn farmer_does_not_know_berrybush_production() {
        let triples = create_cultural_knowledge(Culture::Farmer);
        let has_any = triples.iter().any(|t| {
            t.subject == Node::Concept(Concept::BerryBush) && t.predicate == Predicate::Produces
        });
        assert!(!has_any);
    }

    #[test]
    fn no_culture_redefines_woodlog_or_stonenode_production() {
        for culture in ALL_CULTURES {
            let triples = create_cultural_knowledge(culture);
            for producer in [Concept::WoodLog, Concept::StoneNode] {
                let has_any = triples.iter().any(|t| {
                    t.subject == Node::Concept(producer) && t.predicate == Predicate::Produces
                });
                assert!(!has_any, "{culture:?} re-seeds {producer:?} production");
            }
        }
    }

    #[test]
    fn no_culture_redefines_plant_entity_harvestable_trait() {
        for culture in ALL_CULTURES {
            let triples = create_cultural_knowledge(culture);
            for concept in [Concept::AppleTree, Concept::BerryBush] {
                assert!(
                    !contains(
                        &triples,
                        Node::Concept(concept),
                        Predicate::HasTrait,
                        Value::Concept(Concept::Harvestable),
                    ),
                    "{culture:?} re-seeds {concept:?} HasTrait Harvestable",
                );
            }
        }
    }

    #[test]
    fn ontology_has_universal_production_facts() {
        let ontology = setup_ontology();
        let pairs = [
            (Concept::WoodLog, Concept::Wood),
            (Concept::StoneNode, Concept::Stone),
        ];
        for (producer, product) in pairs {
            let present = ontology.triples.iter().any(|t| {
                t.subject == Node::Concept(producer)
                    && t.predicate == Predicate::Produces
                    && t.object == Value::Item(product, 1)
            });
            assert!(present, "ontology missing {producer:?} -> {product:?}");
        }
    }

    #[test]
    fn plant_entities_inherit_harvestable_from_ontology() {
        let ontology = setup_ontology();
        for concept in [Concept::AppleTree, Concept::BerryBush] {
            assert!(ontology.has_trait(concept, Concept::Harvestable));
        }
    }
}
