//! Relationship dynamics - updates relationships based on social interactions.
//!
//! When social interactions occur:
//! - Trust, affection, and respect are updated
//! - Positive interactions increase values (small amounts)
//! - Negative interactions decrease values (larger amounts - negativity bias!)
//! - Relationships decay slowly without contact
//! - An [`InteractionRecord`] is pushed into [`RelationshipHistory`]
//!
//! The interaction log drives relationship *classification* (Friend, Enemy, etc.)
//! in `recognition.rs`, replacing the old threshold-based approach. Trust and
//! affection floats remain as cached summaries that other systems read.
//!
//! Emits SimEvent::RelationshipChanged on trust/affection updates.

use std::collections::{HashMap, VecDeque};

use crate::agent::Agent;
use crate::agent::events::SimEventKind;
use crate::agent::events::{ConversationTopic, GameEvent};
use crate::agent::mind::knowledge::{
    Metadata, MindGraph, Node, Predicate, Quantity, Triple, Value,
};
use crate::agent::psyche::personality::Personality;
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

/// System: Update relationships based on social interaction events
pub fn update_relationships(
    mut events: MessageReader<GameEvent>,
    mut agents: Query<
        (
            Entity,
            &Name,
            &mut MindGraph,
            &mut crate::agent::mind::social_identity::SocialIdentity,
            &Personality,
            &mut RelationshipHistory,
        ),
        With<Agent>,
    >,
    config: Res<RelationshipConfig>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    let current_time = tick.current;

    for event in events.read() {
        if let GameEvent::SocialInteraction {
            actor,
            target,
            action: _,
            topic,
            valence,
        } = event
        {
            // Update target's feelings about actor (the one who did the action)
            if let Ok((_, actor_name, _, _, _, _)) = agents.get(*actor) {
                let actor_name_str = actor_name.to_string();

                if let Ok((_, _, mut target_mind, mut target_social, personality, mut history)) =
                    agents.get_mut(*target)
                {
                    // Record the interaction in the log.
                    history.push(
                        *actor,
                        InteractionRecord {
                            tick: current_time,
                            topic: *topic,
                            valence: *valence,
                        },
                    );
                    let actor_node = Node::Entity(*actor);

                    // First meeting? Initialize the social ledger entry +
                    // neutral relationship dimensions.
                    if !target_social.knows(*actor) {
                        target_social.introduce(
                            *actor,
                            crate::agent::mind::knowledge::AgentName(actor_name_str.clone()),
                            current_time,
                        );
                        crate::agent::mind::recognition::init_relationship_dimensions(
                            &mut target_mind,
                            *actor,
                            current_time,
                            0.5,
                        );
                    }

                    // Get current values
                    let current_trust = target_mind
                        .get(&actor_node, Predicate::Trust)
                        .and_then(|v| v.as_quantity().map(|q| q.point_estimate()))
                        .unwrap_or(0.5);

                    let current_affection = target_mind
                        .get(&actor_node, Predicate::Affection)
                        .and_then(|v| v.as_quantity().map(|q| q.point_estimate()))
                        .unwrap_or(0.5);

                    // Calculate changes based on valence
                    let (trust_delta, affection_delta) = if *valence > 0.0 {
                        // Positive interaction
                        let trust_gain = config.positive_trust_gain * valence;
                        let affection_gain = config.positive_affection_gain * valence;

                        // Topic modifiers
                        let (t_mod, a_mod) = match topic {
                            Some(ConversationTopic::Feelings) => (0.5, 1.5), // Feelings build affection
                            Some(ConversationTopic::Knowledge) => (1.2, 0.8), // Knowledge builds trust
                            Some(ConversationTopic::Gossip) => (0.8, 1.0),
                            _ => (1.0, 1.0),
                        };

                        (trust_gain * t_mod, affection_gain * a_mod)
                    } else {
                        // Negative interaction - larger impact!
                        let trust_loss = config.negative_trust_loss * valence.abs();
                        let affection_loss = config.negative_affection_loss * valence.abs();
                        (-trust_loss, -affection_loss)
                    };

                    // Apply personality modifiers
                    // High agreeableness = bigger trust gains, smaller losses
                    let agreeableness = personality.traits.agreeableness();
                    let trust_mod = if trust_delta > 0.0 {
                        0.5 + agreeableness
                    } else {
                        1.5 - agreeableness
                    };

                    // High neuroticism = bigger negativity impact
                    let neuroticism = personality.traits.neuroticism();
                    let negative_mod = if trust_delta < 0.0 {
                        1.0 + neuroticism * 0.5
                    } else {
                        1.0
                    };

                    // Calculate new values
                    let new_trust =
                        (current_trust + trust_delta * trust_mod * negative_mod).clamp(0.0, 1.0);
                    let new_affection =
                        (current_affection + affection_delta * negative_mod).clamp(0.0, 1.0);

                    // Update MindGraph
                    target_mind.assert(Triple::with_meta(
                        actor_node.clone(),
                        Predicate::Trust,
                        Value::Quantity(Quantity::Exact(new_trust)),
                        Metadata::semantic(current_time),
                    ));

                    target_mind.assert(Triple::with_meta(
                        actor_node,
                        Predicate::Affection,
                        Value::Quantity(Quantity::Exact(new_affection)),
                        Metadata::semantic(current_time),
                    ));

                    // Emit SimEvents for relationship changes
                    if (new_trust - current_trust).abs() > f32::EPSILON {
                        sim_events.write(crate::agent::events::SimEvent::pair(
                            current_time,
                            *target,
                            *actor,
                            SimEventKind::RelationshipChanged {
                                agent: *target,
                                other: *actor,
                                dimension: crate::agent::events::RelationshipDimension::Trust,
                                old_value: current_trust,
                                new_value: new_trust,
                            },
                        ));
                    }
                    if (new_affection - current_affection).abs() > f32::EPSILON {
                        sim_events.write(crate::agent::events::SimEvent::pair(
                            current_time,
                            *target,
                            *actor,
                            SimEventKind::RelationshipChanged {
                                agent: *target,
                                other: *actor,
                                dimension: crate::agent::events::RelationshipDimension::Affection,
                                old_value: current_affection,
                                new_value: new_affection,
                            },
                        ));
                    }
                }
            }
        }
    }
}

