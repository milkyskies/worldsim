//! Relationship dynamics — updates the central `SocialGraph` resource
//! from `GameEvent::SocialInteraction` and decays edges on a slow tick.
//!
//! Reads: GameEvent, Personality, RelationshipConfig
//! Writes: SocialGraph (canonical edges), RelationshipHistory (per-agent log),
//!         SocialIdentity (introductions), SimEvent::RelationshipChanged
//! Upstream: events (SocialInteraction), psyche::social_graph (resource shape)
//! Downstream: every reader of affection/trust/respect

use std::collections::{HashMap, VecDeque};

use crate::agent::Agent;
use crate::agent::events::SimEventKind;
use crate::agent::events::{ConversationTopic, GameEvent};
use crate::agent::psyche::personality::Personality;
use crate::agent::psyche::social_graph::{NEUTRAL, RelationshipEdge, SocialGraph};
use crate::core::tick::TickCount;
use crate::core::time::GameTime;
use bevy::prelude::*;

// ============================================================================
// Interaction history
// ============================================================================

/// Maximum number of interaction records kept per partner.
pub const MAX_INTERACTION_RECORDS: usize = 50;

/// A single recorded interaction between two agents.
#[derive(Debug, Clone, Reflect)]
pub struct InteractionRecord {
    /// Simulation tick when the interaction occurred.
    pub tick: u64,
    /// What kind of interaction this was.
    pub topic: Option<ConversationTopic>,
    /// Valence of the interaction (-1.0 hostile .. 1.0 friendly).
    pub valence: f32,
}

/// Per-agent log of interactions with other agents. Drives relationship
/// classification in `recognition.rs`.
#[derive(Component, Default, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct RelationshipHistory {
    /// Interaction records keyed by partner entity. Each partner's log is
    /// capped at [`MAX_INTERACTION_RECORDS`]; oldest entries are evicted.
    #[reflect(ignore)]
    pub logs: HashMap<Entity, VecDeque<InteractionRecord>>,
}

impl RelationshipHistory {
    /// Push a new interaction record for a partner, evicting the oldest if
    /// the log exceeds [`MAX_INTERACTION_RECORDS`].
    pub fn push(&mut self, partner: Entity, record: InteractionRecord) {
        let log = self.logs.entry(partner).or_default();
        if log.len() >= MAX_INTERACTION_RECORDS {
            log.pop_front();
        }
        log.push_back(record);
    }

    /// Returns the interaction log for a partner, or an empty slice.
    pub fn get(&self, partner: Entity) -> &VecDeque<InteractionRecord> {
        static EMPTY: VecDeque<InteractionRecord> = VecDeque::new();
        self.logs.get(&partner).unwrap_or(&EMPTY)
    }
}

/// Configuration for relationship dynamics
#[derive(Resource, Reflect)]
#[reflect(Resource)]
pub struct RelationshipConfig {
    /// Trust gain per positive interaction
    pub positive_trust_gain: f32,
    /// Affection gain per positive interaction
    pub positive_affection_gain: f32,
    /// Trust loss per negative interaction (larger = more impactful)
    pub negative_trust_loss: f32,
    /// Affection loss per negative interaction
    pub negative_affection_loss: f32,
    /// Respect gain when witnessing competence
    pub competence_respect_gain: f32,
    /// Half-life (in game days) for the weakest bonds — acquaintances.
    /// Short half-life → casual ties fade within a week.
    pub weak_bond_half_life_days: f32,
    /// Half-life (in game days) for the strongest bonds — close friends, sworn enemies.
    /// Long half-life → lifelong bonds barely erode within a game year.
    pub strong_bond_half_life_days: f32,
    /// Multiplier on half-life when `current < neutral` (grudges / distrust).
    /// Values > 1.0 make negative feelings linger longer than positive ones (negativity bias).
    pub negativity_bias: f32,
    /// Interval (in ticks) between decay fires. Tests can set this small to
    /// avoid ticking through a full game day in the test harness.
    pub decay_interval_ticks: u64,
    /// Grace period (in ticks). Any relationship updated within this window
    /// of the current tick is skipped by decay — recent contact maintains closeness.
    pub decay_grace_ticks: u64,
    /// How much elapsed game time (in days) each decay fire represents.
    /// Production uses 1.0 (each fire = 1 day); tests override to any value.
    pub decay_step_days: f32,
}

impl Default for RelationshipConfig {
    fn default() -> Self {
        Self {
            positive_trust_gain: 0.05,
            positive_affection_gain: 0.03,
            negative_trust_loss: 0.15, // 3x larger than positive - negativity bias!
            negative_affection_loss: 0.10,
            competence_respect_gain: 0.02,
            // Decay timescales chosen so that, at the default 60-day game year:
            //   acquaintance (strength=0.1): ~4 day half-life → gone in a week
            //   close friend  (strength=1.0): ~60 day half-life → lasts a full year
            weak_bond_half_life_days: 3.0,
            strong_bond_half_life_days: 60.0,
            negativity_bias: 1.5,
            decay_interval_ticks: GameTime::TICKS_PER_DAY,
            decay_grace_ticks: GameTime::TICKS_PER_DAY,
            decay_step_days: 1.0,
        }
    }
}

