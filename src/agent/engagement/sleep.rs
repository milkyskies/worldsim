//! `EngagementKind::Sleep` — formalizes the existing Sleep state-shape
//! onto the engagement primitive. Migration only — no rate or scoring
//! changes. The Sleep-specific short-circuit in `start_actions` is
//! replaced by the generic engagement-commitment gate.

use bevy::ecs::world::EntityWorldMut;
use bevy::prelude::*;

use super::component::{Engaged, EngagementEndReason, EngagementId, EngagementKind};
use super::markers::EngagedSleep;
use super::registry::EngagementRegistry;
use crate::agent::Agent;
use crate::agent::actions::registry::{ActionState, ActiveActions};
use crate::agent::actions::types::ActionType;
use crate::agent::events::{EngagementBeatPayload, SimEvent, SimEventKind};
use crate::agent::psyche::emotions::EmotionalState;
use crate::core::not_paused;
use crate::core::tick::TickCount;

pub const FEAR_OVERRIDE_STRESS: f32 = 70.0;
pub const NATURAL_WAKE_THRESHOLD: f32 = 0.95;

#[derive(Debug, Clone, Reflect)]
pub struct SleepSession {
    pub id: EngagementId,
    pub sleeper: Entity,
    pub started_at: u64,
    pub beats: u32,
}

impl SleepSession {
    pub fn new(id: EngagementId, sleeper: Entity, tick: u64) -> Self {
        Self {
            id,
            sleeper,
            started_at: tick,
            beats: 0,
        }
    }
}

#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct SleepRegistry {
    pub sessions: std::collections::HashMap<EngagementId, SleepSession>,
}

impl SleepRegistry {
    pub fn start(
        &mut self,
        ids: &mut EngagementRegistry,
        sleeper: Entity,
        tick: u64,
    ) -> EngagementId {
        let id = ids.mint();
        self.sessions
            .insert(id, SleepSession::new(id, sleeper, tick));
        id
    }
}

pub struct SleepPlugin;

impl Plugin for SleepPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SleepRegistry>().add_systems(
            FixedUpdate,
            (
                process_initiate_sleep
                    .after(crate::agent::nervous_system::execution::start_actions)
                    .before(crate::agent::nervous_system::execution::tick_actions),
                drive_sleep_engagement.after(process_initiate_sleep),
                evaluate_sleep_continuation.after(drive_sleep_engagement),
            )
                .in_set(crate::core::PerfBucket::Brain)
                .run_if(not_paused),
        );
    }
}

struct RemoveSleepActions;

impl EntityCommand for RemoveSleepActions {
    fn apply(self, mut entity: EntityWorldMut) {
        if let Some(mut active) = entity.get_mut::<ActiveActions>() {
            active.remove(ActionType::InitiateSleep);
            active.remove(ActionType::Sleep);
        }
    }
}

pub fn process_initiate_sleep(
    mut commands: Commands,
    mut registry: ResMut<SleepRegistry>,
    mut id_minter: ResMut<EngagementRegistry>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
    engaged: Query<&Engaged, With<Agent>>,
    mut active_actions: Query<(Entity, &mut ActiveActions), With<Agent>>,
) {
    let sleepers: Vec<Entity> = active_actions
        .iter()
        .filter_map(|(entity, active)| {
            if active.contains(ActionType::InitiateSleep) {
                Some(entity)
            } else {
                None
            }
        })
        .collect();

    for sleeper in sleepers {
        let now = tick.current;
        if engaged.get(sleeper).is_ok() {
            // Already engaged. Strip the duplicate InitiateSleep, and
            // re-insert the Sleep beat in case start_actions evicted it
            // via the Locomotion-vs-FullBody channel conflict on this
            // tick's InitiateSleep admission.
            if let Ok((_, mut active)) = active_actions.get_mut(sleeper) {
                active.remove(ActionType::InitiateSleep);
                if !active.contains(ActionType::Sleep) {
                    let mut sleep = ActionState::new(ActionType::Sleep, now);
                    sleep.ticks_remaining = u32::MAX;
                    active.insert(sleep);
                }
            }
            continue;
        }
        let id = registry.start(&mut id_minter, sleeper, now);
        commands
            .entity(sleeper)
            .insert((Engaged::new(EngagementKind::Sleep, id), EngagedSleep(id)));

        if let Ok((_, mut active)) = active_actions.get_mut(sleeper) {
            active.remove(ActionType::InitiateSleep);
            // Insert the Sleep beat directly — it's an indefinite Timed
            // action with FullBody 1.0 channel claim. Recovery math runs
            // through the existing `Sleep` action definition's
            // runtime_effects.
            let mut sleep = ActionState::new(ActionType::Sleep, now);
            sleep.ticks_remaining = u32::MAX;
            active.insert(sleep);
        }
        sim_events.write(SimEvent::single(
            now,
            sleeper,
            SimEventKind::EngagementStarted {
                kind: EngagementKind::Sleep,
                engagement_id: id,
                participants: vec![sleeper],
            },
        ));
    }
}

pub fn drive_sleep_engagement(
    mut registry: ResMut<SleepRegistry>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    let now = tick.current;
    for session in registry.sessions.values_mut() {
        session.beats += 1;
        // Beat events are noisy here (one per tick of sleep); throttle to
        // one per game-minute.
        if session.beats.is_multiple_of(60) {
            sim_events.write(SimEvent::single(
                now,
                session.sleeper,
                SimEventKind::EngagementBeat {
                    kind: EngagementKind::Sleep,
                    engagement_id: session.id,
                    agent: session.sleeper,
                    payload: EngagementBeatPayload::Sleep {
                        sleeper: session.sleeper,
                        beat_index: session.beats,
                    },
                },
            ));
        }
    }
}

pub fn evaluate_sleep_continuation(
    mut commands: Commands,
    mut registry: ResMut<SleepRegistry>,
    mut sim_events: MessageWriter<SimEvent>,
    tick: Res<TickCount>,
    physicals: Query<&crate::agent::body::needs::PhysicalNeeds>,
    emotions: Query<&EmotionalState>,
    actives: Query<&ActiveActions>,
) {
    let mut to_remove: Vec<EngagementId> = Vec::new();
    for (id, session) in registry.sessions.iter() {
        let mut reason: Option<EngagementEndReason> = None;

        // Natural: wakefulness restored, OR explicit WakeUp inserted by
        // emergency / alarm paths.
        if let Ok(p) = physicals.get(session.sleeper)
            && p.wakefulness.value >= NATURAL_WAKE_THRESHOLD
        {
            reason = Some(EngagementEndReason::Natural);
        }
        if reason.is_none()
            && actives
                .get(session.sleeper)
                .map(|a| a.contains(ActionType::WakeUp))
                .unwrap_or(false)
        {
            reason = Some(EngagementEndReason::Natural);
        }
        if reason.is_none()
            && emotions
                .get(session.sleeper)
                .map(|e| e.stress_level >= FEAR_OVERRIDE_STRESS)
                .unwrap_or(false)
        {
            reason = Some(EngagementEndReason::EmotionOverride);
        }

        if let Some(reason) = reason {
            sim_events.write(SimEvent::single(
                tick.current,
                session.sleeper,
                SimEventKind::EngagementEnded {
                    kind: EngagementKind::Sleep,
                    engagement_id: *id,
                    participants: vec![session.sleeper],
                    reason,
                },
            ));
            commands
                .entity(session.sleeper)
                .remove::<Engaged>()
                .remove::<EngagedSleep>()
                .queue(RemoveSleepActions);
            to_remove.push(*id);
        }
    }
    for id in to_remove {
        registry.sessions.remove(&id);
    }
}
