//! Generic target enumeration: turn a `TargetSource` into concrete `TargetCandidate`s.
//!
//! Reads: MindGraph (Contains/HasTrait beliefs), Affordance components on world entities
//! Writes: Vec<TargetCandidate> for the rational brain
//! Upstream: rational brain (asks "what targets does action X have?")
//! Downstream: rational brain (turns candidates into ActionTemplates)
//!
//! This module replaces the per-action collectors that used to live in
//! `rational.rs` (`collect_resource_targets`, `collect_affordance_targets`).
//! Adding a new entity-trait or tile-trait action now requires zero changes
//! here — the action declares its `TargetSource` and the brain just calls
//! `enumerate_targets`.

use bevy::prelude::*;

use crate::agent::Dead;
use crate::agent::actions::{ActionType, TargetCandidate, TargetSource};
use crate::agent::affordance::Affordance;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::world::map::TILE_SIZE;

/// Resolve a `TargetSource` into the concrete list of candidates for a given
/// action and agent mind.
///
/// - `TargetSource::None` → one [`TargetCandidate::None`]
/// - `TargetSource::Implicit` → empty (the planner injects these directly)
/// - `TargetSource::EntityAffordance` → every known entity whose `Affordance`
///   declares `action_type`
/// - `TargetSource::EntityWithTrait(c)` → every perceived entity whose
///   inherited traits include `c` (e.g. Attack/Bite finding `HasTrait Prey`)
/// - `TargetSource::TileWithTrait(c)` → every known tile matching
///   `Tile(?) HasTrait c` in the MindGraph
///
/// The `affordances` query is a bare `&Query<...>` so Bevy's elided
/// WorldQuery lifetimes flow through to callers without a type alias
/// pinning them to `'static`.
pub fn enumerate_targets(
    source: &TargetSource,
    action_type: ActionType,
    mind: &MindGraph,
    affordances: &Query<(&GlobalTransform, Option<&Affordance>, Option<&Dead>)>,
) -> Vec<TargetCandidate> {
    match source {
        TargetSource::None => vec![TargetCandidate::None],
        TargetSource::Implicit => Vec::new(),
        TargetSource::EntityAffordance => {
            enumerate_entities_with_affordance(action_type, mind, affordances)
        }
        TargetSource::EntityWithTrait(concept) => {
            enumerate_entities_with_trait(*concept, mind, affordances, AliveOnly::Yes)
        }
        TargetSource::DeadEntityWithTrait(concept) => {
            enumerate_entities_with_trait(*concept, mind, affordances, AliveOnly::No)
        }
        TargetSource::TileWithTrait(concept) => enumerate_tiles_with_trait(*concept, mind),
    }
}

/// Iterate perceived entities and keep the ones whose world `Affordance`
/// component declares the requested action type.
///
/// "Perceived" means the agent's mind has either a `Contains` belief
/// (observed inventory — e.g. "this bush has 3 berries") or an `IsA`
/// belief (observed type — e.g. "this is a BerryBush"). The IsA path is
/// critical for first-sight planning: before #416, enumeration only ran
/// over `Contains` beliefs, so an agent that perceived a BerryBush but
/// hadn't yet seen its contents couldn't plan a Harvest against it and
/// fell back to random Explore forever. With IsA included, the agent
/// can act on a target the moment perception adds it to the MindGraph,
/// using type-level `Produces` facts (seeded in `add_person_knowledge`)
/// to derive the expected yield.
fn enumerate_entities_with_affordance(
    action_type: ActionType,
    mind: &MindGraph,
    affordances: &Query<(&GlobalTransform, Option<&Affordance>, Option<&Dead>)>,
) -> Vec<TargetCandidate> {
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let consider = |entity: Entity,
                    candidates: &mut Vec<TargetCandidate>,
                    seen: &mut std::collections::HashSet<Entity>| {
        if !seen.insert(entity) {
            return;
        }
        let Ok((transform, Some(affordance), _dead)) = affordances.get(entity) else {
            return;
        };
        if affordance.action_type != action_type {
            return;
        }
        candidates.push(TargetCandidate::Entity {
            entity,
            pos: transform.translation().truncate(),
        });
    };

    for triple in mind.query(None, Some(Predicate::Contains), None) {
        if let Node::Entity(entity) = triple.subject {
            consider(entity, &mut candidates, &mut seen);
        }
    }

    for triple in mind.query(None, Some(Predicate::IsA), None) {
        if let Node::Entity(entity) = triple.subject {
            consider(entity, &mut candidates, &mut seen);
        }
    }

    candidates
}

