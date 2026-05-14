//! `EngagementKind::Hunt` — predator pursuit of living prey with strikes
//! over time as a single commitment.
//!
//! Replaces the discrete `[Walk(snapshot_tile), Bite(stationary)]` plan
//! shape that produced the bite-jitter bug: the wolf would stale-target
//! the deer, arrive at the empty tile, fail Bite preconditions, replan,
//! drift, repeat. The engagement tracks the prey's *current* perceived
//! position each tick, drives a pursue-Walk when out of melee range, and
//! emits a 1-tick `Bite` strike when adjacent and off cooldown.

use bevy::ecs::world::EntityWorldMut;
use bevy::prelude::*;

use super::component::{Engaged, EngagementEndReason, EngagementId, EngagementKind};
use super::markers::EngagedHunt;
use super::registry::EngagementRegistry;
use crate::agent::Agent;
use crate::agent::actions::registry::{ActionState, ActiveActions};
use crate::agent::actions::types::ActionType;
use crate::agent::brains::plan_memory::{PlanAbandonReason, PlanMemory};
use crate::agent::events::{
    EngagementBeatPayload, FailureReason, GameEvent, SimEvent, SimEventKind,
};
use crate::agent::mind::knowledge::{Concept, MindGraph, Node};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::emotions::EmotionalState;
use crate::constants::actions::attack::DURATION_TICKS as STRIKE_COOLDOWN_TICKS;
use crate::core::not_paused;
use crate::core::tick::TickCount;

/// Distance below which a strike beat fires; matches conversation/melee
/// neighbourhood (one tile diagonal slack).
pub const STRIKE_RANGE: f32 = 1.5;

/// Out-of-perception ticks before the engagement ends with `OutOfRange`.
pub const TARGET_LOST_GRACE_TICKS: u64 = 60;

/// Acute-fear / threat-appraisal flip threshold expressed as
/// `EmotionalState.stress_level` percentage. When stress crosses this
/// while engaged, the hunt ends with `EmotionOverride`.
pub const FEAR_OVERRIDE_STRESS: f32 = 70.0;

/// Aerobic stamina below which the hunt ends with `ChannelExhaustion`.
pub const EXHAUSTION_AEROBIC: f32 = 0.05;

/// One live hunt engagement — exactly one predator pursuing one prey.
/// Pack hunting falls out of multiple agents each installing their own
/// `Hunt` against the same prey.
#[derive(Debug, Clone, Reflect)]
pub struct Hunt {
    pub id: EngagementId,
    pub predator: Entity,
    pub prey: Entity,
    pub started_at: u64,
    pub last_strike_tick: u64,
    pub strikes_landed: u32,
    pub last_seen_tick: u64,
}

impl Hunt {
    pub fn new(id: EngagementId, predator: Entity, prey: Entity, tick: u64) -> Self {
        Self {
            id,
            predator,
            prey,
            started_at: tick,
            // Allow first strike immediately on entering range.
            last_strike_tick: tick.saturating_sub(STRIKE_COOLDOWN_TICKS as u64 + 1),
            strikes_landed: 0,
            last_seen_tick: tick,
        }
    }

    pub fn off_cooldown(&self, now: u64) -> bool {
        now.saturating_sub(self.last_strike_tick) >= STRIKE_COOLDOWN_TICKS as u64
    }
}

#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct HuntRegistry {
    pub hunts: std::collections::HashMap<EngagementId, Hunt>,
}

impl HuntRegistry {
    pub fn start(
        &mut self,
        ids: &mut EngagementRegistry,
        predator: Entity,
        prey: Entity,
        tick: u64,
    ) -> EngagementId {
        let id = ids.mint();
        self.hunts.insert(id, Hunt::new(id, predator, prey, tick));
        id
    }

    pub fn for_predator(&self, predator: Entity) -> Option<&Hunt> {
        self.hunts.values().find(|h| h.predator == predator)
    }

    pub fn active_targeting(&self, prey: Entity) -> impl Iterator<Item = &Hunt> {
        self.hunts.values().filter(move |h| h.prey == prey)
    }
}

pub struct HuntPlugin;

impl Plugin for HuntPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HuntRegistry>().add_systems(
            FixedUpdate,
            (
                process_initiate_hunt
                    .after(crate::agent::nervous_system::execution::start_actions)
                    .before(crate::agent::nervous_system::execution::tick_actions),
                drive_hunt_engagement.after(process_initiate_hunt),
                evaluate_hunt_continuation.after(drive_hunt_engagement),
            )
                .in_set(crate::core::PerfBucket::Brain)
                .run_if(not_paused),
        );
    }
}

struct RemoveHuntActions;

impl EntityCommand for RemoveHuntActions {
    fn apply(self, mut entity: EntityWorldMut) {
        if let Some(mut active) = entity.get_mut::<ActiveActions>() {
            active.remove(ActionType::InitiateHunt);
            active.remove(ActionType::Bite);
            active.remove(ActionType::Walk);
        }
    }
}

