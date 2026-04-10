//! Becomes: general substrate for entities that transform into other entities.
//!
//! Reads: Becomes (component), ItemSlots (for SlotsFilled trigger), Transform, TickCount
//! Writes: Despawns source entities, spawns target concept entities at the same position
//! Upstream: Build action (creates construction sites), future cooking/growing/decay systems
//! Downstream: Perception (observers see the transformation result on the next tick)
//!
//! ## World rule vs. agent belief
//!
//! `Becomes` is **world truth**. It fires regardless of who knows or believes what.
//! Agents reason about `Becomes` rules through their MindGraph using the
//! `Predicate::Becomes` triple, which perception writes when an agent sees an
//! entity that has a `Becomes` component. The planner consults beliefs, never
//! the world component directly.

use crate::agent::item_slots::{ItemSlots, SlotRole};
use crate::agent::mind::knowledge::Concept;
use crate::core::tick::TickCount;
use crate::world::spawn::{spawn_concept_entity, transform_concept_in_place};
use bevy::prelude::*;

/// World-truth component declaring "this entity will transform into `target`
/// when `trigger` fires". Lives on the world entity, NOT in any agent's mind.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct Becomes {
    /// The Concept the entity transforms into when `trigger` fires.
    pub target: Concept,
    /// The condition that drives the transformation.
    #[reflect(ignore)]
    pub trigger: BecomesTrigger,
    /// Tick at which this component was attached. Used by `AfterTicks` triggers
    /// to compute elapsed time. Set by the spawner; the system never mutates it.
    pub started_tick: u64,
    /// How the transformation manifests in the world.
    #[reflect(ignore)]
    pub mode: BecomesMode,
}

impl Default for Becomes {
    fn default() -> Self {
        Self {
            target: Concept::Thing,
            trigger: BecomesTrigger::SlotsFilled,
            started_tick: 0,
            mode: BecomesMode::Replace,
        }
    }
}

impl Becomes {
    pub fn new(target: Concept, trigger: BecomesTrigger, started_tick: u64) -> Self {
        Self {
            target,
            trigger,
            started_tick,
            mode: BecomesMode::Replace,
        }
    }

    /// Builder: switch this transformation to in-place mode. The substrate
    /// will morph the existing entity instead of despawning + respawning.
    pub fn in_place(mut self) -> Self {
        self.mode = BecomesMode::InPlace;
        self
    }
}

/// How a `Becomes` transformation manifests when its trigger fires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BecomesMode {
    /// Despawn the source entity and spawn a fresh entity of `target` at the
    /// same position. This is the right mode for things that genuinely have
    /// no prior identity worth preserving (construction sites filling into
    /// campfires).
    #[default]
    Replace,
    /// Morph the source entity into `target` in place, preserving its
    /// entity ID, Transform, Name, MindGraph, Body, etc. The right mode for
    /// transformations that need to keep referential identity — slain prey
    /// becoming corpses (so future episodic memory and relationship triples
    /// keep pointing at a meaningful entity).
    InPlace,
}

/// Condition that drives a `Becomes` transformation.
#[derive(Clone, Debug, Default)]
pub enum BecomesTrigger {
    /// Fires when every `Construction` slot on the entity has reached its
    /// `required` quantity. Entities with no Construction slots never fire
    /// via this trigger (you cannot be "filled" without expectations).
    /// Also the `Default` variant for `Reflect` derive purposes.
    #[default]
    SlotsFilled,
    /// Fires `n` ticks after the component was attached.
    ///
    /// TODO(#171): Migrate to `AfterTicks(Duration)` once the Duration distribution
    /// substrate exists. The `Duration` will sample at attach-time and store the
    /// resolved deadline; agent beliefs about timing will live separately as
    /// `TimeBelief` triples in the MindGraph. See #171's "Migration obligation"
    /// section.
    AfterTicks(u32),
    /// Fires when every sub-trigger fires.
    All(Vec<BecomesTrigger>),
    /// Fires when at least one sub-trigger fires.
    Any(Vec<BecomesTrigger>),
}

impl BecomesTrigger {
    /// Evaluate whether this trigger has fired given the entity's current state.
    pub fn evaluate(
        &self,
        slots: Option<&ItemSlots>,
        started_tick: u64,
        current_tick: u64,
    ) -> bool {
        match self {
            BecomesTrigger::SlotsFilled => slots.is_some_and(slots_filled),
            BecomesTrigger::AfterTicks(ticks) => {
                current_tick.saturating_sub(started_tick) >= *ticks as u64
            }
            BecomesTrigger::All(subs) => subs
                .iter()
                .all(|s| s.evaluate(slots, started_tick, current_tick)),
            BecomesTrigger::Any(subs) => subs
                .iter()
                .any(|s| s.evaluate(slots, started_tick, current_tick)),
        }
    }
}

