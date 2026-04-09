//! ItemSlots: universal storage primitive for agents, chests, furnaces, construction sites, equipment.
//!
//! Reads: Concept (item type vocabulary from knowledge), Ontology (trait membership for filters)
//! Writes: ItemSlots (deposit/extract/add/remove items)
//! Upstream: action execution systems (deposit/extract on success), world entity spawning
//! Downstream: brain_system (slots influence action choices), belief_updater (syncs MindGraph beliefs)

use crate::agent::mind::knowledge::{Concept, Ontology};
use bevy::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// ITEM STACK
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Reflect)]
pub struct ItemStack {
    pub concept: Concept,
    pub quantity: u32,
}

// ═══════════════════════════════════════════════════════════════════════════
// SLOT ROLE
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Reflect, PartialEq)]
pub enum SlotRole {
    /// Generic storage: agent carry, chests, ground piles
    Free,
    /// Furnace / crafting input slot
    Input { recipe_position: u8 },
    /// Furnace / crafting output slot — results are placed here
    Output,
    /// Fuel source for processing entities (furnaces, future torches)
    Fuel,
    /// Equipment slot tied to a body part (future wearables)
    Equipment { part: BodyPart },
    /// Construction site slot — committed once filled, cannot be extracted
    Construction { material: Concept, required: u32 },
}

// ═══════════════════════════════════════════════════════════════════════════
// BODY PART — used by Equipment slots
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Reflect, PartialEq)]
pub enum BodyPart {
    Head,
    Torso,
    Hands,
    Feet,
}

// ═══════════════════════════════════════════════════════════════════════════
// SLOT FILTER
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Reflect)]
pub enum SlotFilter {
    /// Accept anything
    Any,
    /// Accept only items matching one of the listed concepts
    OnlyConcepts(Vec<Concept>),
    /// Accept items that have the given trait in the ontology
    HasTrait(Concept),
}

// ═══════════════════════════════════════════════════════════════════════════
// ACCESS
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Reflect, PartialEq)]
pub enum Access {
    /// Any entity in interaction range may access
    Public,
    /// Only the owning entity may access
    OwnerOnly,
    /// Future: specific entity whitelist
    Whitelist(Vec<Entity>),
    /// Sealed — access is not permitted (e.g. Construction extract)
    None,
}

// ═══════════════════════════════════════════════════════════════════════════
// SLOT
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Reflect)]
pub struct Slot {
    pub role: SlotRole,
    pub filter: SlotFilter,
    /// Maximum total quantity across all stacks. `None` = unlimited.
    pub capacity: Option<u32>,
    pub contents: Vec<ItemStack>,
    pub deposit_access: Access,
    pub extract_access: Access,
}

impl Slot {
    /// Agent carry slot: unlimited, accepts anything, owner-only extract.
    pub fn free() -> Self {
        Self {
            role: SlotRole::Free,
            filter: SlotFilter::Any,
            capacity: None,
            contents: Vec::new(),
            deposit_access: Access::Public,
            extract_access: Access::OwnerOnly,
        }
    }

    /// Construction site slot: accepts only the specified material, write-only.
    pub fn construction(material: Concept, required: u32) -> Self {
        Self {
            role: SlotRole::Construction { material, required },
            filter: SlotFilter::OnlyConcepts(vec![material]),
            capacity: Some(required),
            contents: Vec::new(),
            deposit_access: Access::Public,
            extract_access: Access::None,
        }
    }

    /// Total quantity held across all stacks.
    pub fn total_quantity(&self) -> u32 {
        self.contents.iter().map(|s| s.quantity).sum()
    }

    /// Check whether this slot's filter accepts the given concept.
    pub fn accepts(&self, concept: Concept, ontology: Option<&Ontology>) -> bool {
        match &self.filter {
            SlotFilter::Any => true,
            SlotFilter::OnlyConcepts(concepts) => concepts.contains(&concept),
            SlotFilter::HasTrait(trait_concept) => {
                ontology.is_some_and(|onto| onto.has_trait(concept, *trait_concept))
            }
        }
    }

