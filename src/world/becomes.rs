//! Becomes: general substrate for entities that transform into other entities.
//!
//! Reads: Becomes (component), ItemSlots (for SlotsFilled trigger), Transform, TickCount,
//!        ActiveActions (for LaborAccumulated trigger), BuiltBy (carry-forward on transform)
//! Writes: Despawns source entities, spawns target concept entities at the same position,
//!         mutates LaborAccumulated.current on entities being actively constructed, SimEvent,
//!         MindGraph (writes (Self, Owns, new_entity) for the builder when BuiltBy is present)
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

use std::collections::HashMap;

use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::actions::ActionType;
use crate::agent::actions::registry::ActiveActions;
use crate::agent::events::SimEvent;
use crate::agent::item_slots::{ItemSlots, SlotRole};
use crate::agent::mind::knowledge::{Concept, Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::core::tick::TickCount;
use crate::world::property::BuiltBy;
use crate::world::spawn::{spawn_concept_entity, transform_concept_in_place};

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
    /// Fires when at least `required` ticks of labor have been contributed by
    /// agents actively executing a `Construct` action targeting this entity.
    ///
    /// `current` counts the accumulated labor ticks so far. It is incremented
    /// by `labor_accumulation_system` each tick — once per active constructor,
    /// so two agents both constructing add 2 per tick. Walking away stops
    /// accumulation; the counter persists because it lives on the component.
    ///
    /// TODO(#171): Migrate `required: u32` to `required: Duration` for
    /// intrinsic variance and skill-modulated labor rates.
    LaborAccumulated { required: u32, current: u32 },
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
            BecomesTrigger::LaborAccumulated { required, current } => current >= required,
            BecomesTrigger::All(subs) => subs
                .iter()
                .all(|s| s.evaluate(slots, started_tick, current_tick)),
            BecomesTrigger::Any(subs) => subs
                .iter()
                .any(|s| s.evaluate(slots, started_tick, current_tick)),
        }
    }

    /// Returns true if this trigger tree contains a `LaborAccumulated` variant
    /// at any depth.
    pub fn has_labor_accumulated(&self) -> bool {
        match self {
            BecomesTrigger::LaborAccumulated { .. } => true,
            BecomesTrigger::All(subs) | BecomesTrigger::Any(subs) => {
                subs.iter().any(|s| s.has_labor_accumulated())
            }
            _ => false,
        }
    }

    /// Return the `current` value of the first `LaborAccumulated` node found
    /// in the trigger tree (depth-first), or `None` if no such node exists.
    pub fn labor_current(&self) -> Option<u32> {
        match self {
            BecomesTrigger::LaborAccumulated { current, .. } => Some(*current),
            BecomesTrigger::All(subs) | BecomesTrigger::Any(subs) => {
                subs.iter().find_map(|s| s.labor_current())
            }
            _ => None,
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
/// If the source carried a `BuiltBy` component, the world-truth record is
/// copied onto the new entity AND the builder's MindGraph receives a
/// `(Self, Owns, new_entity)` triple — experiential knowledge that they
/// own what they built. The triple is written directly because ownership
/// is a consequence of the build action, not something the builder needs
/// to perceive.
///
/// Runs after action effects (which mutate slots) and before perception
/// (so observers see consistent state).
pub fn becomes_system(
    mut commands: Commands,
    query: Query<(
        Entity,
        &Becomes,
        &Transform,
        Option<&ItemSlots>,
        Option<&BuiltBy>,
    )>,
    mut agent_minds: Query<&mut MindGraph, With<Agent>>,
    tick: Res<TickCount>,
) {
    for (entity, becomes, transform, slots, built_by) in query.iter() {
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
                let Some(new_entity) = spawn_concept_entity(
                    &mut commands,
                    becomes.target,
                    position,
                    tick.current,
                ) else {
                    continue;
                };

                // Carry BuiltBy forward and record ownership in the builder's mind.
                let Some(built_by) = built_by else {
                    continue;
                };
                commands.entity(new_entity).insert(BuiltBy {
                    builder: built_by.builder,
                    built_at: built_by.built_at,
                });
                if let Ok(mut mind) = agent_minds.get_mut(built_by.builder) {
                    mind.assert(Triple::with_meta(
                        Node::Self_,
                        Predicate::Owns,
                        Value::Entity(new_entity),
                        Metadata::default(),
                    ));
                }
            }
            BecomesMode::InPlace => {
                transform_concept_in_place(&mut commands, entity, becomes.target);
                // Drop the trigger so the next tick doesn't fire it again.
                // BuiltBy is intentionally preserved by in-place transforms,
                // since the entity ID is the same — ownership of a corpse
                // would still trace back to the killer if BuiltBy were
                // attached, though prey aren't BuiltBy in current code.
                commands.entity(entity).remove::<Becomes>();
            }
        }
    }
}

