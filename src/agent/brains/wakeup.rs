//! Brain wakeup events: gates `arbitrate_every_tick` and the rational
//! planner so they only run for agents whose situation actually changed.
//!
//! Reads: SimEvent (action lifecycle, mind-graph mutations), CentralNervousSystem
//!        (urgency thresholds), VisibleObjects (new entities), InConversation
//!        (added/removed/changed), Added<Agent> (initial wakeup).
//! Writes: BrainWakeup (consumed by arbitrate_every_tick + update_rational_planning).
//! Upstream: action lifecycle, perception, cns urgency, conversation lifecycle.
//! Downstream: brains::brain_system, brains::rational.

use crate::agent::Agent;
use crate::agent::events::{SimEvent, SimEventKind};
use crate::agent::mind::conversation::InConversation;
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::core::tick::TickCount;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

/// Why the brain was woken. Carried for diagnostics + for the brain to
/// decide whether to take a fast-path (e.g. PeriodicSafety can skip
/// expensive work that nothing actually changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum BrainTrigger {
    /// Newly-spawned agent's first arbitration. Required so freshly-built
    /// scenarios in tests get a decision on tick 1.
    Initial,
    /// An action just started, completed, failed, or was preempted.
    ActionLifecycle,
    /// An urgency crossed a hysteresis band (rising or falling).
    DriveThreshold,
    /// VisibleObjects gained at least one entity not visible last tick.
    NewPerception,
    /// MindGraph added or removed a triple.
    KnowledgeChanged,
    /// Conversation joined, left, or its turn-state changed.
    ConversationStateChanged,
    /// Periodic catch-all so a missed wakeup can never strand an agent.
    PeriodicSafety,
}

#[derive(Message, Clone, Copy, Debug)]
pub struct BrainWakeup {
    pub tick: u64,
    pub agent: Entity,
    pub reason: BrainTrigger,
}

/// Hysteresis bands for `DriveThreshold` wakeups. A wakeup fires when the
/// urgency crosses a band edge in either direction. Different up vs. down
/// bands stop oscillation around a single threshold from waking the brain
/// every tick.
const URGENCY_BANDS_UP: [f32; 3] = [0.3, 0.5, 0.75];
const URGENCY_BANDS_DOWN: [f32; 3] = [0.2, 0.4, 0.65];

/// Periodic-safety stagger: every tick exactly
/// `agent_count / PERIODIC_SAFETY_PERIOD` agents (rounded) get a wakeup,
/// so an idle agent gets re-arbitrated at most every PERIODIC_SAFETY_PERIOD
/// ticks. With 1 tick = 1 game-second this is a 10-game-minute safety net.
pub const PERIODIC_SAFETY_PERIOD: u64 = 600;

/// Per-agent urgency band index history. Key = agent + source. Value =
/// the index of the highest band the urgency value sat above on the last
/// scan. Wakeups fire on index changes.
#[derive(Resource, Default)]
pub struct UrgencyBandHistory {
    pub last_band: HashMap<(Entity, UrgencySource), usize>,
}

/// Per-agent perception history: the entity set visible last tick.
/// Wakeup fires when the new set has any entity not in the old set.
#[derive(Resource, Default)]
pub struct PerceptionHistory {
    pub last_visible: HashMap<Entity, HashSet<Entity>>,
}

/// Maps urgency value to a discrete band index using hysteresis. The
/// `last_index` argument is the previous band — used to bias the
/// up/down crossing comparison so oscillation around a single edge
/// doesn't toggle.
pub fn urgency_band(value: f32, last_index: usize) -> usize {
    let mut idx = 0usize;
    for (i, threshold) in URGENCY_BANDS_UP.iter().enumerate() {
        if value >= *threshold {
            idx = i + 1;
        }
    }
    if idx < last_index {
        // Going down: require crossing the lower band edge to drop.
        let mut down_idx = 0usize;
        for (i, threshold) in URGENCY_BANDS_DOWN.iter().enumerate() {
            if value >= *threshold {
                down_idx = i + 1;
            }
        }
        return down_idx.max(idx);
    }
    idx
}

pub fn emit_initial_wakeups(
    tick: Res<TickCount>,
    spawned: Query<Entity, Added<Agent>>,
    mut wakeups: MessageWriter<BrainWakeup>,
) {
    for agent in spawned.iter() {
        wakeups.write(BrainWakeup {
            tick: tick.current,
            agent,
            reason: BrainTrigger::Initial,
        });
    }
}

pub fn emit_action_lifecycle_wakeups(
    tick: Res<TickCount>,
    mut events: MessageReader<SimEvent>,
    mut wakeups: MessageWriter<BrainWakeup>,
) {
    for event in events.read() {
        let agent = match &event.kind {
            SimEventKind::ActionStarted { agent, .. }
            | SimEventKind::ActionCompleted { agent, .. }
            | SimEventKind::ActionFailed { agent, .. }
            | SimEventKind::ActionPreempted { agent, .. } => *agent,
            _ => continue,
        };
        wakeups.write(BrainWakeup {
            tick: tick.current,
            agent,
            reason: BrainTrigger::ActionLifecycle,
        });
    }
}

