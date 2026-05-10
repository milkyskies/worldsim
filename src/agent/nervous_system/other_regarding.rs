//! Other-regarding urgency: `urgency = perceived_deficit × Σ channel(ctx)`.
//!
//! Reads: AffectiveToM, SocialGraph, OtherRegardingChannels
//! Writes: nothing (returns urgency values to the urgency loop)
//! Upstream: mind::affective_tom, psyche::social_graph
//! Downstream: nervous_system::urgency (Compassion emission)
//!
//! Each channel is a pluggable `fn(&ChannelContext) -> f32` capturing one
//! reason "their bad state → my urgency" can become true. New channels
//! call `OtherRegardingChannels::register` in their plugin Startup.

use bevy::prelude::*;

use crate::agent::nervous_system::urgency::UrgencySource;
use crate::agent::psyche::social_graph::{NEUTRAL, SocialGraph};

/// Inputs available to every channel function. Carries pointers, not
/// owned data — channels read their slice of world state and return a
/// scalar contribution. Add a new field here when a future channel
/// needs additional world state (kin store, group memberships, etc.).
pub struct ChannelContext<'a> {
    pub observer: Entity,
    pub target: Entity,
    pub drive: UrgencySource,
    pub social_graph: &'a SocialGraph,
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
    /// Register a channel under a stable name. Idempotent — calling
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

    /// Iterate `(name, contribution)` pairs in registration order — used
    /// for diagnostic output (character sheet, decision-trace).
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
    let affection = ctx.social_graph.affection(ctx.observer, ctx.target);
    ((affection - NEUTRAL).max(0.0) / (1.0 - NEUTRAL)).clamp(0.0, 1.0)
}

/// Registers the default channels shipped with this primitive. Called
/// from the nervous-system plugin builder so tests that bypass Startup
/// (TestWorld) still see the affection channel registered.
pub fn register_default_channels(channels: &mut OtherRegardingChannels) {
    channels.register("affection", affection_channel);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::psyche::social_graph::RelationshipEdge;

    fn test_entity(id: u32) -> Entity {
        Entity::from_bits(id as u64)
    }

    fn graph_with_affection(observer: Entity, target: Entity, value: f32) -> SocialGraph {
        let mut graph = SocialGraph::default();
        graph.set(
            observer,
            target,
            RelationshipEdge {
                affection: value,
                ..Default::default()
            },
        );
        graph
    }

    fn ctx<'a>(observer: Entity, target: Entity, graph: &'a SocialGraph) -> ChannelContext<'a> {
        ChannelContext {
            observer,
            target,
            drive: UrgencySource::Compassion,
            social_graph: graph,
        }
    }

    #[test]
    fn affection_channel_zero_at_or_below_neutral() {
        let observer = test_entity(2);
        let target = test_entity(1);
        let graph = graph_with_affection(observer, target, 0.5);
        assert!(affection_channel(&ctx(observer, target, &graph)).abs() < 1e-6);

        let graph_low = graph_with_affection(observer, target, 0.2);
        assert!(affection_channel(&ctx(observer, target, &graph_low)).abs() < 1e-6);
    }

    #[test]
    fn affection_channel_full_at_max_bond() {
        let observer = test_entity(2);
        let target = test_entity(1);
        let graph = graph_with_affection(observer, target, 1.0);
        assert!((affection_channel(&ctx(observer, target, &graph)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn affection_channel_scales_above_neutral() {
        let observer = test_entity(2);
        let target = test_entity(1);
        let weak = graph_with_affection(observer, target, 0.6);
        let strong = graph_with_affection(observer, target, 0.9);
        assert!(
            affection_channel(&ctx(observer, target, &strong))
                > affection_channel(&ctx(observer, target, &weak))
        );
    }

    #[test]
    fn no_edge_record_returns_zero() {
        let graph = SocialGraph::default();
        let value = affection_channel(&ctx(test_entity(2), test_entity(1), &graph));
        assert!(value.abs() < 1e-6);
    }

    #[test]
    fn channels_sum_contributions_across_registered_entries() {
        let mut channels = OtherRegardingChannels::default();
        channels.register("constant_half", |_| 0.5);
        channels.register("constant_quarter", |_| 0.25);

        let graph = SocialGraph::default();
        let context = ctx(test_entity(2), test_entity(1), &graph);
        assert!((channels.total_contribution(&context) - 0.75).abs() < 1e-6);
        assert_eq!(channels.channel_count(), 2);
    }

    #[test]
    fn registering_same_name_replaces_rather_than_double_counts() {
        let mut channels = OtherRegardingChannels::default();
        channels.register("c", |_| 0.5);
        channels.register("c", |_| 0.1);

        let graph = SocialGraph::default();
        let context = ctx(test_entity(2), test_entity(1), &graph);
        assert!((channels.total_contribution(&context) - 0.1).abs() < 1e-6);
        assert_eq!(channels.channel_count(), 1);
    }

    #[test]
    fn breakdown_yields_named_per_channel_contributions_in_registration_order() {
        let mut channels = OtherRegardingChannels::default();
        channels.register("a", |_| 0.3);
        channels.register("b", |_| 0.7);

        let graph = SocialGraph::default();
        let context = ctx(test_entity(2), test_entity(1), &graph);
        let names: Vec<&'static str> = channels.breakdown(&context).map(|(n, _)| n).collect();
        let total: f32 = channels.breakdown(&context).map(|(_, v)| v).sum();
        assert_eq!(names, vec!["a", "b"]);
        assert!((total - 1.0).abs() < 1e-6);
    }
}
