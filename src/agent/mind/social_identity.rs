//! Per-agent social ledger: who I know, when I met them, what they're called.
//!
//! Reads: nothing — pure agent self-state, populated by recognition + social_perception.
//! Writes: SocialIdentity (this module is the canonical home for `Knows` /
//!         `Introduced` / `NameOf` data — those triples no longer exist).
//! Upstream: recognition (first-sight introduction), social_perception (name re-tag).
//! Downstream: ui (relationship panels), character_sheet, ToM, relationships, tests.

use crate::agent::mind::knowledge::AgentName;
use bevy::prelude::*;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct KnownAgent {
    pub introduced: bool,
    pub name: AgentName,
    pub first_seen_tick: u64,
}

#[derive(Component, Debug, Clone, Default)]
pub struct SocialIdentity {
    known: HashMap<Entity, KnownAgent>,
}

impl SocialIdentity {
    pub fn knows(&self, other: Entity) -> bool {
        self.known.contains_key(&other)
    }

    pub fn is_introduced(&self, other: Entity) -> bool {
        self.known.get(&other).is_some_and(|k| k.introduced)
    }

    pub fn name_of(&self, other: Entity) -> Option<&AgentName> {
        self.known.get(&other).map(|k| &k.name)
    }

    pub fn get(&self, other: Entity) -> Option<&KnownAgent> {
        self.known.get(&other)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Entity, &KnownAgent)> {
        self.known.iter()
    }

    pub fn known_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.known.keys().copied()
    }

    pub fn len(&self) -> usize {
        self.known.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// Insert or refresh an acquaintance. Used on first-sight introduction
    /// (when both parties have been formally introduced) and on name re-tag.
    pub fn introduce(&mut self, other: Entity, name: AgentName, tick: u64) {
        self.known
            .entry(other)
            .and_modify(|k| {
                k.introduced = true;
                k.name = name.clone();
            })
            .or_insert(KnownAgent {
                introduced: true,
                name,
                first_seen_tick: tick,
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(bits: u64) -> Entity {
        Entity::from_bits(bits)
    }

    #[test]
    fn introduce_records_known_and_introduced() {
        let mut id = SocialIdentity::default();
        id.introduce(entity(1), AgentName("Bob".into()), 100);
        assert!(id.knows(entity(1)));
        assert!(id.is_introduced(entity(1)));
        assert_eq!(id.name_of(entity(1)).map(|n| n.0.as_str()), Some("Bob"));
    }

    #[test]
    fn second_introduce_refreshes_name_keeps_first_seen() {
        let mut id = SocialIdentity::default();
        id.introduce(entity(1), AgentName("Bob".into()), 100);
        id.introduce(entity(1), AgentName("Robert".into()), 500);
        assert_eq!(id.get(entity(1)).map(|k| k.first_seen_tick), Some(100));
        assert_eq!(id.name_of(entity(1)).map(|n| n.0.as_str()), Some("Robert"));
    }
}