/// Apply a `SocialInteraction` event's effect on the target's directed
/// edge toward the actor. `events` carries the interaction payload, the
/// queries provide name/personality/history/social-id components, and
/// `graph` is the canonical relationship store this writes through.
pub fn update_relationships(
    mut events: MessageReader<GameEvent>,
    actors: Query<&Name, With<Agent>>,
    targets: Query<&Personality, With<Agent>>,
    mut histories: Query<&mut RelationshipHistory, With<Agent>>,
    mut social_ids: Query<&mut crate::agent::mind::social_identity::SocialIdentity, With<Agent>>,
    mut graph: ResMut<SocialGraph>,
    config: Res<RelationshipConfig>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    let now = tick.current;

    for event in events.read() {
        let GameEvent::SocialInteraction {
            actor,
            target,
            topic,
            valence,
            ..
        } = event
        else {
            continue;
        };

        let Ok(actor_name) = actors.get(*actor) else {
            continue;
        };
        let actor_name_str = actor_name.to_string();
        let Ok(personality) = targets.get(*target) else {
            continue;
        };

        if let Ok(mut history) = histories.get_mut(*target) {
            history.push(
                *actor,
                InteractionRecord {
                    tick: now,
                    topic: *topic,
                    valence: *valence,
                },
            );
        }

        if let Ok(mut sid) = social_ids.get_mut(*target)
            && !sid.knows(*actor)
        {
            sid.introduce(
                *actor,
                crate::agent::mind::knowledge::AgentName(actor_name_str.clone()),
                now,
            );
        }

        if !graph.knows(*target, *actor) {
            graph.set(
                *target,
                *actor,
                RelationshipEdge::with_baseline_affection(NEUTRAL, now),
            );
        }

        let edge_before = graph
            .get(*target, *actor)
            .copied()
            .unwrap_or_else(RelationshipEdge::default);
        let (trust_delta, affection_delta) =
            valence_to_deltas(*valence, *topic, &config, &personality.traits);

        let new_trust = (edge_before.trust + trust_delta).clamp(0.0, 1.0);
        let new_affection = (edge_before.affection + affection_delta).clamp(0.0, 1.0);

        if let Some(edge) = graph.get_mut(*target, *actor) {
            edge.trust = new_trust;
            edge.affection = new_affection;
            edge.last_interaction_tick = now;
        }

        if (new_trust - edge_before.trust).abs() > f32::EPSILON {
            sim_events.write(crate::agent::events::SimEvent::pair(
                now,
                *target,
                *actor,
                SimEventKind::RelationshipChanged {
                    agent: *target,
                    other: *actor,
                    dimension: crate::agent::events::RelationshipDimension::Trust,
                    old_value: edge_before.trust,
                    new_value: new_trust,
                },
            ));
        }
        if (new_affection - edge_before.affection).abs() > f32::EPSILON {
            sim_events.write(crate::agent::events::SimEvent::pair(
                now,
                *target,
                *actor,
                SimEventKind::RelationshipChanged {
                    agent: *target,
                    other: *actor,
                    dimension: crate::agent::events::RelationshipDimension::Affection,
                    old_value: edge_before.affection,
                    new_value: new_affection,
                },
            ));
        }
    }
}

/// Convert raw `(valence, topic, personality)` into the per-step trust
/// and affection deltas. Negative interactions hit harder than positive
/// ones do (negativity bias), and the topic biases which dimension
/// shifts more.
fn valence_to_deltas(
    valence: f32,
    topic: Option<ConversationTopic>,
    config: &RelationshipConfig,
    traits: &crate::agent::psyche::personality::PersonalityTraits,
) -> (f32, f32) {
    let (raw_trust, raw_affection) = if valence > 0.0 {
        let trust_gain = config.positive_trust_gain * valence;
        let affection_gain = config.positive_affection_gain * valence;
        let (t_mod, a_mod) = match topic {
            Some(ConversationTopic::Feelings) => (0.5, 1.5),
            Some(ConversationTopic::Knowledge) => (1.2, 0.8),
            Some(ConversationTopic::Gossip) => (0.8, 1.0),
            _ => (1.0, 1.0),
        };
        (trust_gain * t_mod, affection_gain * a_mod)
    } else {
        (
            -config.negative_trust_loss * valence.abs(),
            -config.negative_affection_loss * valence.abs(),
        )
    };

    let agreeableness = traits.agreeableness();
    let trust_mod = if raw_trust > 0.0 {
        0.5 + agreeableness
    } else {
        1.5 - agreeableness
    };
    let neuroticism = traits.neuroticism();
    let negative_mod = if raw_trust < 0.0 {
        1.0 + neuroticism * 0.5
    } else {
        1.0
    };

    (
        raw_trust * trust_mod * negative_mod,
        raw_affection * negative_mod,
    )
}

