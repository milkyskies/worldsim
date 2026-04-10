//! Construction site spawning.
//!
//! Reads: Concept (target recipe), recipe requirements
//! Writes: Construction site entities (ItemSlots + Becomes + EntityType + Transform + optional Affordance)
//! Upstream: execution system (Build action on_complete → SpawnRequest::Site)
//! Downstream: becomes_system (transforms site → finished entity when slots fill),
//!             labor_accumulation_system (increments labor counter for Construct actions),
//!             perception (observers see the site as a perceivable world entity)
//!
//! A construction site is built entirely from existing primitives:
//! - `ItemSlots` with one `Construction` slot per recipe component
//! - `Becomes { target, trigger }` from the substrate (SlotsFilled or All([SlotsFilled, LaborAccumulated]))
//! - Standard perceivable-entity components (Transform, Physical, EntityType)
//! - Optional `Affordance { Construct }` when the site requires labor (enables GOAP planning)
//!
//! There is intentionally no "construction-specific" logic. Filling the slots
//! is the trigger; the `becomes_system` handles the rest.

use crate::agent::actions::ActionType;
use crate::agent::affordance::Affordance;
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::{ItemSlots, Slot};
use crate::agent::mind::knowledge::Concept;
use crate::constants::actions::construct::INTERACTION_DISTANCE;
use crate::world::becomes::{Becomes, BecomesTrigger};
use bevy::prelude::*;

/// Marker for construction site entities. Lets queries narrow to "sites only"
/// without having to read `EntityType` and compare against `ConstructionSite`.
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct ConstructionSiteMarker;

/// Spawn a construction site at `position` that will become `target` when its
/// Construction slots are filled (and optional labor is accumulated).
///
/// `requirements` is the list of `(material, required_quantity)` tuples derived
/// from the target concept's `Requires` recipe triples. One Construction slot is
/// created per requirement.
///
/// `initial_items` are deposited into the matching slots immediately. Used by
/// the Build action when the agent already has the materials in hand. If
/// `initial_items` fully satisfies all requirements AND no labor is required,
/// the next `becomes_system` pass will transform the site into the finished entity.
///
/// `labor_required` optionally adds a `LaborAccumulated` condition that agents
/// must satisfy via the `Construct` action. When `Some(n)`, the site's trigger
/// becomes `All([SlotsFilled, LaborAccumulated { required: n, current: 0 }])` and
/// an `Affordance { Construct }` component is added so the GOAP planner can find
/// this site as a Construct target.
///
/// `started_tick` records when the site was placed (used by composite triggers
/// like `All([SlotsFilled, AfterTicks(N)])` for cooking-style processes).
pub fn spawn_construction_site_headless(
    commands: &mut Commands,
    target: Concept,
    position: Vec2,
    requirements: &[(Concept, u32)],
    initial_items: &[(Concept, u32)],
    labor_required: Option<u32>,
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

    let trigger = match labor_required {
        None => BecomesTrigger::SlotsFilled,
        Some(required) => BecomesTrigger::All(vec![
            BecomesTrigger::SlotsFilled,
            BecomesTrigger::LaborAccumulated {
                required,
                current: 0,
            },
        ]),
    };

    let mut entity_cmd = commands.spawn((
        Name::new(format!("Site<{target:?}>")),
        EntityType(Concept::ConstructionSite),
        ConstructionSiteMarker,
        crate::world::Physical,
        Transform::from_translation(position.extend(1.0)),
        GlobalTransform::default(),
        item_slots,
        Becomes::new(target, trigger, started_tick),
    ));

    if labor_required.is_some() {
        entity_cmd.insert(Affordance {
            action_type: ActionType::Construct,
            cost: 3.0,
            distance: INTERACTION_DISTANCE,
            risk: 0.0,
        });
    }

    entity_cmd.id()
}