/// Increment `LaborAccumulated.current` on every entity that has at least one
/// agent actively running a `Construct` action targeting it this tick.
///
/// Each active constructor contributes 1 labor-tick per simulation tick.
/// Multiple agents add up linearly. Walking away (removing the Construct
/// action from `ActiveActions`) stops accumulation — the counter persists
/// on the `Becomes` component unchanged until construction resumes.
///
/// Runs after `apply_action_effects` and before `becomes_system`, so
/// accumulated labor can fire a `LaborAccumulated` trigger within the same
/// tick that it crosses the threshold.
pub fn labor_accumulation_system(
    active_actions: Query<(Entity, &ActiveActions)>,
    mut becomes_query: Query<&mut Becomes>,
    tick: Res<TickCount>,
    mut events: MessageWriter<SimEvent>,
) {
    // Collect (agent, site) pairs for all active Construct actions.
    let mut constructor_pairs: Vec<(Entity, Entity)> = Vec::new();
    for (agent_entity, actions) in active_actions.iter() {
        for action_state in actions.iter() {
            if action_state.action_type == ActionType::Construct
                && let Some(target) = action_state.target_entity
            {
                constructor_pairs.push((agent_entity, target));
            }
        }
    }

    // Aggregate and increment labor counters.
    let mut constructors_per_site: HashMap<Entity, u32> = HashMap::new();
    for (_, site) in &constructor_pairs {
        *constructors_per_site.entry(*site).or_insert(0) += 1;
    }
    for (site_entity, constructor_count) in &constructors_per_site {
        if let Ok(mut becomes) = becomes_query.get_mut(*site_entity) {
            increment_labor_in_trigger(&mut becomes.trigger, *constructor_count);
        }
    }

    // Emit one event per active constructor so the structured log can trace progress.
    for (agent_entity, site_entity) in constructor_pairs {
        events.write(SimEvent::LaborContributed {
            agent: agent_entity,
            tick: tick.current,
            site: site_entity,
        });
    }
}

/// Recursively find and increment every `LaborAccumulated` node in a trigger
/// tree by `amount`. Handles nested `All` / `Any` composites at arbitrary depth.
pub fn increment_labor_in_trigger(trigger: &mut BecomesTrigger, amount: u32) {
    match trigger {
        BecomesTrigger::LaborAccumulated { current, .. } => {
            *current = current.saturating_add(amount);
        }
        BecomesTrigger::All(subs) | BecomesTrigger::Any(subs) => {
            for sub in subs.iter_mut() {
                increment_labor_in_trigger(sub, amount);
            }
        }
        _ => {}
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

    #[test]
    fn labor_accumulated_fires_when_current_reaches_required() {
        assert!(
            !BecomesTrigger::LaborAccumulated {
                required: 5,
                current: 4,
            }
            .evaluate(None, 0, 0)
        );
        assert!(
            BecomesTrigger::LaborAccumulated {
                required: 5,
                current: 5,
            }
            .evaluate(None, 0, 0)
        );
        assert!(
            BecomesTrigger::LaborAccumulated {
                required: 5,
                current: 10,
            }
            .evaluate(None, 0, 0)
        );
    }

    #[test]
    fn labor_accumulated_zero_required_fires_immediately() {
        assert!(
            BecomesTrigger::LaborAccumulated {
                required: 0,
                current: 0,
            }
            .evaluate(None, 0, 0)
        );
    }

    #[test]
    fn increment_labor_updates_direct_variant() {
        let mut trigger = BecomesTrigger::LaborAccumulated {
            required: 10,
            current: 3,
        };
        increment_labor_in_trigger(&mut trigger, 2);
        match trigger {
            BecomesTrigger::LaborAccumulated { current, .. } => assert_eq!(current, 5),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn increment_labor_updates_nested_inside_all() {
        let mut trigger = BecomesTrigger::All(vec![
            BecomesTrigger::SlotsFilled,
            BecomesTrigger::LaborAccumulated {
                required: 10,
                current: 0,
            },
        ]);
        increment_labor_in_trigger(&mut trigger, 3);
        if let BecomesTrigger::All(subs) = &trigger {
            if let BecomesTrigger::LaborAccumulated { current, .. } = &subs[1] {
                assert_eq!(*current, 3);
            } else {
                panic!("wrong sub-variant");
            }
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn labor_current_returns_current_value() {
        assert_eq!(
            BecomesTrigger::LaborAccumulated {
                required: 10,
                current: 7,
            }
            .labor_current(),
            Some(7)
        );
        assert_eq!(BecomesTrigger::SlotsFilled.labor_current(), None);
        // Nested inside All
        let trigger = BecomesTrigger::All(vec![
            BecomesTrigger::SlotsFilled,
            BecomesTrigger::LaborAccumulated {
                required: 5,
                current: 3,
            },
        ]);
        assert_eq!(trigger.labor_current(), Some(3));
    }

    #[test]
    fn has_labor_accumulated_detects_nested() {
        let trigger = BecomesTrigger::All(vec![
            BecomesTrigger::SlotsFilled,
            BecomesTrigger::LaborAccumulated {
                required: 10,
                current: 0,
            },
        ]);
        assert!(trigger.has_labor_accumulated());
        assert!(!BecomesTrigger::SlotsFilled.has_labor_accumulated());
        assert!(!BecomesTrigger::AfterTicks(5).has_labor_accumulated());
    }
}
