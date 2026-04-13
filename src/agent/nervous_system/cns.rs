//! Central nervous system: exposes the current urgency list and sleep-wake trigger.
//!
//! Reads: nervous_system::urgency writes `urgencies` and `sleep_wake_trigger`.
//! Writes: CentralNervousSystem (via upstream systems)
//! Upstream: nervous_system::urgency (produces Urgency values)
//! Downstream: brains::{survival, rational, emotional} (read urgencies directly)

use super::urgency::{Urgency, UrgencySource};
use bevy::prelude::*;

#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct CentralNervousSystem {
    pub urgencies: Vec<Urgency>,
    /// Set by `generate_urgency` when a drive's raw input has crossed its
    /// `sleep_wake_threshold`. The survival brain reads this to decide
    /// whether a sleeping agent should be roused. Independent from
    /// `urgencies` because it compares against the *pre-gated* input — the
    /// biological wake pathway sees the raw signal, not the alertness-
    /// dampened urgency.
    pub sleep_wake_trigger: Option<UrgencySource>,
}

impl CentralNervousSystem {
    /// Look up the current urgency value for a given source, or 0.0 if
    /// that source isn't in the list.
    pub fn urgency_value(&self, source: UrgencySource) -> f32 {
        self.urgencies
            .iter()
            .find(|u| u.source == source)
            .map(|u| u.value)
            .unwrap_or(0.0)
    }
}

/// Base priority a verbal-commitment plan contributes. Kept attractive
/// but not so attractive that it overrides life-threatening needs like
/// hunger or fear. Post-#487 this flows through `UrgencySource::Commitment`
/// in `generate_urgency` instead of a standalone goal field.
pub const VERBAL_COMMITMENT_PRIORITY_BASE: f32 = 0.4;
/// Multiplier on conscientiousness added on top of the base priority —
/// reliable agents hold verbal commitments tighter.
pub const VERBAL_COMMITMENT_PRIORITY_BONUS: f32 = 0.3;

/// Listener-side demand reduction: when the agent's MindGraph already
/// contains a `(?peer, Committed, my_goal_concept)` triple, a goal's
/// priority is multiplied by `1.0 - PEER_COMMITMENT_DISCOUNT`. The
/// other agent has volunteered to handle this concept, so the listener
/// drops their own competing pursuit of it. This is the "5 cold agents
/// build 1 shelter" coordination behaviour from #338.
pub const PEER_COMMITMENT_DISCOUNT: f32 = 0.4;

/// Returns true if any peer in `mind` has a `Committed` triple
/// targeting the given concept. Self-committed triples (where the
/// subject is `Self_`) are ignored — those are the agent's *own*
/// commitments, not peer broadcasts.
pub fn peer_committed_to(
    mind: &crate::agent::mind::knowledge::MindGraph,
    concept: crate::agent::mind::knowledge::Concept,
) -> bool {
    use crate::agent::mind::knowledge::{Node, Predicate, Value};
    let triples = mind.query(
        None,
        Some(Predicate::Committed),
        Some(&Value::Concept(concept)),
    );
    triples.iter().any(|t| !matches!(t.subject, Node::Self_))
}
