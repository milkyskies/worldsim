//! Engagement-id minter shared across kinds. Each kind owns its own
//! payload registry (e.g. [`super::converse::ConverseRegistry`]); this
//! resource just hands out ids so two kinds can't collide.

use bevy::prelude::*;

use super::component::EngagementId;

#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct EngagementRegistry {
    next: u64,
}

impl EngagementRegistry {
    pub fn mint(&mut self) -> EngagementId {
        let id = EngagementId(self.next);
        self.next += 1;
        id
    }
}
