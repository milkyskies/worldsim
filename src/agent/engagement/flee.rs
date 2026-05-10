//! `EngagementKind::Flee` — sustained threat-tracking pursuit-away.
//!
//! Replaces the standalone `Flee` action's brain re-decides-every-tick
//! pattern (17,722 `ActionStarted` events in the last seed-42 12-game-hour
//! baseline). The engagement re-samples the threat's *current* position
//! each tick and emits a flee-Walk away from it; exits when the threat
//! drops out of perception, the agent corners, exhausts, or rage flips
//! to a future Fight engagement.

use bevy::ecs::world::EntityWorldMut;
use bevy::prelude::*;

use super::component::{Engaged, EngagementEndReason, EngagementId, EngagementKind};
use super::markers::EngagedFlee;
use super::registry::EngagementRegistry;
use crate::agent::Agent;
use crate::agent::actions::registry::{ActionState, ActiveActions};
use crate::agent::actions::types::ActionType;
use crate::agent::events::{
    EngagementBeatPayload, FailureReason, SimEvent, SimEventKind,
};
use crate::agent::mind::perception::VisibleObjects;
use crate::core::not_paused;
use crate::core::tick::TickCount;

pub const TARGET_LOST_GRACE_TICKS: u64 = 60;
pub const SAFE_AERO_THRESHOLD: f32 = 0.05;

#[derive(Debug, Clone, Reflect)]
pub struct FleeSession {
    pub id: EngagementId,
    pub fleer: Entity,
    pub threat: Entity,
    pub started_at: u64,
    pub last_seen_tick: u64,
    pub steps: u32,
}

impl FleeSession {
    pub fn new(id: EngagementId, fleer: Entity, threat: Entity, tick: u64) -> Self {
        Self {
            id,
            fleer,
            threat,
            started_at: tick,
            last_seen_tick: tick,
            steps: 0,
        }
    }
}

#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct FleeRegistry {
    pub sessions: std::collections::HashMap<EngagementId, FleeSession>,
}

impl FleeRegistry {
    pub fn start(
        &mut self,
        ids: &mut EngagementRegistry,
        fleer: Entity,
        threat: Entity,
        tick: u64,
    ) -> EngagementId {
        let id = ids.mint();
        self.sessions
            .insert(id, FleeSession::new(id, fleer, threat, tick));
        id
    }
}

pub struct FleePlugin;

impl Plugin for FleePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FleeRegistry>().add_systems(
            FixedUpdate,
            (
                process_initiate_flee
                    .after(crate::agent::nervous_system::execution::start_actions),
                drive_flee_engagement.after(process_initiate_flee),
                evaluate_flee_continuation.after(drive_flee_engagement),
            )
                .in_set(crate::core::PerfBucket::Brain)
                .run_if(not_paused),
        );
    }
}

struct RemoveFleeActions;

impl EntityCommand for RemoveFleeActions {
    fn apply(self, mut entity: EntityWorldMut) {
        if let Some(mut active) = entity.get_mut::<ActiveActions>() {
            active.remove(ActionType::InitiateFlee);
            active.remove(ActionType::Flee);
        }
    }
}

