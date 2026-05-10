//! Other-regarding urgency: `urgency = perceived_deficit × Σ channel(ctx)`.
//!
//! Reads: AffectiveToM, MindGraph (affection store), OtherRegardingChannels
//! Writes: nothing (returns urgency values to the urgency loop)
//! Upstream: mind::affective_tom, psyche::relationships
//! Downstream: nervous_system::urgency (Compassion emission)
//!
//! Each channel is a pluggable `fn(&ChannelContext) -> f32` capturing one
//! reason "their bad state → my urgency" can become true. New channels
//! call `OtherRegardingChannels::register` in their plugin Startup.

use bevy::prelude::*;

use crate::agent::mind::knowledge::MindGraph;
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::agent::psyche::relationships::{NEUTRAL, affection_toward};

/// Inputs available to every channel function. Carries pointers, not
/// owned data — channels read their slice of world state and return a
/// scalar contribution. Add a new field here when a future channel
/// needs additional world state (kin store, group memberships, etc.).
pub struct ChannelContext<'a> {
    pub observer: Entity,
    pub target: Entity,
    pub drive: UrgencySource,
    /// Observer's mind — affection lookups, kin triples, etc.
    pub mind: &'a MindGraph,
}

/// Function pointer for a single motivation channel. Returns its scalar
/// contribution toward the observer's urgency about the target.
/// Contributions are summed across registered channels.
pub type ChannelFn = fn(&ChannelContext) -> f32;

/// Registry of channel contributors. Plugin Startup hooks call
/// `register` to plug their channel in. The urgency loop calls
/// `total_contribution` per (observer, target, drive) tuple.
#[derive(Resource, Default)]
pub struct OtherRegardingChannels {
    channels: Vec<(&'static str, ChannelFn)>,
}

impl OtherRegardingChannels {
    /// Register a channel under a stable name. The name is used by
    /// debugging tools; the function does the work. Idempotent — calling
    /// twice with the same name replaces the existing entry rather than
    /// double-counting.
    pub fn register(&mut self, name: &'static str, channel: ChannelFn) {
        if let Some(existing) = self.channels.iter_mut().find(|(n, _)| *n == name) {
            existing.1 = channel;
        } else {
            self.channels.push((name, channel));
        }
    }

    /// Sum of every registered channel's contribution. Returns 0 for
    /// no-channels-registered, which suppresses the corresponding
    /// urgency entirely.
    pub fn total_contribution(&self, ctx: &ChannelContext) -> f32 {
        self.channels.iter().map(|(_, f)| f(ctx)).sum()
    }