/// Whether a trait-based enumerator should keep dead entities. Bite/Attack
/// pass `Yes` (corpses are filtered out — a dead deer is not exhibiting
/// Prey-hood). Devour passes `No` (corpses are exactly the targets — Carrion
/// implies dead).
#[derive(Clone, Copy, PartialEq, Eq)]
enum AliveOnly {
    Yes,
    No,
}

/// Iterate every perceived entity (anything the mind has an `IsA` belief
/// about) and keep the ones whose ontology trait inheritance includes
/// `trait_concept`.
///
/// Used by Attack and Bite to enumerate prey: perception writes
/// `(deer_42, IsA, Concept::Deer)` for every visible deer, and
/// `mind.has_trait` walks the IsA chain to discover that
/// `(Deer, HasTrait, Prey)` lives in cultural/intrinsic knowledge.
///
/// `alive_only` controls the Dead-marker filter. Most trait-based actions
/// want living targets — a corpse still carries `IsA Deer` in observer
/// minds until belief invalidation (#524) lands, but a dead deer is not
/// exhibiting Prey-hood in any actionable sense. Devour and other carrion-
/// targeting actions invert this and want dead entities specifically.
fn enumerate_entities_with_trait(
    trait_concept: Concept,
    mind: &MindGraph,
    affordances: &Query<(&GlobalTransform, Option<&Affordance>, Option<&Dead>)>,
    alive_only: AliveOnly,
) -> Vec<TargetCandidate> {
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for triple in mind.query(None, Some(Predicate::IsA), None) {
        let Node::Entity(entity) = triple.subject else {
            continue;
        };
        if !seen.insert(entity) {
            continue;
        }
        if !mind.has_trait(&Node::Entity(entity), trait_concept) {
            continue;
        }

        let Ok((transform, _, dead)) = affordances.get(entity) else {
            continue;
        };
        match alive_only {
            AliveOnly::Yes if dead.is_some() => continue,
            AliveOnly::No if dead.is_none() => continue,
            _ => {}
        }
        candidates.push(TargetCandidate::Entity {
            entity,
            pos: transform.translation().truncate(),
        });
    }

    candidates
}

/// Iterate tiles matching `Tile(?) HasTrait <concept>` in the MindGraph and
/// emit one candidate per known tile. The world position is the tile centre
/// so the planner's distance heuristic and the implicit Walk generator both
/// have something to chew on.
///
/// Drink uses this with `Concept::Drinkable`. Future tile-trait actions
/// (Fish, Forage, Bathe, Sleep-in-shelter) plug in here without touching
/// the brain.
fn enumerate_tiles_with_trait(concept: Concept, mind: &MindGraph) -> Vec<TargetCandidate> {
    let mut candidates = Vec::new();

    for triple in mind.query(
        None,
        Some(Predicate::HasTrait),
        Some(&Value::Concept(concept)),
    ) {
        let Node::Tile((tx, ty)) = triple.subject else {
            continue;
        };

        let pos = Vec2::new(
            tx as f32 * TILE_SIZE + TILE_SIZE / 2.0,
            ty as f32 * TILE_SIZE + TILE_SIZE / 2.0,
        );
        candidates.push(TargetCandidate::Tile {
            tile: (tx, ty),
            pos,
        });
    }

    candidates
}
