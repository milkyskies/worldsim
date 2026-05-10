//! `EngagementKind::Harvest` — sustained per-yield gathering from a
//! single source entity (bush, tree, log).
//!
//! Replaces the `Harvest` standalone action's per-yield brain churn:
//! one engagement per source, multiple yield beats over time, exits
//! when the source is depleted, the agent is full, or a threat overrides.

use bevy::ecs::world::EntityWorldMut;
use bevy::prelude::*;

use super::component::{Engaged, EngagementEndReason, EngagementId, EngagementKind};
use super::markers::EngagedHarvest;
use super::registry::EngagementRegistry;
use crate::agent::Agent;
use crate::agent::actions::registry::{ActionState, ActiveActions};
use crate::agent::actions::types::ActionType;
use crate::agent::events::{
    EngagementBeatPayload, FailureReason, GameEvent, SimEvent, SimEventKind,
};
use crate::agent::psyche::emotions::EmotionalState;
use crate::constants::actions::harvest::DURATION_TICKS as YIELD_COOLDOWN_TICKS;
use crate::core::not_paused;
use crate::core::tick::TickCount;

pub const HARVEST_RANGE: f32 = 1.5;
pub const FEAR_OVERRIDE_STRESS: f32 = 70.0;

#[derive(Debug, Clone, Reflect)]
pub struct HarvestSession {
    pub id: EngagementId,
    pub participants: Vec<Entity>,
    pub source: Entity,
    pub started_at: u64,
    pub last_yield_tick: u64,
    pub yields_taken: u32,
}

impl HarvestSession {
    pub fn new(id: EngagementId, harvester: Entity, source: Entity, tick: u64) -> Self {
        Self {
            id,
            participants: vec![harvester],
            source,
            started_at: tick,
            last_yield_tick: tick.saturating_sub(YIELD_COOLDOWN_TICKS as u64 + 1),
            yields_taken: 0,
        }
    }

    pub fn off_cooldown(&self, now: u64) -> bool {
        now.saturating_sub(self.last_yield_tick) >= YIELD_COOLDOWN_TICKS as u64
    }
}

#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct HarvestRegistry {
    pub sessions: std::collections::HashMap<EngagementId, HarvestSession>,
}

impl HarvestRegistry {
    pub fn start(
        &mut self,
        ids: &mut EngagementRegistry,
        harvester: Entity,
        source: Entity,
        tick: u64,
    ) -> EngagementId {
        let id = ids.mint();
        self.sessions
            .insert(id, HarvestSession::new(id, harvester, source, tick));
        id
    }
}

pub struct HarvestPlugin;

impl Plugin for HarvestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HarvestRegistry>().add_systems(
            FixedUpdate,
            (
                process_initiate_harvest
                    .after(crate::agent::nervous_system::execution::start_actions),
                drive_harvest_engagement.after(process_initiate_harvest),
                evaluate_harvest_continuation.after(drive_harvest_engagement),
            )
                .in_set(crate::core::PerfBucket::Brain)
                .run_if(not_paused),
        );
    }
}

struct RemoveHarvestActions;

impl EntityCommand for RemoveHarvestActions {
    fn apply(self, mut entity: EntityWorldMut) {
        if let Some(mut active) = entity.get_mut::<ActiveActions>() {
            active.remove(ActionType::InitiateHarvest);
            active.remove(ActionType::Harvest);
        }
    }
}

pub fn process_initiate_harvest(
    mut commands: Commands,
    mut registry: ResMut<HarvestRegistry>,
    mut id_minter: ResMut<EngagementRegistry>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
    transforms: Query<&Transform, With<Agent>>,
    target_transforms: Query<&Transform>,
    engaged: Query<&Engaged, With<Agent>>,
    mut active_actions: Query<(Entity, &mut ActiveActions), With<Agent>>,
) {
    let pairs: Vec<(Entity, Option<Entity>)> = active_actions
        .iter()
        .filter_map(|(entity, active)| {
            active
                .get(ActionType::InitiateHarvest)
                .map(|state| (entity, state.target_entity))
        })
        .collect();

    for (harvester, source_opt) in pairs {
        let now = tick.current;
        if engaged.get(harvester).is_ok() {
            if let Ok((_, mut active)) = active_actions.get_mut(harvester) {
                active.remove(ActionType::InitiateHarvest);
            }
            continue;
        }
        let Some(source) = source_opt else {
            drop_initiate(harvester, now, FailureReason::NoTarget, &mut active_actions, &mut sim_events);
            continue;
        };
        let Ok(source_t) = target_transforms.get(source) else {
            drop_initiate(harvester, now, FailureReason::TargetGone, &mut active_actions, &mut sim_events);
            continue;
        };
        let Ok(harvester_t) = transforms.get(harvester) else { continue };
        if harvester_t.translation.truncate().distance(source_t.translation.truncate()) > HARVEST_RANGE {
            continue;
        }
        let id = registry.start(&mut id_minter, harvester, source, now);
        commands
            .entity(harvester)
            .insert((Engaged::new(EngagementKind::Harvest, id), EngagedHarvest(id)));

        if let Ok((_, mut active)) = active_actions.get_mut(harvester) {
            active.remove(ActionType::InitiateHarvest);
        }
        sim_events.write(SimEvent::pair(
            now,
            harvester,
            source,
            SimEventKind::EngagementStarted {
                kind: EngagementKind::Harvest,
                engagement_id: id,
                participants: vec![harvester, source],
            },
        ));
    }
}