/// Neutral value for trust/affection. Decay pulls values toward this.
pub const NEUTRAL: f32 = 0.5;

/// Read the observer's stored affection toward `target`. Returns
/// `NEUTRAL` when no relationship triple exists, matching the decay
/// target the rest of the system pulls toward.
pub fn affection_toward(mind: &MindGraph, target: Entity) -> f32 {
    match mind.get(&Node::Entity(target), Predicate::Affection) {
        Some(Value::Quantity(Quantity::Exact(v))) => *v,
        _ => NEUTRAL,
    }
}

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
    mut agents: Query<&mut MindGraph, With<Agent>>,
    tick: Res<TickCount>,
    config: Res<RelationshipConfig>,
) {
    if config.decay_interval_ticks == 0 || !tick.current.is_multiple_of(config.decay_interval_ticks)
    {
        return;
    }

    let current_time = tick.current;
    let grace_ticks = config.decay_grace_ticks;
    let step_days = config.decay_step_days;

    for mut mind in agents.iter_mut() {
        decay_predicate(
            &mut mind,
            Predicate::Trust,
            current_time,
            grace_ticks,
            step_days,
            &config,
        );
        decay_predicate(
            &mut mind,
            Predicate::Affection,
            current_time,
            grace_ticks,
            step_days,
            &config,
        );
    }
}

/// Apply decay to every `(entity, predicate)` edge in a single MindGraph.
fn decay_predicate(
    mind: &mut MindGraph,
    predicate: Predicate,
    current_time: u64,
    grace_ticks: u64,
    step_days: f32,
    config: &RelationshipConfig,
) {
    let entries: Vec<(Entity, f32, u64)> = mind
        .query(None, Some(predicate), None)
        .into_iter()
        .filter_map(|t| {
            if let Node::Entity(e) = &t.subject
                && let Value::Quantity(q) = &t.object
            {
                return Some((*e, q.point_estimate(), t.meta.timestamp));
            }
            None
        })
        .collect();

    for (entity, current, last_updated) in entries {
        // Grace period: skip recently refreshed relationships.
        if current_time.saturating_sub(last_updated) < grace_ticks {
            continue;
        }

        let fraction = decay_fraction(current, step_days, config);
        let new_value = (current + (NEUTRAL - current) * fraction).clamp(0.0, 1.0);

        mind.assert(Triple::with_meta(
            Node::Entity(entity),
            predicate,
            Value::Quantity(Quantity::Exact(new_value)),
            Metadata::semantic(current_time),
        ));
    }
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
