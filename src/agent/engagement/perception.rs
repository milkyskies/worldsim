//! `perceive_engagements`: write `(observed, EngagedWith, peer)` into
//! the observer's MindGraph so other systems can read engagement state
//! through the standard belief path.

use bevy::prelude::*;

use super::component::{Engaged, EngagementKind};
use super::converse::ConverseRegistry;
use crate::agent::Agent;
use crate::agent::mind::knowledge::{Metadata, MindGraph, Node, Predicate, Triple, Value};
use crate::agent::mind::perception::VisibleObjects;
use crate::core::tick::TickCount;

/// Per-agent stagger interval. Engagements last seconds-to-minutes;
/// 60-tick (~1 game-minute) refresh is plenty for the busy-partner
/// filter to react and keeps the per-tick MindGraph mutation budget
/// down.
const PERCEPTION_INTERVAL_TICKS: u64 = 60;

pub fn perceive_engagements(
    mut observers: Query<(Entity, &VisibleObjects, &mut MindGraph), With<Agent>>,
    engaged_lookup: Query<&Engaged>,
    converse_registry: Res<ConverseRegistry>,
    tick: Res<TickCount>,
) {
    let now = tick.current;
    for (observer, visible, mut mind) in observers.iter_mut() {
        if !tick.should_run(observer, PERCEPTION_INTERVAL_TICKS) {
            continue;
        }
        for &observed in &visible.entities {
            if observed == observer {
                continue;
            }
            let Ok(engaged) = engaged_lookup.get(observed) else {
                continue;
            };
            // Each kind owns peer resolution; arbitration / perception
            // stay kind-agnostic. New kinds add their own arm.
            let participants: &[Entity] = match engaged.kind {
                EngagementKind::Converse => match converse_registry.get(engaged.id) {
                    Some(c) => &c.participants,
                    None => continue,
                },
            };
            for &peer in participants {
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
