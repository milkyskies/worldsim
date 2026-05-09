//! Generic engagement primitive: persistent, multi-tick interactions
//! between two or more agents owned end-to-end by a kind-specific
//! plugin.
//!
//! Conversation is the first kind. Hunt, Tend, Court, Nurse, etc. ship
//! as their own kinds against this primitive.

pub mod component;
pub mod converse;
mod perception;
pub mod registry;

pub use component::{Engaged, EngagementEndReason, EngagementId, EngagementKind};
pub use registry::EngagementRegistry;

use bevy::prelude::*;

use crate::core::not_paused;

/// Top-level plugin for the engagement primitive. Hosts the id minter
/// and the perception system that lets other agents see who's engaged
/// with whom; each kind registers its own [`Plugin`] (e.g.
/// [`converse::ConversePlugin`]) for inner-loop systems.
pub struct EngagementPlugin;

impl Plugin for EngagementPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EngagementRegistry>()
            .register_type::<Engaged>()
            .register_type::<EngagementId>()
            .register_type::<EngagementKind>()
            .register_type::<EngagementEndReason>()
            .add_plugins(converse::ConversePlugin)
            .add_systems(
                FixedUpdate,
                perception::perceive_engagements
                    .in_set(crate::core::PerfBucket::Perception)
                    .in_set(crate::core::PerfSubBucket::PerceptionSocial)
                    .after(crate::agent::mind::social_perception::perceive_other_agents)
                    .run_if(not_paused),
            );
    }
}
