//! ItemSlots: universal storage primitive for agents, chests, furnaces, construction sites, equipment.
//!
//! Reads: Concept (item type vocabulary from knowledge), Ontology (trait membership for filters)
//! Writes: ItemSlots (deposit/extract/add/remove items), SimEvent (freshness decay transitions)
//! Upstream: action execution systems (deposit/extract on success), world entity spawning
//! Downstream: brain_system (slots influence action choices), belief_updater (syncs MindGraph beliefs)

use crate::agent::events::SimEventKind;
use crate::agent::mind::knowledge::{Concept, Ontology};
use bevy::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// THING PROPERTIES — Per-instance metadata
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Default, Reflect)]
pub struct ThingProperties {
    /// 1.0 = fresh, 0.0 = fully decayed. Only present on perishables.
    pub freshness: Option<f32>,

    /// 0.0–1.0 craftsmanship. Only present on crafted items.
    pub quality: Option<f32>,

    /// The entity that created or harvested this item.
    pub created_by: Option<Entity>,

    /// The tick at which this item was created or harvested.
    pub created_at: Option<u64>,
}

// ═══════════════════════════════════════════════════════════════════════════
// THING — A discrete world object with per-instance identity
// ═══════════════════════════════════════════════════════════════════════════

/// A discrete object with per-instance properties.
///
/// Replaces the old `ItemStack { concept, quantity }`. Three apples are now
/// three individual `Thing` values — each can have different freshness,
/// quality, or provenance.
///
/// Lives either in `ItemSlots` (carried/stored) or as a component on a world
/// entity (dropped on the ground, placed in the world).
#[derive(Clone, Debug, Reflect)]
pub struct Thing {
    pub concept: Concept,
    pub properties: ThingProperties,
}

impl Thing {
    /// Create a fresh Thing with no special properties.
    pub fn new(concept: Concept) -> Self {
        Self {
            concept,
            properties: ThingProperties::default(),
        }
    }

    /// Create a perishable Thing with freshness initialized to 1.0.
    pub fn fresh(concept: Concept, tick: u64) -> Self {
        Self {
            concept,
            properties: ThingProperties {
                freshness: Some(1.0),
                created_at: Some(tick),
                ..Default::default()
            },
        }
    }