pub fn process_initiate_flee(
    mut commands: Commands,
    mut registry: ResMut<FleeRegistry>,
    mut id_minter: ResMut<EngagementRegistry>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
    engaged: Query<&Engaged, With<Agent>>,
    mut active_actions: Query<(Entity, &mut ActiveActions), With<Agent>>,
) {
    let pairs: Vec<(Entity, Option<Entity>)> = active_actions
        .iter()
        .filter_map(|(entity, active)| {
            active
                .get(ActionType::InitiateFlee)
                .map(|state| (entity, state.target_entity))
        })
        .collect();

    for (fleer, threat_opt) in pairs {
        let now = tick.current;

        // Already engaged in something? Flee preempts via the
        // ENGAGEMENT_BREAK_URGENCY gate at arbitration time, so by the
        // time we see this, the prior engagement should have been broken.
        // But if `Engaged` is still attached (stale frame), drop the
        // initiate; it'll be re-proposed next tick after the existing
        // engagement is cleared.
        if engaged.get(fleer).is_ok() {
            if let Ok((_, mut active)) = active_actions.get_mut(fleer) {
                active.remove(ActionType::InitiateFlee);
            }
            continue;
        }

        let Some(threat) = threat_opt else {
            if let Ok((_, mut active)) = active_actions.get_mut(fleer) {
                active.remove(ActionType::InitiateFlee);
            }
            sim_events.write(SimEvent::single(
                now,
                fleer,
                SimEventKind::ActionFailed {
                    agent: fleer,
                    action: ActionType::InitiateFlee,
                    reason: FailureReason::NoTarget,
                },
            ));
            continue;
        };

        let id = registry.start(&mut id_minter, fleer, threat, now);
        commands
            .entity(fleer)
            .insert((Engaged::new(EngagementKind::Flee, id), EngagedFlee(id)));

        if let Ok((_, mut active)) = active_actions.get_mut(fleer) {
            active.remove(ActionType::InitiateFlee);
        }
        sim_events.write(SimEvent::pair(
            now,
            fleer,
            threat,
            SimEventKind::EngagementStarted {
                kind: EngagementKind::Flee,
                engagement_id: id,
                participants: vec![fleer, threat],
            },
        ));
    }
}

pub fn drive_flee_engagement(
    mut registry: ResMut<FleeRegistry>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
    transforms: Query<&Transform>,
    visible: Query<&VisibleObjects>,
    mut active_actions: Query<&mut ActiveActions, With<Agent>>,
) {
    let now = tick.current;
    for session in registry.sessions.values_mut() {
        let perceives_threat = visible
            .get(session.fleer)
            .map(|v| v.entities.contains(&session.threat))
            .unwrap_or(false);
        if perceives_threat {
            session.last_seen_tick = now;
        }

        let threat_pos = transforms.get(session.threat).ok().map(|t| t.translation.truncate());

        if let Ok(mut active) = active_actions.get_mut(session.fleer) {
            let mut flee = ActionState::new(ActionType::Flee, now);
            flee.target_entity = Some(session.threat);
            flee.target_position = threat_pos;
            flee.locomotion_intensity = 1.0;
            active.insert(flee);
        }
        session.steps += 1;
        sim_events.write(SimEvent::single(
            now,
            session.fleer,
            SimEventKind::EngagementBeat {
                kind: EngagementKind::Flee,
                engagement_id: session.id,
                agent: session.fleer,
                payload: EngagementBeatPayload::Flee {
                    fleer: session.fleer,
                    threat: session.threat,
                    step_index: session.steps,
                },
            },
        ));
    }
}

pub fn evaluate_flee_continuation(
    mut commands: Commands,
    mut registry: ResMut<FleeRegistry>,
    mut sim_events: MessageWriter<SimEvent>,
    tick: Res<TickCount>,
    transforms: Query<&Transform>,
    physicals: Query<&crate::agent::body::needs::PhysicalNeeds>,
) {
    let mut to_remove: Vec<EngagementId> = Vec::new();
    for (id, session) in registry.sessions.iter() {
        let mut reason: Option<EngagementEndReason> = None;

        if transforms.get(session.threat).is_err() {
            reason = Some(EngagementEndReason::Natural);
        }
        if reason.is_none()
            && tick.current.saturating_sub(session.last_seen_tick) > TARGET_LOST_GRACE_TICKS
        {
            reason = Some(EngagementEndReason::OutOfRange);
        }
        if reason.is_none()
            && physicals
                .get(session.fleer)
                .map(|p| p.stamina.aerobic < SAFE_AERO_THRESHOLD)
                .unwrap_or(false)
        {
            reason = Some(EngagementEndReason::Other);
        }

        if let Some(reason) = reason {
            sim_events.write(SimEvent::pair(
                tick.current,
                session.fleer,
                session.threat,
                SimEventKind::EngagementEnded {
                    kind: EngagementKind::Flee,
                    engagement_id: *id,
                    participants: vec![session.fleer, session.threat],
                    reason,
                },
            ));
            commands
                .entity(session.fleer)
                .remove::<Engaged>()
                .remove::<EngagedFlee>()
                .queue(RemoveFleeActions);
            to_remove.push(*id);
        }
    }
    for id in to_remove {
        registry.sessions.remove(&id);
    }
}
