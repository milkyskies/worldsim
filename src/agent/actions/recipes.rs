//! Auto-derivation of cultural recipe triples from [`ActionDefinition::recipe`].
//!
//! Build-style actions declare a `recipe: Some(Recipe { ... })` on their
//! static definition. Culture seeding walks the registry and emits the
//! `(concept, Requires, Item)` / `(concept, Provides, trait)` /
//! `(concept, BuildTime, ticks)` triples automatically — no second source
//! of truth for the same data to drift from.

use super::definition::Recipe;
use super::registry::ActionRegistry;
use crate::agent::mind::knowledge::{
    MemoryType, Metadata, Node, Predicate, Quantity, Source, Triple, Value,
};

/// Build the cultural recipe triples sourced from every registered action's
/// `recipe` field. Returns triples tagged with [`Source::Cultural`], ready
/// to fold into an agent's starting MindGraph.
pub fn derive_recipe_triples(registry: &ActionRegistry) -> Vec<Triple> {
    let mut triples = Vec::new();
    for recipe in registry.recipes() {
        extend_with_recipe(&mut triples, recipe);
    }
    triples
}

fn extend_with_recipe(triples: &mut Vec<Triple>, recipe: &Recipe) {
    let meta = cultural_metadata();
    for (material, quantity) in recipe.requirements {
        triples.push(Triple::with_meta(
            Node::Concept(recipe.concept),
            Predicate::Requires,
            Value::Item(*material, *quantity),
            meta.clone(),
        ));
    }
    for trait_concept in recipe.provides {
        triples.push(Triple::with_meta(
            Node::Concept(recipe.concept),
            Predicate::Provides,
            Value::Concept(*trait_concept),
            meta.clone(),
        ));
    }
    triples.push(Triple::with_meta(
        Node::Concept(recipe.concept),
        Predicate::BuildTime,
        Value::Quantity(Quantity::Exact(recipe.build_time_ticks as f32)),
        meta,
    ));
}

fn cultural_metadata() -> Metadata {
    Metadata {
        source: Source::Cultural,
        memory_type: MemoryType::Cultural,
        timestamp: 0,
        confidence: 0.8,
        salience: 0.5,
        ..Default::default()
    }
}
