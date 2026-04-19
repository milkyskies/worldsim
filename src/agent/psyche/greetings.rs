//! Social acknowledgments — lightweight passing greetings between agents.
//!
//! Reads: VisibleObjects, MindGraph, PsychologicalDrives, EmotionalState, InConversation
//! Writes: PsychologicalDrives (companionship bump), GameEvent (SocialInteraction), SimEvent (SocialAcknowledgment)
//! Upstream: perception (VisibleObjects), recognition (relationship initialization)
//! Downstream: relationships (consumes SocialInteraction), flocking (reads companionship)
//!
//! When two agents who know each other pass within perception range, this
//! system fires a brief social acknowledgment — a nod, wave, or "hey!" —
//! without stopping or entering the conversation state machine. This is the
//! most common form of social contact in real life and gives agents a steady
//! trickle of companionship satisfaction between full conversations.

use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::actions::ActionType;
use crate::agent::body::needs::PsychologicalDrives;
use crate::agent::events::{GameEvent, SimEvent, SimEventKind};
use crate::agent::mind::conversation::InConversation;
use crate::agent::mind::knowledge::{MindGraph, Node, Predicate};
use crate::agent::mind::perception::VisibleObjects;
use crate::agent::psyche::emotions::EmotionalState;
use crate::core::tick::TickCount;

/// Minimum affection to count as "knows this person well enough to greet."
const FAMILIARITY_THRESHOLD: f32 = 0.1;

/// Companionship bump for greeting an acquaintance (affection ~0.2-0.4).
const ACQUAINTANCE_COMPANIONSHIP_BUMP: f32 = 0.003;

/// Companionship bump for greeting a friend (affection > 0.5).
const FRIEND_COMPANIONSHIP_BUMP: f32 = 0.008;

/// Ticks between acknowledgments for the same pair. At 60 tps this is ~5
/// seconds — you don't wave at someone you just waved at.
const GREETING_COOLDOWN_TICKS: u64 = 300;

/// Base valence of a greeting interaction. Friendly but mild.
const GREETING_VALENCE: f32 = 0.2;

/// Run the greeting check every N ticks per agent, staggered. Greetings
/// are not urgent — checking once per second is plenty.
const CHECK_INTERVAL: u64 = 60;

/// Tracks recent greetings to enforce per-pair cooldowns.
#[derive(Resource, Default)]
pub struct GreetingCooldowns {
    entries: Vec<(Entity, Entity, u64)>,
}

impl GreetingCooldowns {
    fn is_on_cooldown(&self, a: Entity, b: Entity, now: u64) -> bool {
        self.entries.iter().any(|&(ea, eb, tick)| {
            ((ea == a && eb == b) || (ea == b && eb == a))
                && now.saturating_sub(tick) < GREETING_COOLDOWN_TICKS
        })
    }

    fn record(&mut self, a: Entity, b: Entity, now: u64) {
        self.entries
            .retain(|&(_, _, tick)| now.saturating_sub(tick) < GREETING_COOLDOWN_TICKS);
        self.entries.push((a, b, now));
    }
}

pub fn social_acknowledgments(
    tick: Res<TickCount>,
    mut cooldowns: ResMut<GreetingCooldowns>,
    mut agents: Query<
        (
            Entity,
            &VisibleObjects,
            &MindGraph,
            &mut PsychologicalDrives,
            &EmotionalState,
            Option<&InConversation>,
        ),
        With<Agent>,
    >,
    mut game_events: MessageWriter<GameEvent>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    let now = tick.current;

    let mut greetings: Vec<(Entity, Entity, f32)> = Vec::new();

    for (agent, visible, mind, _drives, emotions, in_conversation) in agents.iter() {
        if !tick.should_run(agent, CHECK_INTERVAL) {
            continue;
        }

        // Agents already in a conversation don't need passing greetings.
        if in_conversation.is_some() {
            continue;
        }

        for &other in &visible.entities {
            if other == agent {
                continue;
            }

            if cooldowns.is_on_cooldown(agent, other, now) {
                continue;
            }

            let Some(affection) = mind
                .get(&Node::Entity(other), Predicate::Affection)
                .and_then(|v| v.as_quantity())
                .map(|q| q.point_estimate())
            else {
                continue;
            };

            if affection < FAMILIARITY_THRESHOLD {
                continue;
            }

            let bump = if affection > 0.5 {
                FRIEND_COMPANIONSHIP_BUMP
            } else {
                ACQUAINTANCE_COMPANIONSHIP_BUMP
            };

            // Mood modifier: happy agents give warmer greetings.
            let mood_modifier = 1.0 + emotions.current_mood * 0.3;
            let bump = bump * mood_modifier.max(0.3);

            greetings.push((agent, other, bump));
            cooldowns.record(agent, other, now);
        }
    }

    for (actor, target, bump) in greetings {
        if let Ok((_, _, _, mut drives, _, _)) = agents.get_mut(actor) {
            drives.companionship.top_up(bump);
        }

        game_events.write(GameEvent::SocialInteraction {
            actor,
            target,
            action: ActionType::Idle,
            topic: None,
            valence: GREETING_VALENCE,
        });

        sim_events.write(SimEvent::pair(
            now,
            actor,
            target,
            SimEventKind::SocialAcknowledgment { actor, target },
        ));
    }
}