/// Consume `InitiateHunt` proposals from brains, install `EngagedHunt`,
/// mint the engagement.
pub fn process_initiate_hunt(
    mut commands: Commands,
    mut registry: ResMut<HuntRegistry>,
    mut id_minter: ResMut<EngagementRegistry>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
    transforms: Query<&Transform, With<Agent>>,
    engaged: Query<&Engaged, With<Agent>>,
    mut active_actions: Query<(Entity, &mut ActiveActions), With<Agent>>,
    mut plan_memory_query: Query<&mut PlanMemory>,
) {
    let pairs: Vec<(Entity, Option<Entity>)> = active_actions
        .iter()
        .filter_map(|(entity, active)| {
            active
                .get(ActionType::InitiateHunt)
                .map(|state| (entity, state.target_entity))
        })
        .collect();

    for (predator, prey_opt) in pairs {
        let now = tick.current;

        if engaged.get(predator).is_ok() {
            // Already engaged in something — drop the stale initiate.
            if let Ok((_, mut active)) = active_actions.get_mut(predator) {
                active.remove(ActionType::InitiateHunt);
            }
            continue;
        }

        let Some(prey) = prey_opt else {
            drop_initiate(
                predator,
                now,
                FailureReason::NoTarget,
                &mut active_actions,
                &mut plan_memory_query,
                &mut sim_events,
            );
            continue;
        };

        let Ok(_prey_t) = transforms.get(prey) else {
            drop_initiate(
                predator,
                now,
                FailureReason::TargetGone,
                &mut active_actions,
                &mut plan_memory_query,
                &mut sim_events,
            );
            continue;
        };

        let id = registry.start(&mut id_minter, predator, prey, now);
        commands
            .entity(predator)
            .insert((Engaged::new(EngagementKind::Hunt, id), EngagedHunt(id)));

        if let Ok((_, mut active)) = active_actions.get_mut(predator) {
            active.remove(ActionType::InitiateHunt);
        }

        sim_events.write(SimEvent::pair(
            now,
            predator,
            prey,
            SimEventKind::EngagementStarted {
                kind: EngagementKind::Hunt,
                engagement_id: id,
                participants: vec![predator, prey],
            },
        ));
    }
}

fn drop_initiate(
    predator: Entity,
    tick: u64,
    reason: FailureReason,
    active_actions: &mut Query<(Entity, &mut ActiveActions), With<Agent>>,
    plan_memory_query: &mut Query<&mut PlanMemory>,
    sim_events: &mut MessageWriter<SimEvent>,
) {
    if let Ok((_, mut active)) = active_actions.get_mut(predator) {
        active.remove(ActionType::InitiateHunt);
    }
    if let Ok(mut memory) = plan_memory_query.get_mut(predator) {
        let doomed: Vec<_> = memory
            .plans
            .iter()
            .filter(|p| {
                p.current()
                    .map(|a| a.action_type == ActionType::InitiateHunt)
                    .unwrap_or(false)
            })
            .map(|p| (p.id, p.driving_urgency))
            .collect();
        for (id, source) in doomed {
            memory.remove(id);
            sim_events.write(SimEvent::plan_abandoned(
                tick,
                predator,
                id,
                source,
                PlanAbandonReason::StepAdvancedInvalid,
            ));
        }
    }
    sim_events.write(SimEvent::single(
        tick,
        predator,
        SimEventKind::ActionFailed {
            agent: predator,
            action: ActionType::InitiateHunt,
            reason,
        },
    ));
}

