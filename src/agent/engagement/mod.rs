//! Generic engagement primitive — persistent multi-agent interactions
//! owned by per-kind plugins (Converse today; Hunt / Tend / Court land
//! as their own sub-modules).

pub mod component;
pub mod converse;
mod perception;
pub mod registry;

pub use component::{Engaged, EngagementEndReason, EngagementId, EngagementKind};
pub use registry::EngagementRegistry;

use bevy::prelude::*;

use crate::core::not_paused;

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