pub fn emit_drive_threshold_wakeups(
    tick: Res<TickCount>,
    agents: Query<(Entity, &CentralNervousSystem), With<Agent>>,
    mut history: ResMut<UrgencyBandHistory>,
    mut wakeups: MessageWriter<BrainWakeup>,
) {
    for (agent, cns) in agents.iter() {
        for urgency in cns.urgencies.iter() {
            let key = (agent, urgency.source);
            let last = history.last_band.get(&key).copied().unwrap_or(0);
            let next = urgency_band(urgency.value, last);
            if next != last {
                history.last_band.insert(key, next);
                wakeups.write(BrainWakeup {
                    tick: tick.current,
                    agent,
                    reason: BrainTrigger::DriveThreshold,
                });
            }
        }
    }
}

pub fn emit_new_perception_wakeups(
    tick: Res<TickCount>,
    agents: Query<(Entity, &VisibleObjects), With<Agent>>,
    mut history: ResMut<PerceptionHistory>,
    mut wakeups: MessageWriter<BrainWakeup>,
) {
    for (agent, visible) in agents.iter() {
        let current: HashSet<Entity> = visible.entities.iter().copied().collect();
        let prev = history.last_visible.entry(agent).or_default();
        let any_new = current.iter().any(|e| !prev.contains(e));
        if any_new {
            wakeups.write(BrainWakeup {
                tick: tick.current,
                agent,
                reason: BrainTrigger::NewPerception,
            });
        }
        *prev = current;
    }
}

pub fn emit_knowledge_change_wakeups(
    tick: Res<TickCount>,
    mut events: MessageReader<SimEvent>,
    mut wakeups: MessageWriter<BrainWakeup>,
) {
    for event in events.read() {
        let SimEventKind::MindGraphMutation { agent, .. } = &event.kind else {
            continue;
        };
        wakeups.write(BrainWakeup {
            tick: tick.current,
            agent: *agent,
            reason: BrainTrigger::KnowledgeChanged,
        });
    }
}

pub fn emit_conversation_state_wakeups(
    tick: Res<TickCount>,
    changed: Query<Entity, (With<Agent>, Changed<InConversation>)>,
    removed: RemovedComponents<InConversation>,
    mut wakeups: MessageWriter<BrainWakeup>,
) {
    for agent in changed.iter() {
        wakeups.write(BrainWakeup {
            tick: tick.current,
            agent,
            reason: BrainTrigger::ConversationStateChanged,
        });
    }
    let mut iter = removed;
    for agent in iter.read() {
        wakeups.write(BrainWakeup {
            tick: tick.current,
            agent,
            reason: BrainTrigger::ConversationStateChanged,
        });
    }
}

pub fn emit_periodic_safety_wakeups(
    tick: Res<TickCount>,
    agents: Query<Entity, With<Agent>>,
    mut wakeups: MessageWriter<BrainWakeup>,
) {
    for agent in agents.iter() {
        if (agent.index_u32() as u64)
            .wrapping_add(tick.current)
            .is_multiple_of(PERIODIC_SAFETY_PERIOD)
        {
            wakeups.write(BrainWakeup {
                tick: tick.current,
                agent,
                reason: BrainTrigger::PeriodicSafety,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urgency_band_classifies_simple_values() {
        assert_eq!(urgency_band(0.0, 0), 0);
        assert_eq!(urgency_band(0.4, 0), 1);
        assert_eq!(urgency_band(0.6, 0), 2);
        assert_eq!(urgency_band(0.9, 0), 3);
    }

    #[test]
    fn urgency_band_hysteresis_holds_through_oscillation() {
        // Once we crossed up to band 1 (>= 0.3), values down to 0.2 stay
        // in band 1 — so a value oscillating between 0.25 and 0.32 won't
        // toggle.
        let after_up = urgency_band(0.32, 0);
        assert_eq!(after_up, 1);
        let oscillating_down = urgency_band(0.25, after_up);
        assert_eq!(oscillating_down, 1, "0.25 within hysteresis band, no drop");
        let crossed_down = urgency_band(0.15, after_up);
        assert_eq!(crossed_down, 0, "0.15 below the down-band, drops");
    }

    #[test]
    fn periodic_safety_period_covers_every_agent_within_window() {
        let mut hits = vec![0usize; 32];
        for tick in 0..PERIODIC_SAFETY_PERIOD {
            for idx in 0..32u32 {
                if (idx as u64)
                    .wrapping_add(tick)
                    .is_multiple_of(PERIODIC_SAFETY_PERIOD)
                {
                    hits[idx as usize] += 1;
                }
            }
        }
        for (idx, count) in hits.iter().enumerate() {
            assert_eq!(
                *count, 1,
                "agent {idx} fired {count} times in a {PERIODIC_SAFETY_PERIOD}-tick window",
            );
        }
    }
}
