//! `perceive_engagements`: write `(observed, EngagedWith, peer)` triples
//! into the observer's MindGraph for every visible agent who carries an
//! [`Engaged`] component.
//!
//! Lets `seek_social_initiation` filter visibly-busy partners through
//! the standard MindGraph path instead of a special-case Component
//! query — and gives downstream systems (kin reactions, "wait before
//! joining a conversation") a perceivable handle on engagements they
//! aren't part of.

use bevy::prelude::*;

use super::component::Engaged;
use super::converse::ConverseRegistry;
use crate::agent::Agent;
use crate::agent::mind::knowledge::{Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::core::tick::TickCount;

pub fn perceive_engagements(
    mut observers: Query<(Entity, &VisibleObjects, &mut MindGraph), With<Agent>>,
    engaged_lookup: Query<&Engaged>,
    converse_registry: Res<ConverseRegistry>,
    tick: Res<TickCount>,
) {
    let now = tick.current;
    for (observer, visible, mut mind) in observers.iter_mut() {
        for &observed in &visible.entities {
            if observed == observer {
                continue;
            }
            let Ok(engaged) = engaged_lookup.get(observed) else {
                continue;
            };
            // Resolve peers via the kind-specific registry. Today only
            // Converse exists.
            let peers: Vec<Entity> = converse_registry
                .get(engaged.id)
                .map(|c| c.participants.to_vec())
                .unwrap_or_default();
            for peer in peers {
                if peer == observed {
                    continue;
                }
                mind.assert(Triple::with_meta(
                    Node::Entity(observed),
                    Predicate::EngagedWith,
                    Value::Entity(peer),
                    Metadata::perception(now),
                ));
            }
        }
    }
}
