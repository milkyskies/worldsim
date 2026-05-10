//! `ExploredTiles` — per-agent record of which chunks the agent has
//! visited, and when.
//!
//! Replaces the old `(Chunk(..), Explored, Boolean(true))` MindGraph
//! triples. Set-membership is the natural shape — querying "have I been
//! here?" was paying the full triple-machinery cost (subject, predicate,
//! object, three indices) for one bit of information per chunk, plus a
//! timestamp for staleness scoring. This keeps both, in a much cheaper
//! representation, and removes the per-tile-visit churn from MindGraph.

use bevy::prelude::*;
use std::collections::HashMap;

/// Per-agent set of explored chunks plus the tick of the most-recent
/// visit per chunk. Staleness scoring (Explore / LookFor target picker)
/// reads `last_visit_tick`; presence-only checks (theory-of-mind topic
/// inference) read `is_explored`.
#[derive(Component, Reflect, Default, Debug)]
#[reflect(Component)]
pub struct ExploredTiles {
    visits: HashMap<(i32, i32), u64>,
}

impl ExploredTiles {
    pub fn mark_explored(&mut self, chunk: (i32, i32), tick: u64) {
        self.visits.insert(chunk, tick);
    }

    pub fn is_explored(&self, chunk: (i32, i32)) -> bool {
        self.visits.contains_key(&chunk)
    }

    pub fn last_visit_tick(&self, chunk: (i32, i32)) -> Option<u64> {
        self.visits.get(&chunk).copied()
    }

    pub fn iter_explored(&self) -> impl Iterator<Item = ((i32, i32), u64)> + '_ {
        self.visits.iter().map(|(&k, &v)| (k, v))
    }

    pub fn len(&self) -> usize {
        self.visits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visits.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_then_query_returns_true() {
        let mut ex = ExploredTiles::default();
        ex.mark_explored((3, 4), 100);
        assert!(ex.is_explored((3, 4)));
        assert_eq!(ex.last_visit_tick((3, 4)), Some(100));
    }

    #[test]
    fn unvisited_chunk_returns_false() {
        let ex = ExploredTiles::default();
        assert!(!ex.is_explored((0, 0)));
        assert_eq!(ex.last_visit_tick((0, 0)), None);
    }

    #[test]
    fn revisiting_updates_tick() {
        let mut ex = ExploredTiles::default();
        ex.mark_explored((1, 1), 50);
        ex.mark_explored((1, 1), 200);
        assert_eq!(ex.last_visit_tick((1, 1)), Some(200));
        assert_eq!(ex.len(), 1);
    }
}
