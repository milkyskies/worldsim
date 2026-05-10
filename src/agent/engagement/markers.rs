//! Per-kind engagement marker components.
//!
//! Each kind's plugin inserts both the generic [`Engaged`] component (used
//! by arbitration's commitment gate, which stays kind-agnostic) and the
//! kind-specific marker here. Bevy queries can then filter at compile
//! time — `Query<&mut PlanMemory, Without<EngagedHunt>>` rather than
//! runtime branching on `engaged.kind != EngagementKind::Hunt`.
//!
//! Invariant: an agent has at most one EngagedX marker at a time, and
//! [`Engaged`] exists iff some marker exists. Plugins insert/remove the
//! pair together; `check_invariants_system` panics on violation.
//!
//! [`Engaged`]: super::component::Engaged

use bevy::prelude::*;

use super::component::EngagementId;

#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct EngagedConverse(pub EngagementId);

#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct EngagedHunt(pub EngagementId);

#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct EngagedDevour(pub EngagementId);

#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct EngagedHarvest(pub EngagementId);

#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct EngagedFlee(pub EngagementId);

#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct EngagedSleep(pub EngagementId);
