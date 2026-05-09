//! Generic concept-to-entity spawn dispatch.
//!
//! Reads: Concept (target type)
//! Writes: Spawns world entities via Commands
//! Upstream: action SpawnRequest processing, Becomes substrate transformations
//! Downstream: world entities (visible, perceivable by agents)
//!
//! Single source of truth for "spawn an entity of this Concept at this position".
//! Add new branches here as new spawnable entity types come online.

use crate::agent::mind::knowledge::Concept;
use crate::palette::Palette;
use bevy::prelude::*;

/// Spawn an entity of the given concept at the given world position.
///
/// Returns `Some(entity_id)` for handled concepts; returns `None` for concepts
/// without a registered spawner. Callers may log/skip when `None` is returned.
///
/// Campfires use the sprited spawner so Build-produced campfires are visible
/// in the windowed game — matching pre-placed ones from world init. Color-based
/// sprites (no textures) are inert in headless, so tests don't regress.
pub fn spawn_concept_entity(
    commands: &mut Commands,
    palette: &Palette,
    concept: Concept,
    position: Vec2,
    _started_tick: u64,
) -> Option<Entity> {
    match concept {
        Concept::Campfire => Some(crate::world::campfire::spawn_campfire(
            commands, palette, position,
        )),
        Concept::Corpse => Some(crate::world::corpse::spawn_corpse_headless(
            commands, position,
        )),
        Concept::Ash => Some(spawn_ash(commands, position)),
        _ => None,
    }
}

/// In-place transformation dispatcher: morphs an existing entity into the
/// target concept, preserving its entity ID. Used by the Becomes substrate's
/// `InPlace` mode (e.g. slain prey -> Corpse).
pub fn transform_concept_in_place(
    commands: &mut Commands,
    entity: Entity,
    concept: Concept,
) -> bool {
    match concept {
        Concept::Corpse => {
            crate::world::corpse::kill_into_corpse(
                commands,
                entity,
                crate::world::corpse::DEFAULT_CORPSE_MEAT,
            );
            true
        }
        _ => false,
    }
}

fn spawn_ash(commands: &mut Commands, position: Vec2) -> Entity {
    commands
        .spawn((
            Name::new("Ash"),
            crate::agent::inventory::EntityType(Concept::Ash),
            crate::world::Physical,
            Transform::from_translation(position.extend(0.5)),
            GlobalTransform::default(),
        ))
        .id()
}
