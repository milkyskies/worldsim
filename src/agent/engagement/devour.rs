//! `EngagementKind::Devour` — multi-participant feeding from a corpse.
//!
//! Replaces the standalone `ActionType::Devour` action's per-bite churn
//! and 1,638 `NoEdibleFood` failures from the seed-42 baseline (the
//! survival brain proposed `Eat` for predators with empty inventories).
//! Predators now route through `InitiateDevour(corpse)`, which installs
//! one engagement per participant. Multiple predators on the same
//! corpse interleave bite beats — pack feeding falls out of independent
//! initiation, no explicit join mechanic.

use bevy::ecs::world::EntityWorldMut;
use bevy::prelude::*;

use super::component::{Engaged, EngagementEndReason, EngagementId, EngagementKind};
use super::markers::EngagedDevour;
use super::registry::EngagementRegistry;
use crate::agent::Agent;
use crate::agent::actions::registry::{ActionState, ActiveActions};
use crate::agent::actions::types::ActionType;
use crate::agent::events::{
    EngagementBeatPayload, FailureReason, GameEvent, SimEvent, SimEventKind,
};
use crate::agent::mind::knowledge::{Concept, MindGraph, Node};
use crate::agent::psyche::emotions::EmotionalState;
use crate::constants::actions::devour::DURATION_TICKS as BITE_COOLDOWN_TICKS;
use crate::core::not_paused;
use crate::core::tick::TickCount;

pub const DEVOUR_RANGE: f32 = 1.5;
pub const FEAR_OVERRIDE_STRESS: f32 = 70.0;
pub const HUNGER_FULL_THRESHOLD: f32 = 80.0;

#[derive(Debug, Clone, Reflect)]
pub struct DevourSession {
    pub id: EngagementId,
    pub participant: Entity,
    pub corpse: Entity,
    pub started_at: u64,
    pub last_bite_tick: u64,
    pub bites_taken: u32,
}

impl DevourSession {
    pub fn new(id: EngagementId, participant: Entity, corpse: Entity, tick: u64) -> Self {
        Self {
            id,
            participant,
            corpse,
            started_at: tick,
            last_bite_tick: tick.saturating_sub(BITE_COOLDOWN_TICKS as u64 + 1),
            bites_taken: 0,
        }
    }

    pub fn off_cooldown(&self, now: u64) -> bool {
        now.saturating_sub(self.last_bite_tick) >= BITE_COOLDOWN_TICKS as u64
    }
}

/// Per-participant registry. Multiple sessions can share the same
/// `corpse` — that's how pack feeding emerges.
#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct DevourRegistry {
    pub sessions: std::collections::HashMap<EngagementId, DevourSession>,
}

impl DevourRegistry {
    pub fn start(
        &mut self,
        ids: &mut EngagementRegistry,
        participant: Entity,
        corpse: Entity,
        tick: u64,
    ) -> EngagementId {
        let id = ids.mint();
        self.sessions
            .insert(id, DevourSession::new(id, participant, corpse, tick));
        id
    }

    pub fn participants_of(&self, corpse: Entity) -> impl Iterator<Item = Entity> + '_ {
        self.sessions
            .values()
            .filter(move |s| s.corpse == corpse)
            .map(|s| s.participant)
    }
}

pub struct DevourPlugin;

impl Plugin for DevourPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DevourRegistry>().add_systems(
            FixedUpdate,
            (
                process_initiate_devour
                    .after(crate::agent::nervous_system::execution::start_actions),
                drive_devour_engagement.after(process_initiate_devour),
                evaluate_devour_continuation.after(drive_devour_engagement),
            )
                .in_set(crate::core::PerfBucket::Brain)
                .run_if(not_paused),
        );
    }
}

struct RemoveDevourActions;

impl EntityCommand for RemoveDevourActions {
    fn apply(self, mut entity: EntityWorldMut) {
        if let Some(mut active) = entity.get_mut::<ActiveActions>() {
            active.remove(ActionType::InitiateDevour);
            active.remove(ActionType::Devour);
        }
    }
}

pub fn process_initiate_devour(
    mut commands: Commands,
    mut registry: ResMut<DevourRegistry>,
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
                .get(ActionType::InitiateDevour)
                .map(|state| (entity, state.target_entity))
        })
        .collect();

    for (predator, corpse_opt) in pairs {
        let now = tick.current;

        if engaged.get(predator).is_ok() {
            if let Ok((_, mut active)) = active_actions.get_mut(predator) {
                active.remove(ActionType::InitiateDevour);
            }
            continue;
        }

        let Some(corpse) = corpse_opt else {
            drop_initiate(predator, now, FailureReason::NoTarget, &mut active_actions, &mut sim_events);
            continue;
        };
        let Ok(corpse_t) = target_transforms.get(corpse) else {
            drop_initiate(predator, now, FailureReason::TargetGone, &mut active_actions, &mut sim_events);
            continue;
        };
        let Ok(predator_t) = transforms.get(predator) else { continue };
        let distance = predator_t.translation.truncate().distance(corpse_t.translation.truncate());
        if distance > DEVOUR_RANGE {
            // Walk-leg auto-injected via the `InitiateDevour` proximity
            // precondition — the brain's plan should already be Walk →
            // Initiate. While walking, just leave the initiate in place.
            continue;
        }

        let id = registry.start(&mut id_minter, predator, corpse, now);
        commands
            .entity(predator)
            .insert((Engaged::new(EngagementKind::Devour, id), EngagedDevour(id)));

        if let Ok((_, mut active)) = active_actions.get_mut(predator) {
            active.remove(ActionType::InitiateDevour);
        }

        sim_events.write(SimEvent::pair(
            now,
            predator,
            corpse,
            SimEventKind::EngagementStarted {
                kind: EngagementKind::Devour,
                engagement_id: id,
                participants: vec![predator, corpse],
            },
        ));
    }
}

