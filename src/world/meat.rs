//! Meat drop spawning logic.
//!
//! Reads: nothing
//! Writes: Meat drop entities (EntityType, Transform, ItemSlots holding Meat,
//!         Affordance for Take, Physical)
//! Upstream: Becomes substrate (when a slain deer transforms into Meat)
//! Downstream: agent perception, Take action
//!
//! A meat drop is the post-kill remains of a Prey entity. It exists so other
//! agents (scavenging wolves, hungry hunters) can `Take` from the carcass
//! after the killer has moved on.

use crate::agent::actions::ActionType;
use crate::agent::affordance::Affordance;
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::Concept;
use bevy::prelude::*;

/// Bundle of components shared between the visual and headless meat-drop
/// spawners. One unit of meat is currently the canonical drop quantity —
/// future butchery actions can multiply this when introduced.
pub fn meat_drop_components(position: Vec2) -> impl Bundle {
    let mut inventory = ItemSlots::agent_carry();
    inventory.add(Concept::Meat, 1);

    (
        Name::new("Meat"),
        EntityType(Concept::Meat),
        crate::world::Physical,
        Transform::from_translation(position.extend(1.0)),
        GlobalTransform::default(),
        inventory,
        Affordance {
            action_type: ActionType::Take,
            cost: 1.0,
            distance: 16.0,
            risk: 0.0,
        },
    )
}

/// Logic-only meat drop spawner used by the Becomes substrate when a slain
/// deer transforms. No sprites — visual variants live in the rendering layer.
pub fn spawn_meat_drop_headless(commands: &mut Commands, position: Vec2) -> Entity {
    commands.spawn(meat_drop_components(position)).id()
}
