//! Centralised social graph: directed edges between agent entities
//! carry trust / affection / respect / power_balance / kind / last
//! interaction tick. Source of truth for relationship state.
//!
//! Reads: nothing (pure data resource)
//! Writes: nothing (consumers mutate edges via the resource API)
//! Upstream: psyche::relationships (lifecycle), mind::recognition (introduction)
//! Downstream: every system that previously read Affection/Trust/Respect
//!             from per-agent MindGraph triples
//!
//! Edges are directed: `(Alice, Bob)` and `(Bob, Alice)` are distinct.
//! Asymmetry matters — Alice may adore Bob while Bob barely knows her.
//! Lives on a Resource, not on either entity, so demoted/despawned
//! agents don't lose their relationships (LOD prep, see #752).

use bevy::prelude::*;
use std::collections::HashMap;

/// Neutral starting value for trust / affection / respect — also the
/// asymptote relationship decay pulls back toward.
pub const NEUTRAL: f32 = 0.5;

/// Categorical label inferred from the edge's quantitative dimensions.
/// Updated by `recognition::classify_relationship`; consumed by UI and
/// behavioural systems that branch on relationship type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default, serde::Serialize)]
pub enum RelationshipKind {
    #[default]
    Stranger,
    Acquaintance,
    Friend,
    Rival,
    Kin,
    Mate,
}

/// One directed edge in the social graph: how `observer` feels about
/// `target`. Symmetric companion may have entirely different values.
#[derive(Debug, Clone, Copy, Reflect, serde::Serialize)]
pub struct RelationshipEdge {
    pub affection: f32,
    pub trust: f32,
    pub respect: f32,
    /// -1.0 = subordinate to target, +1.0 = dominant over target.
    pub power_balance: f32,
    pub last_interaction_tick: u64,
    pub kind: RelationshipKind,
}

impl Default for RelationshipEdge {
    fn default() -> Self {
        Self {
            affection: NEUTRAL,
            trust: NEUTRAL,
            respect: NEUTRAL,
            power_balance: 0.0,
            last_interaction_tick: 0,
            kind: RelationshipKind::Stranger,
        }
    }
}

impl RelationshipEdge {
    /// New edge initialised at neutral with a custom affection baseline
    /// (recognition uses the personality-derived first-impression value).
    pub fn with_baseline_affection(affection: f32, tick: u64) -> Self {
        Self {
            affection: affection.clamp(0.0, 1.0),
            last_interaction_tick: tick,
            ..Self::default()
        }
    }
}

/// Centralised store of every directed relationship in the world.
/// Survives agent entity demotion / despawn — that's the whole point.
#[derive(Resource, Debug, Default)]
pub struct SocialGraph {
    edges: HashMap<(Entity, Entity), RelationshipEdge>,
}

impl SocialGraph {
    /// Read `observer`'s edge toward `target`. `None` if they've never
    /// interacted.
    pub fn get(&self, observer: Entity, target: Entity) -> Option<&RelationshipEdge> {
        self.edges.get(&(observer, target))
    }

    /// Mutable access to an existing edge — `None` if not yet introduced.
    pub fn get_mut(&mut self, observer: Entity, target: Entity) -> Option<&mut RelationshipEdge> {
        self.edges.get_mut(&(observer, target))
    }

    /// True iff `observer` has any edge data on `target`.
    pub fn knows(&self, observer: Entity, target: Entity) -> bool {
        self.edges.contains_key(&(observer, target))
    }

    /// Insert or overwrite an edge. Used by introduction (`recognition`)
    /// and tests that seed specific relationship state.
    pub fn set(&mut self, observer: Entity, target: Entity, edge: RelationshipEdge) {
        self.edges.insert((observer, target), edge);
    }

    /// Read-or-default helper: returns the stored edge or a stranger
    /// default. Doesn't insert — keeps the graph sparse for never-met
    /// pairs.
    pub fn get_or_default(&self, observer: Entity, target: Entity) -> RelationshipEdge {
        self.edges
            .get(&(observer, target))
            .copied()
            .unwrap_or_default()
    }

    /// Affection on the directed edge, defaulting to `NEUTRAL` for
    /// never-introduced pairs. Single most-called accessor — matches
    /// the previous `mind.get(target, Affection)` shape.
    pub fn affection(&self, observer: Entity, target: Entity) -> f32 {
        self.get(observer, target)
            .map(|e| e.affection)
            .unwrap_or(NEUTRAL)
    }

    pub fn trust(&self, observer: Entity, target: Entity) -> f32 {
        self.get(observer, target)
            .map(|e| e.trust)
            .unwrap_or(NEUTRAL)
    }