    /// Create a perishable Thing harvested by a specific agent.
    pub fn harvested(concept: Concept, tick: u64, harvester: Entity) -> Self {
        Self {
            concept,
            properties: ThingProperties {
                freshness: Some(1.0),
                created_at: Some(tick),
                created_by: Some(harvester),
                ..Default::default()
            },
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PERISHABILITY — Which concepts decay and how fast
// ═══════════════════════════════════════════════════════════════════════════

/// Returns the freshness lost per 100-tick decay event for this concept,
/// or `None` if the concept does not decay.
///
/// Decay rates (100-tick event):
/// - Apple:       0.005  → ~200 events → 20 000 ticks to fully rot
/// - Berry:       0.020  → ~50 events  → 5 000 ticks to fully rot
/// - Meat:        0.010  → ~100 events → 10 000 ticks to fully rot
/// - CookedMeat:  0.005  → half the rate of raw meat (cooking preserves)
pub fn perishable_decay_rate(concept: Concept) -> Option<f32> {
    match concept {
        Concept::Apple => Some(0.005),
        Concept::Berry => Some(0.020),
        Concept::Meat => Some(0.010),
        Concept::CookedMeat => Some(0.005),
        _ => None,
    }
}

/// Returns the concept this item should become when its freshness hits 0,
/// or `None` if there is no rotten variant.
pub fn rotten_variant(concept: Concept) -> Option<Concept> {
    match concept {
        Concept::Apple => Some(Concept::RottenApple),
        Concept::Berry => Some(Concept::RottenBerry),
        _ => None,
    }
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
    /// Maximum number of items this slot holds. `None` = unlimited.
    pub capacity: Option<u32>,
    pub contents: Vec<Thing>,
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

    /// Fuel slot: accepts only the specified material, deposit-only (no extraction).
    /// Used for campfires, furnaces, and other entities that burn consumables.
    pub fn fuel(material: Concept, capacity: u32) -> Self {
        Self {
            role: SlotRole::Fuel,
            filter: SlotFilter::OnlyConcepts(vec![material]),
            capacity: Some(capacity),
            contents: Vec::new(),
            deposit_access: Access::Public,
            extract_access: Access::None,
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

    /// Total number of items currently in this slot.
    pub fn total_quantity(&self) -> u32 {
        self.contents.len() as u32
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

    /// Check whether a deposit of `quantity` items is possible (filter + capacity + access).
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
///
/// Items are stored as individual `Thing` values — three apples are three
/// Things, each with their own freshness, quality, and provenance.
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

    /// Add `quantity` Things with default properties to the first Free slot,
    /// bypassing filter and access checks.
    /// Use this for trusted internal writes (spawn, test setup).
    /// Use [`deposit`] for access-controlled writes from external agents.
    pub fn add(&mut self, concept: Concept, quantity: u32) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.role == SlotRole::Free) {
            for _ in 0..quantity {
                slot.contents.push(Thing::new(concept));
            }
        }
    }

    /// Add a Thing with full properties to the first Free slot,
    /// bypassing filter and access checks.
    /// Use this for harvest completion and other property-preserving writes.
    pub fn add_thing(&mut self, thing: Thing) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.role == SlotRole::Free) {
            slot.contents.push(thing);
        }
    }

    /// Remove `quantity` Things of `concept` from whichever slot holds them.
    /// Returns `true` if the full quantity was removed.
    /// Properties are discarded — use [`remove_thing`] to preserve them.
    pub fn remove(&mut self, concept: Concept, quantity: u32) -> bool {
        let mut remaining = quantity;
        for slot in &mut self.slots {
            while remaining > 0 {
                let pos = slot.contents.iter().position(|t| t.concept == concept);
                match pos {
                    Some(i) => {
                        slot.contents.remove(i);
                        remaining -= 1;
                    }
                    None => break,
                }
            }
            if remaining == 0 {
                return true;
            }
        }
        // Partial remove happened — but if we couldn't remove the full qty,
        // return false. We already removed `quantity - remaining` things.
        remaining == 0
    }

    /// Remove one Thing of `concept` and return it with its properties intact.
    /// Returns `None` if no such Thing exists.
    /// Use for transfers where the item's metadata must be preserved (deposit, take, eat).
    pub fn remove_thing(&mut self, concept: Concept) -> Option<Thing> {
        for slot in &mut self.slots {
            if slot.extract_access == Access::None {
                continue;
            }
            if let Some(pos) = slot.contents.iter().position(|t| t.concept == concept) {
                return Some(slot.contents.remove(pos));
            }
        }
        None
    }

    /// Remove one Thing of `concept` from any slot, ignoring extract_access.
    /// Use for trusted internal operations (build, consume, world transitions).
    pub fn remove_thing_unchecked(&mut self, concept: Concept) -> Option<Thing> {
        for slot in &mut self.slots {
            if let Some(pos) = slot.contents.iter().position(|t| t.concept == concept) {
                return Some(slot.contents.remove(pos));
            }
        }
        None
    }

    /// Total count of Things with `concept` across all slots.
    pub fn count(&self, concept: Concept) -> u32 {
        self.slots
            .iter()
            .flat_map(|s| s.contents.iter())
            .filter(|t| t.concept == concept)
            .count() as u32
    }

    /// Returns `true` if any slot holds at least one Thing with `concept`.
    pub fn has(&self, concept: Concept) -> bool {
        self.count(concept) > 0
    }

    /// Returns the first edible concept found across all slots.
    pub fn first_edible(&self, ontology: &Ontology) -> Option<Concept> {
        for slot in &self.slots {
            for thing in &slot.contents {
                if ontology.has_trait(thing.concept, Concept::Edible) {
                    return Some(thing.concept);
                }
            }
        }
        None
    }

    /// Returns `true` if any slot holds an edible item.
    pub fn has_edible(&self, ontology: &Ontology) -> bool {
        self.first_edible(ontology).is_some()
    }

    /// Iterate over every Thing across all slots.
    pub fn all_items(&self) -> impl Iterator<Item = &Thing> {
        self.slots.iter().flat_map(|s| s.contents.iter())
    }

    /// Attempt to deposit `quantity` Things with default properties into the first
    /// slot that accepts the concept, respecting filter, capacity, and deposit access.
    /// Returns `true` on success, `false` if every slot rejects.
    /// Use [`deposit_thing`] when you need to preserve item properties.
    pub fn deposit(
        &mut self,
        concept: Concept,
        quantity: u32,
        ontology: Option<&Ontology>,
    ) -> bool {
        for slot in &mut self.slots {
            if slot.can_deposit(concept, quantity, ontology) {
                for _ in 0..quantity {
                    slot.contents.push(Thing::new(concept));
                }
                return true;
            }
        }
        false
    }

    /// Attempt to deposit a specific Thing (with its properties) into the first
    /// slot that accepts its concept. Returns `true` on success.
    pub fn deposit_thing(&mut self, thing: Thing, ontology: Option<&Ontology>) -> bool {
        for slot in &mut self.slots {
            if slot.can_deposit(thing.concept, 1, ontology) {
                slot.contents.push(thing);
                return true;
            }
        }
        false
    }

    /// Returns `true` if no slot blocks extraction of `concept`
    /// (Construction slots have `extract_access: None`).
    pub fn can_extract(&self, concept: Concept) -> bool {
        for slot in &self.slots {
            if slot.contents.iter().any(|t| t.concept == concept) {
                return slot.extract_access != Access::None;
            }
        }
        true
    }

    /// Attempt to remove `quantity` Things of `concept` from the first slot
    /// that holds them AND permits extraction. Access-checked dual of [`deposit`].
    /// Returns `true` on success. Use [`extract_thing`] to preserve properties.
    pub fn extract(&mut self, concept: Concept, quantity: u32) -> bool {
        for slot in &mut self.slots {
            if slot.extract_access == Access::None {
                continue;
            }
            let available = slot
                .contents
                .iter()
                .filter(|t| t.concept == concept)
                .count() as u32;
            if available >= quantity {
                let mut removed = 0u32;
                slot.contents.retain(|t| {
                    if t.concept == concept && removed < quantity {
                        removed += 1;
                        false
                    } else {
                        true
                    }
                });
                return removed == quantity;
            }
        }
        false
    }

    /// Attempt to extract one Thing of `concept`, returning it with properties.
    /// Returns `None` if no extractable Thing of that concept exists.
    pub fn extract_thing(&mut self, concept: Concept) -> Option<Thing> {
        self.remove_thing(concept)
    }

    /// Returns a map of concept → count across all slots.
    /// Used for UI display and belief assertions where per-instance properties
    /// don't matter, only the total count per type.
    pub fn group_by_concept(&self) -> std::collections::HashMap<Concept, u32> {
        let mut counts = std::collections::HashMap::new();
        for thing in self.all_items() {
            *counts.entry(thing.concept).or_default() += 1;
        }
        counts
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// FRESHNESS DECAY SYSTEM
// ═══════════════════════════════════════════════════════════════════════════

/// Runs every 100 ticks. Decrements freshness on perishable Things in all
/// `ItemSlots`. When freshness reaches 0, the concept changes to its rotten
/// variant (e.g. Apple → RottenApple) and a `SimEvent::ItemSpoiled` fires.
///
/// Items with `freshness = None` are skipped — that sentinel marks "still
/// attached to the source" (berries on a bush, apples on a tree). A berry
/// only starts aging once an agent plucks it via Harvest's on_complete,
/// which replaces the sourceless `Thing::new` with a `Thing::fresh` that
/// carries `freshness = Some(1.0)` and a `created_at` timestamp. Before
/// this gate, the old `get_or_insert(1.0)` path rotted berries in place
/// on the bush: agents would wander up to a visibly-stocked bush and
/// harvest a handful of RottenBerry, fail to eat them, and starve (#416).
pub fn freshness_decay_system(
    mut query: Query<(Entity, &mut ItemSlots)>,
    tick: Res<crate::core::tick::TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    for (owner, mut slots) in &mut query {
        for slot in &mut slots.slots {
            for thing in &mut slot.contents {
                let Some(rate) = perishable_decay_rate(thing.concept) else {
                    continue;
                };
                let Some(freshness) = thing.properties.freshness.as_mut() else {
                    continue;
                };
                *freshness = (*freshness - rate).max(0.0);
                if *freshness == 0.0
                    && let Some(rotten) = rotten_variant(thing.concept)
                {
                    let from = thing.concept;
                    thing.concept = rotten;
                    thing.properties.freshness = None;
                    sim_events.write(crate::agent::events::SimEvent::single(
                        tick.current,
                        owner,
                        SimEventKind::ItemSpoiled {
                            agent: owner,
                            from,
                            to: rotten,
                        },
                    ));
                }
            }
        }
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
        slot_a.contents.push(Thing::new(Concept::Apple));
        slot_a.contents.push(Thing::new(Concept::Apple));
        let mut slot_b = Slot::free();
        slot_b.contents.push(Thing::new(Concept::Berry));
        slot_b.contents.push(Thing::new(Concept::Berry));
        slot_b.contents.push(Thing::new(Concept::Berry));
        let slots = ItemSlots {
            slots: vec![slot_a, slot_b],
        };

        let total = slots.all_items().count();
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

    // -----------------------------------------------------------------------
    // Thing — per-instance properties
    // -----------------------------------------------------------------------

    #[test]
    fn add_thing_preserves_freshness() {
        let mut slots = ItemSlots::agent_carry();
        let apple = Thing {
            concept: Concept::Apple,
            properties: ThingProperties {
                freshness: Some(0.7),
                created_at: Some(42),
                ..Default::default()
            },
        };
        slots.add_thing(apple);
        let stored = slots.all_items().next().unwrap();
        assert_eq!(stored.properties.freshness, Some(0.7));
        assert_eq!(stored.properties.created_at, Some(42));
    }

    #[test]
    fn remove_thing_returns_properties_intact() {
        let mut slots = ItemSlots::agent_carry();
        slots.add_thing(Thing {
            concept: Concept::Apple,
            properties: ThingProperties {
                freshness: Some(0.5),
                quality: Some(0.8),
                ..Default::default()
            },
        });
        let removed = slots.remove_thing(Concept::Apple).unwrap();
        assert_eq!(removed.properties.freshness, Some(0.5));
        assert_eq!(removed.properties.quality, Some(0.8));
    }

    #[test]
    fn deposit_thing_preserves_properties_in_target() {
        let mut agent = ItemSlots::agent_carry();
        let mut chest = ItemSlots {
            slots: vec![Slot {
                role: SlotRole::Free,
                filter: SlotFilter::Any,
                capacity: None,
                contents: Vec::new(),
                deposit_access: Access::Public,
                extract_access: Access::Public,
            }],
        };

        agent.add_thing(Thing {
            concept: Concept::Apple,
            properties: ThingProperties {
                freshness: Some(0.9),
                created_at: Some(100),
                ..Default::default()
            },
        });

        let thing = agent.remove_thing(Concept::Apple).unwrap();
        assert!(chest.deposit_thing(thing, None));

        let in_chest = chest.all_items().next().unwrap();
        assert_eq!(in_chest.properties.freshness, Some(0.9));
        assert_eq!(in_chest.properties.created_at, Some(100));
    }

    #[test]
    fn two_apples_with_different_freshness_are_independent() {
        let mut slots = ItemSlots::agent_carry();
        slots.add_thing(Thing {
            concept: Concept::Apple,
            properties: ThingProperties {
                freshness: Some(1.0),
                ..Default::default()
            },
        });
        slots.add_thing(Thing {
            concept: Concept::Apple,
            properties: ThingProperties {
                freshness: Some(0.3),
                ..Default::default()
            },
        });
        // Both count as Apple
        assert_eq!(slots.count(Concept::Apple), 2);
        // But they are distinct objects
        let items: Vec<_> = slots.all_items().collect();
        assert_eq!(items.len(), 2);
        let fresh_vals: Vec<Option<f32>> = items.iter().map(|t| t.properties.freshness).collect();
        assert!(fresh_vals.contains(&Some(1.0)));
        assert!(fresh_vals.contains(&Some(0.3)));
    }

    // -----------------------------------------------------------------------
    // Freshness decay
    // -----------------------------------------------------------------------

    #[test]
    fn freshness_decays_after_decay_events() {
        let mut slots = ItemSlots::agent_carry();
        slots.add_thing(Thing::fresh(Concept::Apple, 0));

        // Simulate 10 decay events (runs every 100 ticks)
        for _ in 0..10 {
            for slot in &mut slots.slots {
                for thing in &mut slot.contents {
                    if let Some(rate) = perishable_decay_rate(thing.concept) {
                        let freshness = thing.properties.freshness.get_or_insert(1.0);
                        *freshness = (*freshness - rate).max(0.0);
                    }
                }
            }
        }

        let item = slots.all_items().next().unwrap();
        let freshness = item.properties.freshness.unwrap();
        // After 10 events at 0.005/event: 1.0 - 0.05 = 0.95
        assert!((freshness - 0.95).abs() < 0.001);
    }

    #[test]
    fn freshness_zero_transitions_concept_to_rotten_variant() {
        let mut slots = ItemSlots::agent_carry();
        slots.add_thing(Thing {
            concept: Concept::Apple,
            properties: ThingProperties {
                freshness: Some(0.001),
                ..Default::default()
            },
        });

        // One more decay event to push freshness to 0
        for slot in &mut slots.slots {
            for thing in &mut slot.contents {
                if let Some(rate) = perishable_decay_rate(thing.concept) {
                    let freshness = thing.properties.freshness.get_or_insert(1.0);
                    *freshness = (*freshness - rate).max(0.0);
                    if *freshness == 0.0
                        && let Some(rotten) = rotten_variant(thing.concept)
                    {
                        thing.concept = rotten;
                        thing.properties.freshness = None;
                    }
                }
            }
        }

        let item = slots.all_items().next().unwrap();
        assert_eq!(item.concept, Concept::RottenApple);
        assert_eq!(item.properties.freshness, None);
    }
}
