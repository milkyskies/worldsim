use crate::agent::mind::knowledge::Concept;
use bevy::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// ENTITY TYPE — Universal type marker using Concept
// ═══════════════════════════════════════════════════════════════════════════

/// Marks what type of thing an entity IS. Uses Concept as the shared vocabulary
/// between ECS (reality) and MindGraph (beliefs).
#[derive(Component, Reflect, Default, Clone, Copy, Debug)]
#[reflect(Component)]
pub struct EntityType(pub Concept);

// ═══════════════════════════════════════════════════════════════════════════
// INVENTORY — Uses Concept directly for items
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct Inventory {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Reflect)]
pub struct Item {
    pub concept: Concept, // What type of item (Apple, Stick, Stone, etc.)
    pub quantity: u32,
}

impl Inventory {
    pub fn add(&mut self, concept: Concept, quantity: u32) {
        if let Some(existing) = self.items.iter_mut().find(|i| i.concept == concept) {
            existing.quantity += quantity;
        } else {
            self.items.push(Item { concept, quantity });
        }
    }

    pub fn remove(&mut self, concept: Concept, quantity: u32) -> bool {
        if let Some(existing) = self.items.iter_mut().find(|i| i.concept == concept)
            && existing.quantity >= quantity {
                existing.quantity -= quantity;
                if existing.quantity == 0 {
                    self.items.retain(|i| i.concept != concept);
                }
                return true;
            }
        false
    }

    pub fn count(&self, concept: Concept) -> u32 {
        self.items
            .iter()
            .find(|i| i.concept == concept)
            .map(|i| i.quantity)
            .unwrap_or(0)
    }

    /// Check if inventory has any of a specific item
    pub fn has(&self, concept: Concept) -> bool {
        self.count(concept) > 0
    }

    /// Returns the first edible food concept found in the inventory, if any.
    /// Queries the Ontology to check if items have the Edible trait.
    pub fn first_edible(&self, ontology: &crate::agent::mind::knowledge::Ontology) -> Option<Concept> {
        for item in &self.items {
            if item.quantity > 0 && ontology.has_trait(item.concept, Concept::Edible) {
                return Some(item.concept);
            }
        }
        None
    }

    /// Check if inventory has any edible food
    pub fn has_edible(&self, ontology: &crate::agent::mind::knowledge::Ontology) -> bool {
        self.first_edible(ontology).is_some()
    }
}