fn drop_initiate(
    predator: Entity,
    tick: u64,
    reason: FailureReason,
    active_actions: &mut Query<(Entity, &mut ActiveActions), With<Agent>>,
    sim_events: &mut MessageWriter<SimEvent>,
) {
    if let Ok((_, mut active)) = active_actions.get_mut(predator) {
        active.remove(ActionType::InitiateDevour);
    }
    sim_events.write(SimEvent::single(
        tick,
        predator,
        SimEventKind::ActionFailed {
            agent: predator,
            action: ActionType::InitiateDevour,
            reason,
        },
    ));
}

pub fn drive_devour_engagement(
    mut registry: ResMut<DevourRegistry>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
    mut active_actions: Query<&mut ActiveActions, With<Agent>>,
) {
    let now = tick.current;
    for session in registry.sessions.values_mut() {
        if !session.off_cooldown(now) {
            continue;
        }
        if let Ok(mut active) = active_actions.get_mut(session.participant) {
            let mut bite = ActionState::new(ActionType::Devour, now);
            bite.target_entity = Some(session.corpse);
            bite.ticks_remaining = BITE_COOLDOWN_TICKS;
            active.insert(bite);
        }
        session.last_bite_tick = now;
        session.bites_taken += 1;

        sim_events.write(SimEvent::single(
            now,
            session.participant,
            SimEventKind::EngagementBeat {
                kind: EngagementKind::Devour,
                engagement_id: session.id,
                agent: session.participant,
                payload: EngagementBeatPayload::Devour {
                    participant: session.participant,
                    corpse: session.corpse,
                    bite_index: session.bites_taken,
                },
            },
        ));
    }
}

pub fn evaluate_devour_continuation(
    mut commands: Commands,
    mut registry: ResMut<DevourRegistry>,
    mut sim_events: MessageWriter<SimEvent>,
    mut game_events: MessageWriter<GameEvent>,
    tick: Res<TickCount>,
    transforms: Query<&Transform>,
    physicals: Query<&crate::agent::body::needs::PhysicalNeeds>,
    emotions: Query<&EmotionalState>,
    minds: Query<&MindGraph>,
    inventories: Query<&crate::agent::item_slots::ItemSlots>,
) {
    let _ = game_events;
    let mut to_remove: Vec<EngagementId> = Vec::new();
    for (id, session) in registry.sessions.iter() {
        let mut reason: Option<EngagementEndReason> = None;

        let corpse_gone = transforms.get(session.corpse).is_err();
        if corpse_gone {
            reason = Some(EngagementEndReason::Natural);
        }

        // Corpse depleted of edible content?
        if reason.is_none()
            && let Ok(inv) = inventories.get(session.corpse)
        {
            let any_edible = inv.all_items().any(|item| {
                minds
                    .get(session.participant)
                    .ok()
                    .map(|m| m.is_a(&Node::Concept(item.concept), Concept::Food))
                    .unwrap_or(false)
            });
            if !any_edible {
                reason = Some(EngagementEndReason::Natural);
            }
        }

        // Stomach full?
        if reason.is_none()
            && physicals
                .get(session.participant)
                .map(|p| p.metabolism.stomach_fullness() >= HUNGER_FULL_THRESHOLD)
                .unwrap_or(false)
        {
            reason = Some(EngagementEndReason::Natural);
        }

        if reason.is_none()
            && emotions
                .get(session.participant)
                .map(|e| e.stress_level >= FEAR_OVERRIDE_STRESS)
                .unwrap_or(false)
        {
            reason = Some(EngagementEndReason::EmotionOverride);
        }

        if let Some(reason) = reason {
            sim_events.write(SimEvent::pair(
                tick.current,
                session.participant,
                session.corpse,
                SimEventKind::EngagementEnded {
                    kind: EngagementKind::Devour,
                    engagement_id: *id,
                    participants: vec![session.participant, session.corpse],
                    reason,
                },
            ));
            commands
                .entity(session.participant)
                .remove::<Engaged>()
                .remove::<EngagedDevour>()
                .queue(RemoveDevourActions);
            to_remove.push(*id);
        }
    }
    for id in to_remove {
        registry.sessions.remove(&id);
    }
}