/// Returns true if the entity has at least one Construction slot AND every
/// Construction slot has reached its `required` quantity.
pub fn slots_filled(slots: &ItemSlots) -> bool {
    let mut saw_construction = false;
    for slot in &slots.slots {
        if let SlotRole::Construction { required, .. } = slot.role {
            saw_construction = true;
            if slot.total_quantity() < required {
                return false;
            }
        }
    }
    saw_construction
}

/// Process all entities with a `Becomes` component. For any whose trigger has
/// fired this tick, dispatch on `mode`: `Replace` despawns the source and
/// spawns a fresh target entity; `InPlace` morphs the existing entity.
///
/// Runs after action effects (which mutate slots) and before perception
/// (so observers see consistent state).
pub fn becomes_system(
    mut commands: Commands,
    query: Query<(Entity, &Becomes, &Transform, Option<&ItemSlots>)>,
    tick: Res<TickCount>,
) {
    for (entity, becomes, transform, slots) in query.iter() {
        if !becomes
            .trigger
            .evaluate(slots, becomes.started_tick, tick.current)
        {
            continue;
        }

        match becomes.mode {
            BecomesMode::Replace => {
                let position = transform.translation.truncate();
                commands.entity(entity).despawn();
                spawn_concept_entity(&mut commands, becomes.target, position);
            }
            BecomesMode::InPlace => {
                transform_concept_in_place(&mut commands, entity, becomes.target);
                // Drop the trigger so the next tick doesn't fire it again.
                commands.entity(entity).remove::<Becomes>();
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
    use crate::agent::item_slots::Slot;

    fn site_slots(material: Concept, required: u32) -> ItemSlots {
        ItemSlots {
            slots: vec![Slot::construction(material, required)],
        }
    }

    #[test]
    fn slots_filled_true_when_construction_slot_at_required() {
        let mut slots = site_slots(Concept::Wood, 3);
        slots.deposit(Concept::Wood, 3, None);
        assert!(slots_filled(&slots));
    }

    #[test]
    fn slots_filled_false_when_partial() {
        let mut slots = site_slots(Concept::Wood, 3);
        slots.deposit(Concept::Wood, 2, None);
        assert!(!slots_filled(&slots));
    }

    #[test]
    fn slots_filled_false_when_no_construction_slots() {
        let slots = ItemSlots::agent_carry();
        assert!(!slots_filled(&slots));
    }

    #[test]
    fn slots_filled_requires_all_construction_slots() {
        let mut slots = ItemSlots {
            slots: vec![
                Slot::construction(Concept::Wood, 3),
                Slot::construction(Concept::Stone, 2),
            ],
        };
        slots.deposit(Concept::Wood, 3, None);
        // Only wood is filled — stone is still empty.
        assert!(!slots_filled(&slots));
        slots.deposit(Concept::Stone, 2, None);
        assert!(slots_filled(&slots));
    }

    #[test]
    fn after_ticks_fires_at_exact_deadline() {
        let trigger = BecomesTrigger::AfterTicks(10);
        assert!(!trigger.evaluate(None, 100, 109));
        assert!(trigger.evaluate(None, 100, 110));
        assert!(trigger.evaluate(None, 100, 200));
    }

    #[test]
    fn after_ticks_handles_started_tick_in_future_safely() {
        // saturating_sub: if current < started, age is 0 → not fired
        let trigger = BecomesTrigger::AfterTicks(5);
        assert!(!trigger.evaluate(None, 100, 50));
    }

    #[test]
    fn composite_all_requires_every_subtrigger() {
        let mut slots = site_slots(Concept::Wood, 3);
        let trigger = BecomesTrigger::All(vec![
            BecomesTrigger::SlotsFilled,
            BecomesTrigger::AfterTicks(5),
        ]);
        // Slots empty, time not elapsed
        assert!(!trigger.evaluate(Some(&slots), 100, 102));
        // Time elapsed but slots empty
        assert!(!trigger.evaluate(Some(&slots), 100, 110));
        // Slots filled but time not elapsed
        slots.deposit(Concept::Wood, 3, None);
        assert!(!trigger.evaluate(Some(&slots), 100, 102));
        // Both: fires
        assert!(trigger.evaluate(Some(&slots), 100, 110));
    }

    #[test]
    fn composite_any_fires_on_first_subtrigger() {
        let slots = site_slots(Concept::Wood, 3);
        let trigger = BecomesTrigger::Any(vec![
            BecomesTrigger::SlotsFilled,
            BecomesTrigger::AfterTicks(5),
        ]);
        // Neither
        assert!(!trigger.evaluate(Some(&slots), 100, 102));
        // Time elapsed: fires
        assert!(trigger.evaluate(Some(&slots), 100, 110));
    }
}
