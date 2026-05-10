//! `WorldEntityPositions` — global snapshot of static (non-agent) world
//! entities and their current tile.
//!
//! Replaces the per-agent `(StaticEntity, LocatedAt, Tile(...))`
//! perception writes (#756). Static-object positions are objective world
//! facts: the tree is at `(45, 67)` regardless of which agent is
//! looking. Mirroring that into every perceiving agent's MindGraph
//! duplicated the same fact N times across N agents.
//!
//! The planner, runtime gates, and search heuristics that used to walk
//! `mind.query(_, LocatedAt, _)` now consult this resource. Mobile
//! entities — other agents — keep their `LocatedAt` triples in
//! MindGraph, where stale beliefs ("I last saw Bob at the river") are
//! the right shape.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::agent::Agent;
use crate::agent::inventory::EntityType;
use crate::agent::mind::knowledge::Concept;
use crate::world::Physical;
use crate::world::map::TILE_SIZE;

/// One entry per static world entity. The concept is included so the
/// planner can filter by type (e.g. "find every nearby Campfire") without
/// going through `mind.is_a` — the MindGraph still stores `IsA` for
/// perceived entities, but for unobserved-but-real static entities the
/// mind doesn't have that fact.
#[derive(Debug, Clone, Copy)]
pub struct EntityLocation {
    pub concept: Concept,
    pub tile: (i32, i32),
}

#[derive(Resource, Default)]
pub struct WorldEntityPositions {
    by_entity: HashMap<Entity, EntityLocation>,
}

impl WorldEntityPositions {
    pub fn position_of(&self, entity: Entity) -> Option<(i32, i32)> {
        self.by_entity.get(&entity).map(|e| e.tile)
    }

    pub fn entry(&self, entity: Entity) -> Option<&EntityLocation> {
        self.by_entity.get(&entity)
    }

    /// Iterate every static entity at `tile`.
    pub fn entities_at_tile<'a>(&'a self, tile: (i32, i32)) -> impl Iterator<Item = Entity> + 'a {
        self.by_entity
            .iter()
            .filter(move |(_, loc)| loc.tile == tile)
            .map(|(e, _)| *e)
    }

    /// Iterate every (entity, location) pair.
    pub fn iter(&self) -> impl Iterator<Item = (Entity, &EntityLocation)> {
        self.by_entity.iter().map(|(e, l)| (*e, l))
    }
}

/// Rebuild the static-position snapshot every tick. Static = `Physical`
/// without `Agent` — trees, rocks, campfires, lean-tos, chests, logs.
/// Cheap (one map clear + one Transforms scan); runs before any consumer
/// that relies on it.
pub fn update_world_entity_positions(
    mut positions: ResMut<WorldEntityPositions>,
    statics: Query<(Entity, &Transform, &EntityType), (With<Physical>, Without<Agent>)>,
) {
    positions.by_entity.clear();
    for (entity, transform, entity_type) in statics.iter() {
        let pos = transform.translation.truncate();
        let tile = (
            (pos.x / TILE_SIZE).floor() as i32,
            (pos.y / TILE_SIZE).floor() as i32,
        );
        positions.by_entity.insert(
            entity,
            EntityLocation {
                concept: entity_type.0,
                tile,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_lookup_returns_inserted_tile() {
        let mut positions = WorldEntityPositions::default();
        let entity = Entity::from_bits(42);
        positions.by_entity.insert(
            entity,
            EntityLocation {
                concept: Concept::Campfire,
                tile: (3, 4),
            },
        );
        assert_eq!(positions.position_of(entity), Some((3, 4)));
        assert!(positions.entry(entity).is_some());
    }

    #[test]
    fn entities_at_tile_filters_by_position() {
        let mut positions = WorldEntityPositions::default();
        let a = Entity::from_bits(1);
        let b = Entity::from_bits(2);
        positions.by_entity.insert(
            a,
            EntityLocation {
                concept: Concept::Campfire,
                tile: (5, 5),
            },
        );
        positions.by_entity.insert(
            b,
            EntityLocation {
                concept: Concept::LeanTo,
                tile: (7, 7),
            },
        );

        let at_5_5: Vec<Entity> = positions.entities_at_tile((5, 5)).collect();
        assert_eq!(at_5_5, vec![a]);
        assert!(positions.entities_at_tile((9, 9)).next().is_none());
    }
}
