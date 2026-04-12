//! Corpse: the post-kill state of a slain creature.
//!
//! Reads: nothing
//! Writes: corpse entities — strips living components from the source entity
//!         and adds Affordance::Harvest plus meat to its existing ItemSlots.
//! Upstream: Becomes substrate (InPlace mode), called from Attack/Bite kills
//! Downstream: agent perception, Harvest action (scavengers + the killer)
//!
//! A corpse is the slain creature's *same entity* with its living components
//! stripped and its body's meat exposed for harvesting. We do not despawn
//! and respawn — preserving the entity ID keeps episodic memory and
//! relationship triples (e.g. "wolf_pack_member_3 IsA Friend") pointing at
//! a meaningful entity even after death.

use crate::agent::actions::ActionType;
use crate::agent::affordance::Affordance;
use crate::agent::inventory::EntityType;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::Concept;
use crate::ui::sprite_animation::SpriteBody;
use crate::world::property::HarvestableComponent;
use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

/// Default amount of meat a freshly butchered corpse holds. Future per-species
/// yields can override this; for now every prey species drops one unit.
pub const DEFAULT_CORPSE_MEAT: u32 = 1;

/// Component bundle for a freshly-spawned corpse with no prior identity.
/// Used by `spawn_concept_entity` for the rare case where a corpse is
/// summoned standalone (e.g. tests, future "old bones" world generation).
pub fn corpse_components(position: Vec2, meat_qty: u32) -> impl Bundle {
    let mut inventory = ItemSlots::agent_carry();
    if meat_qty > 0 {
        inventory.add(Concept::Meat, meat_qty);
    }

    (
        Name::new("Corpse"),
        EntityType(Concept::Corpse),
        crate::world::Physical,
        Transform::from_translation(position.extend(1.0)),
        GlobalTransform::default(),
        inventory,
        Affordance {
            action_type: ActionType::Harvest,
            cost: 2.0,
            distance: 16.0,
            risk: 0.0,
        },
        HarvestableComponent {
            yields: Concept::Meat,
        },
    )
}

/// Headless spawner — used by `spawn_concept_entity` when a Becomes/Replace
/// rule fires with `target = Corpse`.
pub fn spawn_corpse_headless(commands: &mut Commands, position: Vec2) -> Entity {
    commands
        .spawn(corpse_components(position, DEFAULT_CORPSE_MEAT))
        .id()
}

/// In-place transformation: morph an existing living entity into a corpse,
/// preserving its entity ID, Transform, Name, MindGraph, and Body. Strips
/// the components that make it actively think (Agent marker, Vision,
/// ActiveActions, movement state) so brain/perception/movement systems
/// stop processing it.
///
/// Called by the Becomes substrate when an `InPlace` Becomes triggers
/// against a living entity. Issued from Attack/Bite's `on_complete` via
/// `SpawnRequest::BecomesAttach { mode: InPlace, ... }`.
///
/// Bevy's command-queue API doesn't let us mutate a component (`ItemSlots`)
/// from a regular `commands` chain — we have to drop into a deferred
/// closure with full world access to deposit the meat.
pub fn kill_into_corpse(commands: &mut Commands, entity: Entity, meat_qty: u32) {
    let mut queue = CommandQueue::default();
    queue.push(move |world: &mut World| {
        // Tilt corpses: humans fall on their side (90°), animals flip upside down (180°).
        let rotation_z = {
            let concept = world.get::<EntityType>(entity).map(|et| et.0);
            match concept {
                Some(Concept::Person) => std::f32::consts::FRAC_PI_2,
                _ => std::f32::consts::PI,
            }
        };

        {
            let mut sb_query = world.query::<(Entity, &SpriteBody)>();
            let sb_entity = sb_query
                .iter(world)
                .find(|(_, sb)| sb.root == entity)
                .map(|(e, _)| e);
            if let Some(sb_entity) = sb_entity
                && let Ok(mut sb_mut) = world.get_entity_mut(sb_entity)
                && let Some(mut transform) = sb_mut.get_mut::<Transform>()
            {
                transform.rotation = Quat::from_rotation_z(rotation_z);
            }
        }

        let Ok(mut entity_mut) = world.get_entity_mut(entity) else {
            return;
        };

        // Strip the components that make the entity actively alive.
        // `die()` already removes `Alive` and inserts `Dead` before the
        // Becomes substrate fires, but we do it defensively here too in
        // case anything ever calls `kill_into_corpse` directly.
        // Brain/perception/action systems use `With<Agent>` or
        // `With<Alive>`, so removing both markers stops all processing.
        // Mind/body data stays on the entity as frozen memorial state.
        entity_mut.remove::<crate::agent::Alive>();
        if !entity_mut.contains::<crate::agent::Dead>() {
            entity_mut.insert(crate::agent::Dead);
        }
        entity_mut.remove::<crate::agent::Agent>();
        entity_mut.remove::<crate::agent::mind::perception::Vision>();
        entity_mut.remove::<crate::agent::actions::ActiveActions>();
        entity_mut.remove::<crate::agent::movement::MovementState>();
        entity_mut.remove::<crate::agent::TargetPosition>();

        // Swap identity and add the corpse-specific affordance.
        entity_mut.insert(EntityType(Concept::Corpse));
        entity_mut.insert(Name::new("Corpse"));
        entity_mut.insert(Affordance {
            action_type: ActionType::Harvest,
            cost: 2.0,
            distance: 16.0,
            risk: 0.0,
        });
        entity_mut.insert(HarvestableComponent {
            yields: Concept::Meat,
        });

        if meat_qty > 0
            && let Some(mut slots) = entity_mut.get_mut::<ItemSlots>()
        {
            slots.add(Concept::Meat, meat_qty);
        }
    });
    commands.append(&mut queue);
}