    /// Check whether a deposit of `quantity` units is possible (filter + capacity + access).
    pub fn can_deposit(
        &self,
        concept: Concept,
        quantity: u32,
        ontology: Option<&Ontology>,
    ) -> bool {
        if self.deposit_access == Access::None {
            return false;
        }
        if !self.accepts(concept, ontology) {
            return false;
        }
        match self.capacity {
            Some(cap) => self.total_quantity() + quantity <= cap,
            None => true,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ITEM SLOTS
// ═══════════════════════════════════════════════════════════════════════════

/// Universal storage component. Replaces the agent-only `Inventory`.
///
/// Each entity that holds items uses this component with different slot configurations:
/// - Agents: one `Free` slot (unlimited carry)
/// - Chests: N `Free` slots, both accesses `Public`
/// - Furnaces: `Fuel` + `Input` + `Output` slots with process component
/// - Construction sites: per-material `Construction` slots
#[derive(Component, Reflect, Default, Clone)]
#[reflect(Component)]
pub struct ItemSlots {
    pub slots: Vec<Slot>,
}

impl ItemSlots {
    /// Create an agent carry: one `Free` slot with unlimited capacity.
    pub fn agent_carry() -> Self {
        Self {
            slots: vec![Slot::free()],
        }
    }

    // -----------------------------------------------------------------------
    // Convenience helpers (backward-compatible with old Inventory API)
    // All helpers operate on the Free slot(s) — appropriate for agents,
    // resource nodes, and any entity configured with Free slots.
    // -----------------------------------------------------------------------

    /// Add `quantity` of `concept` to the first Free slot, bypassing filter and access checks.
    /// Use this for trusted internal writes (spawn, harvest completion, test setup).
    /// Use [`deposit`] for access-controlled writes from external agents.
    pub fn add(&mut self, concept: Concept, quantity: u32) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.role == SlotRole::Free) {
            if let Some(stack) = slot.contents.iter_mut().find(|s| s.concept == concept) {
                stack.quantity += quantity;
            } else {
                slot.contents.push(ItemStack { concept, quantity });
            }
        }
    }

    /// Remove `quantity` of `concept` from whichever slot holds it.
    /// Returns `true` if the removal succeeded.
    pub fn remove(&mut self, concept: Concept, quantity: u32) -> bool {
        for slot in &mut self.slots {
            if let Some(stack) = slot.contents.iter_mut().find(|s| s.concept == concept)
                && stack.quantity >= quantity
            {
                stack.quantity -= quantity;
                if stack.quantity == 0 {
                    slot.contents.retain(|s| s.concept != concept);
                }
                return true;
            }
        }
        false
    }

    /// Total quantity of `concept` across all slots.
    pub fn count(&self, concept: Concept) -> u32 {
        self.slots
            .iter()
            .flat_map(|s| s.contents.iter())
            .filter(|stack| stack.concept == concept)
            .map(|stack| stack.quantity)
            .sum()
    }

    /// Returns `true` if any slot holds at least one unit of `concept`.
    pub fn has(&self, concept: Concept) -> bool {
        self.count(concept) > 0
    }

    /// Returns the first edible concept found across all slots.
    pub fn first_edible(&self, ontology: &Ontology) -> Option<Concept> {
        for slot in &self.slots {
            for stack in &slot.contents {
                if stack.quantity > 0 && ontology.has_trait(stack.concept, Concept::Edible) {
                    return Some(stack.concept);
                }
            }
        }
        None
    }

    /// Returns `true` if any slot holds an edible item.
    pub fn has_edible(&self, ontology: &Ontology) -> bool {
        self.first_edible(ontology).is_some()
    }

    /// Iterate over every item stack across all slots.
    pub fn all_items(&self) -> impl Iterator<Item = &ItemStack> {
        self.slots.iter().flat_map(|s| s.contents.iter())
    }

    /// Attempt to deposit `quantity` of `concept` into the first slot that accepts it,
    /// respecting filter, capacity, and deposit access rules.
    /// Returns `true` on success, `false` if every slot rejects.
    /// Use this for externally-initiated transfers (Deposit action, trade).
    /// Use [`add`] for trusted internal writes that bypass slot rules.
    pub fn deposit(
        &mut self,
        concept: Concept,
        quantity: u32,
        ontology: Option<&Ontology>,
    ) -> bool {
        for slot in &mut self.slots {
            if slot.can_deposit(concept, quantity, ontology) {
                if let Some(stack) = slot.contents.iter_mut().find(|s| s.concept == concept) {
                    stack.quantity += quantity;
                } else {
                    slot.contents.push(ItemStack { concept, quantity });
                }
                return true;
            }
        }
        false
    }

    /// Returns `true` if any slot has `extract_access == Access::None` for `concept`.
    /// Used to enforce write-only construction slots.
    pub fn can_extract(&self, concept: Concept) -> bool {
        for slot in &self.slots {
            if slot.contents.iter().any(|s| s.concept == concept) {
                return slot.extract_access != Access::None;
            }
        }
        true // no slot holds it, nothing to block
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Agent carry (Free slot helpers)
    // -----------------------------------------------------------------------

    #[test]
    fn agent_carry_add_count_returns_correct_quantity() {
        let mut slots = ItemSlots::agent_carry();
        slots.add(Concept::Apple, 3);
        assert_eq!(slots.count(Concept::Apple), 3);
    }

    #[test]
    fn agent_carry_add_multiple_concepts() {
        let mut slots = ItemSlots::agent_carry();
        slots.add(Concept::Apple, 2);
        slots.add(Concept::Berry, 5);
        assert_eq!(slots.count(Concept::Apple), 2);
        assert_eq!(slots.count(Concept::Berry), 5);
    }

    #[test]
    fn agent_carry_stacks_same_concept() {
        let mut slots = ItemSlots::agent_carry();
        slots.add(Concept::Apple, 1);
        slots.add(Concept::Apple, 2);
        assert_eq!(slots.count(Concept::Apple), 3);
    }

    #[test]
    fn agent_carry_remove_success_returns_true() {
        let mut slots = ItemSlots::agent_carry();
        slots.add(Concept::Apple, 3);
        assert!(slots.remove(Concept::Apple, 2));
        assert_eq!(slots.count(Concept::Apple), 1);
    }

    #[test]
    fn agent_carry_remove_clears_zero_quantity_stacks() {
        let mut slots = ItemSlots::agent_carry();
        slots.add(Concept::Apple, 1);
        slots.remove(Concept::Apple, 1);
        assert_eq!(slots.count(Concept::Apple), 0);
        assert!(!slots.has(Concept::Apple));
    }

    #[test]
    fn agent_carry_remove_insufficient_returns_false() {
        let mut slots = ItemSlots::agent_carry();
        slots.add(Concept::Apple, 2);
        assert!(!slots.remove(Concept::Apple, 5));
        assert_eq!(slots.count(Concept::Apple), 2);
    }

    // -----------------------------------------------------------------------
    // Slot filter rejection
    // -----------------------------------------------------------------------

    #[test]
    fn slot_filter_rejects_wrong_concept() {
        let slot = Slot {
            role: SlotRole::Free,
            filter: SlotFilter::OnlyConcepts(vec![Concept::Wood]),
            capacity: None,
            contents: Vec::new(),
            deposit_access: Access::Public,
            extract_access: Access::Public,
        };
        assert!(!slot.accepts(Concept::Stone, None));
        assert!(slot.accepts(Concept::Wood, None));
    }

    #[test]
    fn deposit_into_filtered_slot_fails_for_wrong_concept() {
        let mut slots = ItemSlots {
            slots: vec![Slot {
                role: SlotRole::Free,
                filter: SlotFilter::OnlyConcepts(vec![Concept::Wood]),
                capacity: None,
                contents: Vec::new(),
                deposit_access: Access::Public,
                extract_access: Access::Public,
            }],
        };
        assert!(!slots.deposit(Concept::Stone, 1, None));
        assert_eq!(slots.count(Concept::Stone), 0);
    }

    #[test]
    fn deposit_into_filtered_slot_succeeds_for_correct_concept() {
        let mut slots = ItemSlots {
            slots: vec![Slot {
                role: SlotRole::Free,
                filter: SlotFilter::OnlyConcepts(vec![Concept::Wood]),
                capacity: None,
                contents: Vec::new(),
                deposit_access: Access::Public,
                extract_access: Access::Public,
            }],
        };
        assert!(slots.deposit(Concept::Wood, 3, None));
        assert_eq!(slots.count(Concept::Wood), 3);
    }

    // -----------------------------------------------------------------------
    // Capacity enforcement
    // -----------------------------------------------------------------------

    #[test]
    fn capacity_slot_rejects_overflow() {
        let mut slots = ItemSlots {
            slots: vec![Slot {
                role: SlotRole::Free,
                filter: SlotFilter::Any,
                capacity: Some(5),
                contents: Vec::new(),
                deposit_access: Access::Public,
                extract_access: Access::Public,
            }],
        };
        assert!(slots.deposit(Concept::Apple, 5, None));
        assert!(!slots.deposit(Concept::Apple, 1, None));
        assert_eq!(slots.count(Concept::Apple), 5);
    }

    #[test]
    fn capacity_slot_accepts_exact_maximum() {
        let mut slots = ItemSlots {
            slots: vec![Slot {
                role: SlotRole::Free,
                filter: SlotFilter::Any,
                capacity: Some(3),
                contents: Vec::new(),
                deposit_access: Access::Public,
                extract_access: Access::Public,
            }],
        };
        assert!(slots.deposit(Concept::Berry, 3, None));
        assert_eq!(slots.count(Concept::Berry), 3);
    }

    // -----------------------------------------------------------------------
    // Multiple slots
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_free_slots_each_hold_different_items() {
        let mut slots = ItemSlots {
            slots: vec![Slot::free(), Slot::free(), Slot::free()],
        };
        // add() always targets the first Free slot — so all items land there,
        // but the entity has three slots available for future deposit routing.
        slots.add(Concept::Apple, 1);
        slots.add(Concept::Berry, 1);
        slots.add(Concept::Wood, 1);
        assert_eq!(slots.count(Concept::Apple), 1);
        assert_eq!(slots.count(Concept::Berry), 1);
        assert_eq!(slots.count(Concept::Wood), 1);
    }

    #[test]
    fn all_items_iterates_across_all_slots() {
        let mut slot_a = Slot::free();
        slot_a.contents.push(ItemStack {
            concept: Concept::Apple,
            quantity: 2,
        });
        let mut slot_b = Slot::free();
        slot_b.contents.push(ItemStack {
            concept: Concept::Berry,
            quantity: 3,
        });
        let slots = ItemSlots {
            slots: vec![slot_a, slot_b],
        };

        let total: u32 = slots.all_items().map(|s| s.quantity).sum();
        assert_eq!(total, 5);
    }

    // -----------------------------------------------------------------------
    // Construction slot — write-only, sealed extract
    // -----------------------------------------------------------------------

    #[test]
    fn construction_slot_rejects_extract() {
        let mut slots = ItemSlots {
            slots: vec![Slot::construction(Concept::Wood, 3)],
        };
        slots.deposit(Concept::Wood, 2, None);
        assert!(!slots.can_extract(Concept::Wood));
    }

    #[test]
    fn construction_slot_accepts_matching_material() {
        let mut slots = ItemSlots {
            slots: vec![Slot::construction(Concept::Wood, 3)],
        };
        assert!(slots.deposit(Concept::Wood, 3, None));
        assert_eq!(slots.count(Concept::Wood), 3);
    }

    #[test]
    fn construction_slot_rejects_wrong_material() {
        let mut slots = ItemSlots {
            slots: vec![Slot::construction(Concept::Wood, 3)],
        };
        assert!(!slots.deposit(Concept::Stone, 1, None));
        assert_eq!(slots.count(Concept::Stone), 0);
    }

    #[test]
    fn construction_slot_respects_capacity() {
        let mut slots = ItemSlots {
            slots: vec![Slot::construction(Concept::Wood, 3)],
        };
        assert!(slots.deposit(Concept::Wood, 3, None));
        assert!(!slots.deposit(Concept::Wood, 1, None));
    }

    // -----------------------------------------------------------------------
    // Access::None blocks deposit
    // -----------------------------------------------------------------------

    #[test]
    fn deposit_access_none_always_rejects() {
        let mut slots = ItemSlots {
            slots: vec![Slot {
                role: SlotRole::Output,
                filter: SlotFilter::Any,
                capacity: None,
                contents: Vec::new(),
                deposit_access: Access::None,
                extract_access: Access::Public,
            }],
        };
        assert!(!slots.deposit(Concept::Apple, 1, None));
    }

    // -----------------------------------------------------------------------
    // has / has_edible (ontology-dependent methods are tested via has())
    // -----------------------------------------------------------------------

    #[test]
    fn has_returns_true_when_item_present() {
        let mut slots = ItemSlots::agent_carry();
        slots.add(Concept::Apple, 1);
        assert!(slots.has(Concept::Apple));
        assert!(!slots.has(Concept::Stone));
    }

    #[test]
    fn empty_agent_carry_has_no_items() {
        let slots = ItemSlots::agent_carry();
        assert_eq!(slots.count(Concept::Apple), 0);
        assert!(!slots.has(Concept::Apple));
    }
}
