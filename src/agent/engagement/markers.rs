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

/// #743 invariant: an agent has at most one EngagedX marker at a time,
/// and the generic [`super::Engaged`] component exists iff some marker
/// exists. Run as a debug-only system so a buggy plugin that forgets to
/// remove its marker on engagement end gets caught at the next tick.
#[cfg(debug_assertions)]
pub fn check_engagement_marker_invariants(
    converse: Query<Entity, With<EngagedConverse>>,
    hunt: Query<Entity, With<EngagedHunt>>,
    devour: Query<Entity, With<EngagedDevour>>,
    harvest: Query<Entity, With<EngagedHarvest>>,
    flee: Query<Entity, With<EngagedFlee>>,
    sleep: Query<Entity, With<EngagedSleep>>,
) {
    use std::collections::HashMap;
    let mut counts: HashMap<Entity, u8> = HashMap::new();
    for e in converse.iter() {
        *counts.entry(e).or_default() += 1;
    }
    for e in hunt.iter() {
        *counts.entry(e).or_default() += 1;
    }
    for e in devour.iter() {
        *counts.entry(e).or_default() += 1;
    }
    for e in harvest.iter() {
        *counts.entry(e).or_default() += 1;
    }
    for e in flee.iter() {
        *counts.entry(e).or_default() += 1;
    }
    for e in sleep.iter() {
        *counts.entry(e).or_default() += 1;
    }
    for (entity, count) in counts {
        debug_assert!(
            count <= 1,
            "agent {entity:?} has {count} EngagedX markers — at most one is permitted",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;

    #[test]
    fn invariant_passes_with_single_marker() {
        let mut world = World::new();
        world.spawn(EngagedConverse(EngagementId(0)));
        let mut schedule = Schedule::default();
        schedule.add_systems(check_engagement_marker_invariants);
        schedule.run(&mut world);
    }

    #[test]
    #[should_panic(expected = "EngagedX markers")]
    fn invariant_panics_with_two_markers() {
        let mut world = World::new();
        world.spawn((
            EngagedConverse(EngagementId(0)),
            EngagedHunt(EngagementId(1)),
        ));
        let mut schedule = Schedule::default();
        schedule.add_systems(check_engagement_marker_invariants);
        schedule.run(&mut world);
    }
}