    /// Iterate `(name, contribution)` pairs — used for diagnostic output
    /// (the character sheet UI, decision-trace explanations, etc.).
    pub fn breakdown<'a>(
        &'a self,
        ctx: &'a ChannelContext<'a>,
    ) -> impl Iterator<Item = (&'static str, f32)> + 'a {
        self.channels.iter().map(|(name, f)| (*name, f(ctx)))
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }
}

/// Channel 1 — affection. Linear above the relationship-store neutral:
/// neutral or below contributes nothing, fully-bonded (1.0) contributes
/// 1.0. The above-neutral gate keeps disliked strangers from generating
/// compassion urgency.
pub fn affection_channel(ctx: &ChannelContext) -> f32 {
    let affection = affection_toward(ctx.mind, ctx.target);
    ((affection - NEUTRAL).max(0.0) / (1.0 - NEUTRAL)).clamp(0.0, 1.0)
}

/// Plugin Startup hook: registers the default channels shipped with this
/// primitive. Future-channel PRs add their own registrations alongside.
pub fn register_default_channels(mut channels: ResMut<OtherRegardingChannels>) {
    channels.register("affection", affection_channel);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mind::knowledge::{
        Metadata, Node, Predicate, Quantity, Triple, Value, setup_ontology,
    };

    fn ctx_with_affection<'a>(
        observer: Entity,
        target: Entity,
        mind: &'a MindGraph,
    ) -> ChannelContext<'a> {
        ChannelContext {
            observer,
            target,
            drive: UrgencySource::Compassion,
            mind,
        }
    }

    fn mind_with_affection(target: Entity, value: f32) -> MindGraph {
        // The relationships store writes Entity-keyed affection triples;
        // the channel reads through that subject form.
        let mut mind = MindGraph::new(setup_ontology());
        mind.assert(Triple::with_meta(
            Node::Entity(target),
            Predicate::Affection,
            Value::Quantity(Quantity::Exact(value)),
            Metadata::default(),
        ));
        mind
    }

    fn test_entity(id: u32) -> Entity {
        Entity::from_bits(id as u64)
    }

    #[test]
    fn affection_channel_zero_at_or_below_neutral() {
        let target = test_entity(1);
        let mind = mind_with_affection(target, 0.5);
        let ctx = ctx_with_affection(test_entity(2), target, &mind);
        assert!(affection_channel(&ctx).abs() < 1e-6);

        let mind_low = mind_with_affection(target, 0.2);
        let ctx_low = ctx_with_affection(test_entity(2), target, &mind_low);
        assert!(affection_channel(&ctx_low).abs() < 1e-6);
    }

    #[test]
    fn affection_channel_full_at_max_bond() {
        let target = test_entity(1);
        let mind = mind_with_affection(target, 1.0);
        let ctx = ctx_with_affection(test_entity(2), target, &mind);
        assert!((affection_channel(&ctx) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn affection_channel_scales_above_neutral() {
        let target = test_entity(1);
        let weak = mind_with_affection(target, 0.6);
        let strong = mind_with_affection(target, 0.9);
        let ctx_weak = ctx_with_affection(test_entity(2), target, &weak);
        let ctx_strong = ctx_with_affection(test_entity(2), target, &strong);
        assert!(affection_channel(&ctx_strong) > affection_channel(&ctx_weak));
    }

    #[test]
    fn no_affection_record_returns_zero() {
        let target = test_entity(1);
        let mind = MindGraph::new(setup_ontology());
        let ctx = ctx_with_affection(test_entity(2), target, &mind);
        assert!(affection_channel(&ctx).abs() < 1e-6);
    }

    #[test]
    fn channels_sum_contributions_across_registered_entries() {
        let mut channels = OtherRegardingChannels::default();
        channels.register("constant_half", |_| 0.5);
        channels.register("constant_quarter", |_| 0.25);

        let target = test_entity(1);
        let mind = MindGraph::new(setup_ontology());
        let ctx = ctx_with_affection(test_entity(2), target, &mind);
        assert!((channels.total_contribution(&ctx) - 0.75).abs() < 1e-6);
        assert_eq!(channels.channel_count(), 2);
    }

    #[test]
    fn registering_same_name_replaces_rather_than_double_counts() {
        let mut channels = OtherRegardingChannels::default();
        channels.register("c", |_| 0.5);
        channels.register("c", |_| 0.1);

        let target = test_entity(1);
        let mind = MindGraph::new(setup_ontology());
        let ctx = ctx_with_affection(test_entity(2), target, &mind);
        assert!((channels.total_contribution(&ctx) - 0.1).abs() < 1e-6);
        assert_eq!(channels.channel_count(), 1);
    }

    #[test]
    fn breakdown_yields_named_per_channel_contributions() {
        let mut channels = OtherRegardingChannels::default();
        channels.register("a", |_| 0.3);
        channels.register("b", |_| 0.7);

        let target = test_entity(1);
        let mind = MindGraph::new(setup_ontology());
        let ctx = ctx_with_affection(test_entity(2), target, &mind);

        let names: Vec<&'static str> = channels.breakdown(&ctx).map(|(n, _)| n).collect();
        let total: f32 = channels.breakdown(&ctx).map(|(_, v)| v).sum();
        assert_eq!(names, vec!["a", "b"]);
        assert!((total - 1.0).abs() < 1e-6);
    }
}
