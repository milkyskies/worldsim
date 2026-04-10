//! Construction site spawning.
//!
//! Reads: Concept (target recipe), recipe requirements
//! Writes: Construction site entities (ItemSlots + Becomes + EntityType + Transform)
//! Upstream: execution system (Build action on_complete → SpawnRequest::Site)
//! Downstream: becomes_system (transforms site → finished entity when slots fill),
//!             perception (observers see the site as a perceivable world entity)
//!
//! A construction site is built entirely from existing primitives:
//! - `ItemSlots` with one `Construction` slot per recipe component
//! - `Becomes { target, trigger: SlotsFilled }` from the substrate
//! - Standard perceivable-entity components (Transform, Physical, EntityType)
//!
//! There is intentionally no "construction-specific" logic. Filling the slots
//! is the trigger; the `becomes_system` handles the rest.

use crate::agent::inventory::EntityType;
use crate::agent::item_slots::{ItemSlots, Slot};
use crate::agent::mind::knowledge::Concept;
use crate::world::becomes::{Becomes, BecomesTrigger};
use bevy::prelude::*;

/// Marker for construction site entities. Lets queries narrow to "sites only"
/// without having to read `EntityType` and compare against `ConstructionSite`.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct ConstructionSiteMarker;

/// Spawn a construction site at `position` that will become `target` when its
/// Construction slots are filled.
///
/// `requirements` is the list of `(material, required_quantity)` tuples derived
/// from the target concept's `Requires` recipe triples. One Construction slot is
/// created per requirement.
///
/// `initial_items` are deposited into the matching slots immediately. Used by
/// the Build action when the agent already has the materials in hand. If
/// `initial_items` fully satisfies all requirements, the next `becomes_system`
/// pass will transform the site into the finished entity.
///
/// `started_tick` records when the site was placed (used by composite triggers
/// like `All([SlotsFilled, AfterTicks(N)])` for cooking-style processes).
pub fn spawn_construction_site_headless(
    commands: &mut Commands,
    target: Concept,
    position: Vec2,
    requirements: &[(Concept, u32)],
    initial_items: &[(Concept, u32)],
    started_tick: u64,
) -> Entity {
    let mut item_slots = ItemSlots {
        slots: requirements
            .iter()
            .map(|(material, required)| Slot::construction(*material, *required))
            .collect(),
    };

    for (material, qty) in initial_items {
        item_slots.deposit(*material, *qty, None);
    }

    commands
        .spawn((
            Name::new(format!("Site<{target:?}>")),
            EntityType(Concept::ConstructionSite),
            ConstructionSiteMarker,
            crate::world::Physical,
            Transform::from_translation(position.extend(1.0)),
            GlobalTransform::default(),
            item_slots,
            Becomes::new(target, BecomesTrigger::SlotsFilled, started_tick),
        ))
        .id()
}
