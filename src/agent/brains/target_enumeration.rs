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
    affordances: &Query<(&GlobalTransform, Option<&Affordance>)>,
) -> Vec<TargetCandidate> {
    match source {
        TargetSource::None => vec![TargetCandidate::None],
        TargetSource::Implicit => Vec::new(),
        TargetSource::EntityAffordance => {
            enumerate_entities_with_affordance(action_type, mind, affordances)
        }
        TargetSource::EntityWithTrait(concept) => {
            enumerate_entities_with_trait(*concept, mind, affordances)
        }
        TargetSource::TileWithTrait(concept) => enumerate_tiles_with_trait(*concept, mind),
    }
}

/// Iterate entities the agent's mind says contain something, then keep the
/// ones whose world `Affordance` component declares the requested action type.
///
/// The "knows-about-it via Contains" gate is the legacy filter from the
/// pre-refactor `collect_resource_targets` and `collect_affordance_targets`:
/// it ensures the brain only plans against entities it has actually perceived.
fn enumerate_entities_with_affordance(
    action_type: ActionType,
    mind: &MindGraph,
    affordances: &Query<(&GlobalTransform, Option<&Affordance>)>,
) -> Vec<TargetCandidate> {
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for triple in mind.query(None, Some(Predicate::Contains), None) {
        let Node::Entity(entity) = triple.subject else {
            continue;
        };
        if !seen.insert(entity) {
            continue;
        }

        let Ok((transform, Some(affordance))) = affordances.get(entity) else {
            continue;
        };
        if affordance.action_type != action_type {
            continue;
        }

        candidates.push(TargetCandidate::Entity {
            entity,
            pos: transform.translation().truncate(),
        });
    }

    candidates
}

/// Iterate every perceived entity (anything the mind has an `IsA` belief
/// about) and keep the ones whose ontology trait inheritance includes
/// `trait_concept`.
///
/// Used by Attack and Bite to enumerate prey: perception writes
/// `(deer_42, IsA, Concept::Deer)` for every visible deer, and
/// `mind.has_trait` walks the IsA chain to discover that
/// `(Deer, HasTrait, Prey)` lives in cultural/intrinsic knowledge.
fn enumerate_entities_with_trait(
    trait_concept: Concept,
    mind: &MindGraph,
    affordances: &Query<(&GlobalTransform, Option<&Affordance>)>,
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

        let Ok((transform, _)) = affordances.get(entity) else {
            continue;
        };
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