    pub fn respect(&self, observer: Entity, target: Entity) -> f32 {
        self.get(observer, target)
            .map(|e| e.respect)
            .unwrap_or(NEUTRAL)
    }

    /// Iterate every directed edge — used by the decay system, UI
    /// inspection, and migration tooling. Yields `(observer, target, edge)`.
    pub fn iter(&self) -> impl Iterator<Item = (Entity, Entity, &RelationshipEdge)> {
        self.edges.iter().map(|((o, t), e)| (*o, *t, e))
    }

    /// Mutable iterator counterpart — the decay system uses this.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, Entity, &mut RelationshipEdge)> {
        self.edges.iter_mut().map(|((o, t), e)| (*o, *t, e))
    }

    /// Total number of directed edges. Useful for tests and observability.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Drop every edge that touches `entity` on either endpoint. Not
    /// called during normal demotion (the whole point is that demoted
    /// agents keep their relationships); reserved for true death /
    /// permanent-removal flows.
    pub fn forget_agent(&mut self, entity: Entity) {
        self.edges.retain(|(o, t), _| *o != entity && *t != entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(id: u32) -> Entity {
        Entity::from_bits(id as u64)
    }

    #[test]
    fn add_and_get_edge_roundtrips() {
        let mut graph = SocialGraph::default();
        let alice = entity(1);
        let bob = entity(2);
        let mut edge = RelationshipEdge::default();
        edge.affection = 0.7;
        graph.set(alice, bob, edge);

        let stored = graph.get(alice, bob).expect("edge must round-trip");
        assert!((stored.affection - 0.7).abs() < 1e-6);
    }

    #[test]
    fn directed_edges_track_asymmetry() {
        let mut graph = SocialGraph::default();
        let alice = entity(1);
        let bob = entity(2);

        graph.set(
            alice,
            bob,
            RelationshipEdge {
                affection: 0.8,
                ..Default::default()
            },
        );
        graph.set(
            bob,
            alice,
            RelationshipEdge {
                affection: 0.2,
                ..Default::default()
            },
        );

        assert!((graph.affection(alice, bob) - 0.8).abs() < 1e-6);
        assert!((graph.affection(bob, alice) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn unknown_pair_returns_neutral() {
        let graph = SocialGraph::default();
        assert!(!graph.knows(entity(1), entity(2)));
        assert!((graph.affection(entity(1), entity(2)) - NEUTRAL).abs() < 1e-6);
        assert!(graph.get(entity(1), entity(2)).is_none());
    }

    #[test]
    fn updating_one_directed_edge_does_not_affect_the_reverse() {
        let mut graph = SocialGraph::default();
        let alice = entity(1);
        let bob = entity(2);

        graph.set(alice, bob, RelationshipEdge::default());
        graph.set(bob, alice, RelationshipEdge::default());

        graph.get_mut(alice, bob).unwrap().affection = 0.9;

        assert!((graph.affection(alice, bob) - 0.9).abs() < 1e-6);
        assert!((graph.affection(bob, alice) - NEUTRAL).abs() < 1e-6);
    }

    #[test]
    fn forget_agent_removes_both_sides_of_every_edge() {
        let mut graph = SocialGraph::default();
        let alice = entity(1);
        let bob = entity(2);
        let charlie = entity(3);

        graph.set(alice, bob, RelationshipEdge::default());
        graph.set(bob, alice, RelationshipEdge::default());
        graph.set(charlie, alice, RelationshipEdge::default());
        graph.set(bob, charlie, RelationshipEdge::default());
        assert_eq!(graph.edge_count(), 4);

        graph.forget_agent(alice);
        assert_eq!(graph.edge_count(), 1);
        assert!(graph.knows(bob, charlie));
    }

    /// The headline LOD-survival property: an edge stored on the
    /// SocialGraph remains intact if one of its endpoints' Bevy entity
    /// is destroyed elsewhere. The `Entity` value lingers as a key —
    /// entity-id reuse / re-introduction is layered on top by #752.
    #[test]
    fn edge_data_outlives_entity_in_resource_only_lifecycle() {
        let mut graph = SocialGraph::default();
        let alice = entity(1);
        let bob = entity(2);
        let edge = RelationshipEdge {
            affection: 0.85,
            trust: 0.7,
            ..Default::default()
        };
        graph.set(alice, bob, edge);

        // Simulate Alice's entity being despawned elsewhere — the edge
        // is unaffected because it lives on the resource.
        let after = graph.get(alice, bob).copied().unwrap();
        assert!((after.affection - 0.85).abs() < 1e-6);
        assert!((after.trust - 0.7).abs() < 1e-6);
    }
}