fn drop_initiate(
    harvester: Entity,
    tick: u64,
    reason: FailureReason,
    active_actions: &mut Query<(Entity, &mut ActiveActions), With<Agent>>,
    sim_events: &mut MessageWriter<SimEvent>,
) {
    if let Ok((_, mut active)) = active_actions.get_mut(harvester) {
        active.remove(ActionType::InitiateHarvest);
    }
    sim_events.write(SimEvent::single(
        tick,
        harvester,
        SimEventKind::ActionFailed {
            agent: harvester,
            action: ActionType::InitiateHarvest,
            reason,
        },
    ));
}

pub fn drive_harvest_engagement(
    mut registry: ResMut<HarvestRegistry>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
    mut active_actions: Query<&mut ActiveActions, With<Agent>>,
) {
    let now = tick.current;
    for session in registry.sessions.values_mut() {
        if !session.off_cooldown(now) {
            continue;
        }
        for participant in session.participants.iter().copied() {
            if let Ok(mut active) = active_actions.get_mut(participant) {
                let mut yld = ActionState::new(ActionType::Harvest, now);
                yld.target_entity = Some(session.source);
                yld.ticks_remaining = YIELD_COOLDOWN_TICKS;
                active.insert(yld);
            }
            sim_events.write(SimEvent::single(
                now,
                participant,
                SimEventKind::EngagementBeat {
                    kind: EngagementKind::Harvest,
                    engagement_id: session.id,
                    agent: participant,
                    payload: EngagementBeatPayload::Harvest {
                        harvester: participant,
                        source: session.source,
                        yield_index: session.yields_taken + 1,
                    },
                },
            ));
        }
        session.last_yield_tick = now;
        session.yields_taken += 1;
    }
}

pub fn evaluate_harvest_continuation(
    mut commands: Commands,
    mut registry: ResMut<HarvestRegistry>,
    mut sim_events: MessageWriter<SimEvent>,
    tick: Res<TickCount>,
    transforms: Query<&Transform>,
    inventories: Query<&crate::agent::item_slots::ItemSlots>,
    emotions: Query<&EmotionalState>,
) {
    let mut to_remove: Vec<EngagementId> = Vec::new();
    for (id, session) in registry.sessions.iter() {
        let mut reason: Option<EngagementEndReason> = None;

        if transforms.get(session.source).is_err() {
            reason = Some(EngagementEndReason::OutOfRange);
        }
        if reason.is_none()
            && let Ok(inv) = inventories.get(session.source)
            && inv.all_items().next().is_none()
        {
            reason = Some(EngagementEndReason::Natural);
        }

        if reason.is_none() {
            for participant in session.participants.iter().copied() {
                if let Ok(e) = emotions.get(participant)
                    && e.stress_level >= FEAR_OVERRIDE_STRESS
                {
                    reason = Some(EngagementEndReason::EmotionOverride);
                    break;
                }
            }
        }

        if let Some(reason) = reason {
            sim_events.write(SimEvent::new(
                tick.current,
                session.participants.clone(),
                SimEventKind::EngagementEnded {
                    kind: EngagementKind::Harvest,
                    engagement_id: *id,
                    participants: session.participants.clone(),
                    reason,
                },
            ));
            for participant in session.participants.iter().copied() {
                commands
                    .entity(participant)
                    .remove::<Engaged>()
                    .remove::<EngagedHarvest>()
                    .queue(RemoveHarvestActions);
            }
            to_remove.push(*id);
        }
    }
    for id in to_remove {
        registry.sessions.remove(&id);
    }
}