/// Per-tick inner loop: pursue the prey's *current* perceived position
/// when out of strike range; emit a `Bite` beat when adjacent + off
/// cooldown.
pub fn drive_hunt_engagement(
    mut registry: ResMut<HuntRegistry>,
    tick: Res<TickCount>,
    transforms: Query<&Transform>,
    visible: Query<&VisibleObjects>,
    mut sim_events: MessageWriter<SimEvent>,
    mut game_events: MessageWriter<GameEvent>,
    mut active_actions: Query<&mut ActiveActions, With<Agent>>,
    mut target_positions: Query<&mut crate::agent::TargetPosition>,
) {
    let now = tick.current;
    for hunt in registry.hunts.values_mut() {
        let Ok(predator_t) = transforms.get(hunt.predator) else {
            continue;
        };
        let Ok(prey_t) = transforms.get(hunt.prey) else {
            continue;
        };
        let predator_pos = predator_t.translation.truncate();
        let prey_pos = prey_t.translation.truncate();

        let perceives_prey = visible
            .get(hunt.predator)
            .map(|v| v.entities.contains(&hunt.prey))
            .unwrap_or(false);
        if perceives_prey {
            hunt.last_seen_tick = now;
        }

        let distance = predator_pos.distance(prey_pos);

        if distance > STRIKE_RANGE {
            // Pursuit leg: drive a Walk toward the prey's current position.
            // Re-target every tick — this is the core fix vs. the old
            // snapshot-target jitter.
            if let Ok(mut active) = active_actions.get_mut(hunt.predator) {
                if !active.contains(ActionType::Walk) {
                    let mut walk = ActionState::new(ActionType::Walk, now);
                    walk.target_entity = Some(hunt.prey);
                    walk.target_position = Some(prey_pos);
                    walk.locomotion_intensity = 1.0;
                    active.insert(walk);
                } else if let Some(state) = active.get_mut(ActionType::Walk) {
                    state.target_entity = Some(hunt.prey);
                    state.target_position = Some(prey_pos);
                    state.locomotion_intensity = 1.0;
                }
            }
            if let Ok(mut tp) = target_positions.get_mut(hunt.predator) {
                tp.0 = Some(prey_pos);
            }
            continue;
        }

        // In range — drop pursuit Walk if any.
        if let Ok(mut active) = active_actions.get_mut(hunt.predator) {
            active.remove(ActionType::Walk);
        }
        if let Ok(mut tp) = target_positions.get_mut(hunt.predator) {
            tp.0 = None;
        }

        if !hunt.off_cooldown(now) {
            continue;
        }

        // Strike! Insert a 1-tick Bite as a beat. The damage path in
        // `combat.rs` wires off the Bite ActionStarted event, unchanged.
        if let Ok(mut active) = active_actions.get_mut(hunt.predator) {
            let mut bite = ActionState::new(ActionType::Bite, now);
            bite.target_entity = Some(hunt.prey);
            bite.target_position = Some(prey_pos);
            bite.ticks_remaining = 1;
            active.insert(bite);
        }
        hunt.last_strike_tick = now;
        hunt.strikes_landed += 1;

        sim_events.write(SimEvent::single(
            now,
            hunt.predator,
            SimEventKind::EngagementBeat {
                kind: EngagementKind::Hunt,
                engagement_id: hunt.id,
                agent: hunt.predator,
                payload: EngagementBeatPayload::Hunt {
                    predator: hunt.predator,
                    prey: hunt.prey,
                    strike_index: hunt.strikes_landed,
                },
            },
        ));
        game_events.write(GameEvent::Interaction {
            actor: hunt.predator,
            action: ActionType::Bite,
            target: Some(hunt.prey),
            location: Some(prey_pos),
        });
    }
}

/// Cleanup pass: end engagements when target dies, escapes, predator
/// exhausts, or fear flips the brain to flight.
pub fn evaluate_hunt_continuation(
    mut commands: Commands,
    mut registry: ResMut<HuntRegistry>,
    mut sim_events: MessageWriter<SimEvent>,
    tick: Res<TickCount>,
    transforms: Query<&Transform>,
    minds: Query<&MindGraph>,
    physicals: Query<&crate::agent::body::needs::PhysicalNeeds>,
    emotions: Query<&EmotionalState>,
) {
    let mut to_remove: Vec<EngagementId> = Vec::new();
    for (id, hunt) in registry.hunts.iter() {
        let mut reason: Option<EngagementEndReason> = None;

        // Target dead?
        let prey_dead = transforms.get(hunt.prey).is_err()
            || minds
                .get(hunt.predator)
                .ok()
                .map(|m| m.is_a(&Node::Entity(hunt.prey), Concept::Carrion))
                .unwrap_or(false);
        if prey_dead {
            reason = Some(EngagementEndReason::Natural);
        }

        // Out of perception too long?
        if reason.is_none()
            && tick.current.saturating_sub(hunt.last_seen_tick) > TARGET_LOST_GRACE_TICKS
        {
            reason = Some(EngagementEndReason::OutOfRange);
        }

        // Exhaustion?
        if reason.is_none()
            && physicals
                .get(hunt.predator)
                .map(|p| p.stamina.aerobic < EXHAUSTION_AEROBIC)
                .unwrap_or(false)
        {
            reason = Some(EngagementEndReason::Other);
        }

        // Acute-fear override?
        if reason.is_none()
            && emotions
                .get(hunt.predator)
                .map(|e| e.stress_level >= FEAR_OVERRIDE_STRESS)
                .unwrap_or(false)
        {
            reason = Some(EngagementEndReason::EmotionOverride);
        }

        if let Some(reason) = reason {
            sim_events.write(SimEvent::pair(
                tick.current,
                hunt.predator,
                hunt.prey,
                SimEventKind::EngagementEnded {
                    kind: EngagementKind::Hunt,
                    engagement_id: *id,
                    participants: vec![hunt.predator, hunt.prey],
                    reason,
                },
            ));
            commands
                .entity(hunt.predator)
                .remove::<Engaged>()
                .remove::<EngagedHunt>()
                .queue(RemoveHuntActions);
            to_remove.push(*id);
        }
    }
    for id in to_remove {
        registry.hunts.remove(&id);
    }
}