// `NEUTRAL` lives on `social_graph::NEUTRAL`; relationship decay pulls
// edges toward that midpoint.

/// Compute the fraction of the distance to neutral that a relationship should
/// decay over one step, given the step size and the current value.
///
/// Uses an exponential half-life model: `frac = 1 - 0.5^(step/half_life)`.
/// The half-life scales with bond strength (how far from neutral), so strong
/// bonds resist decay and weak ties fade quickly. Negative feelings (below
/// neutral) decay with a longer half-life — grudges linger.
fn decay_fraction(current: f32, step_days: f32, config: &RelationshipConfig) -> f32 {
    // Bond strength: 0.0 at neutral, 1.0 at the extremes.
    let strength = ((current - NEUTRAL).abs() * 2.0).clamp(0.0, 1.0);

    // Interpolate half-life between the weak and strong bounds.
    let base_half_life = config.weak_bond_half_life_days
        + (config.strong_bond_half_life_days - config.weak_bond_half_life_days) * strength;

    // Apply negativity bias: below-neutral feelings decay slower.
    let half_life = if current < NEUTRAL {
        base_half_life * config.negativity_bias
    } else {
        base_half_life
    };

    1.0 - 0.5_f32.powf(step_days / half_life)
}

/// System: Decay relationships toward neutral over time without contact.
///
/// Fires every `config.decay_interval_ticks` (default: once per game day).
/// Uses an exponential half-life that scales with bond strength, so close
/// friends take a full in-game year to fade while acquaintances fade within
/// a week. A grace period skips any relationship refreshed by a recent
/// interaction.
pub fn decay_relationships(
    mut graph: ResMut<SocialGraph>,
    tick: Res<TickCount>,
    config: Res<RelationshipConfig>,
) {
    if config.decay_interval_ticks == 0 || !tick.current.is_multiple_of(config.decay_interval_ticks)
    {
        return;
    }

    let now = tick.current;
    let grace_ticks = config.decay_grace_ticks;
    let step_days = config.decay_step_days;

    for (_, _, edge) in graph.iter_mut() {
        if now.saturating_sub(edge.last_interaction_tick) < grace_ticks {
            continue;
        }
        edge.trust = pull_toward_neutral(edge.trust, step_days, &config);
        edge.affection = pull_toward_neutral(edge.affection, step_days, &config);
        edge.respect = pull_toward_neutral(edge.respect, step_days, &config);
    }
}

/// One step of half-life-based pull toward `NEUTRAL`. Strong bonds
/// resist decay (long half-life), weak ties fade quickly, and grudges
/// linger via the negativity-bias multiplier on below-neutral values.
fn pull_toward_neutral(current: f32, step_days: f32, config: &RelationshipConfig) -> f32 {
    let fraction = decay_fraction(current, step_days, config);
    (current + (NEUTRAL - current) * fraction).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RelationshipConfig {
        RelationshipConfig::default()
    }

    /// Strong bonds (trust near the extremes) should barely decay in one day
    /// thanks to the ~60-day half-life at maximum strength.
    #[test]
    fn strong_trust_barely_decays_in_one_day() {
        let config = test_config();
        let fraction = decay_fraction(0.95, 1.0, &config);
        // Half-life ≈ 3 + (60-3)*0.9 ≈ 54.3 days → frac ≈ 1 - 0.5^(1/54.3) ≈ 0.0127
        assert!(
            fraction < 0.02,
            "strong trust should decay <2% per day, got {fraction}"
        );
        assert!(fraction > 0.0, "some decay should occur");
    }

    /// Weak bonds decay quickly — a few percent per day at the weakest half-life.
    #[test]
    fn weak_trust_decays_quickly() {
        let config = test_config();
        let fraction = decay_fraction(0.55, 1.0, &config);
        // Half-life ≈ 3 + (60-3)*0.1 ≈ 8.7 days → frac ≈ 0.077
        assert!(
            fraction > 0.05,
            "weak trust should decay >5% per day, got {fraction}"
        );
    }

    /// Negative trust decays slower than symmetric positive trust — grudges linger.
    #[test]
    fn negative_trust_lingers_longer_than_positive() {
        let config = test_config();
        let positive_fraction = decay_fraction(0.8, 1.0, &config);
        let negative_fraction = decay_fraction(0.2, 1.0, &config);
        assert!(
            negative_fraction < positive_fraction,
            "negativity bias: negative ({negative_fraction}) should decay slower \
             than positive ({positive_fraction})"
        );
    }

    /// At neutral (0.5) exactly, strength is 0 so the half-life is the weakest.
    /// The fraction is still a positive number, but applied to a zero distance
    /// → no actual change. This just verifies the math doesn't NaN.
    #[test]
    fn decay_at_neutral_is_finite() {
        let config = test_config();
        let fraction = decay_fraction(0.5, 1.0, &config);
        assert!(
            fraction.is_finite() && fraction >= 0.0,
            "fraction at neutral should be finite and non-negative, got {fraction}"
        );
    }
}
