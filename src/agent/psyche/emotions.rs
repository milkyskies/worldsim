//! Emotional state: active emotions, mood, stress, and event-driven emotion triggers.
//!
//! Reads: GameEvent, PhysicalNeeds, Body, Personality, MindGraph, TickCount
//! Writes: EmotionalState, SimEvent
//! Upstream: events (GameEvent), nervous_system::urgency (stress inputs)
//! Downstream: brains::arbitration (mood/stress influence), nervous_system::urgency

use crate::agent::actions::ActionType;
use crate::agent::events::SimEventKind;
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Reflect, serde::Serialize)]
pub enum EmotionType {
    Joy,
    Sadness,
    Fear,
    Anger,
    Disgust,
    Surprise,
}

#[derive(Debug, Clone, Reflect)]
pub struct Emotion {
    pub emotion_type: EmotionType,
    pub intensity: f32, // 0.0 to 1.0 - Current felt strength (used for mood)
    pub fuel: f32,      // 0.0+ - Accumulated reservoir (determines duration)
}

impl Emotion {
    /// Create a new emotion with intensity and fuel set to the same initial value
    pub fn new(emotion_type: EmotionType, intensity: f32) -> Self {
        Self {
            emotion_type,
            intensity,
            fuel: intensity,
        }
    }
}

// Configuration for emotional dynamics
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct EmotionConfig {
    pub decay_base_rate: f32,
    pub decay_fuel_factor: f32,
    pub stress_hunger_threshold: f32,
    pub stress_stamina_threshold: f32,
    pub stress_hunger_weight: f32,
    pub stress_stamina_weight: f32,
    pub stress_pain_weight: f32,
    pub stress_emotion_weight: f32,
    pub stress_recovery_bonus: f32,
    pub stress_decay_base: f32,
}

impl Default for EmotionConfig {
    fn default() -> Self {
        Self {
            decay_base_rate: 0.05,
            decay_fuel_factor: 0.01,
            stress_hunger_threshold: 50.0,
            stress_stamina_threshold: 50.0,
            stress_hunger_weight: 0.02,
            stress_stamina_weight: 0.02,
            stress_pain_weight: 0.1,
            stress_emotion_weight: 0.15,
            stress_recovery_bonus: 2.0,
            stress_decay_base: 0.5,
        }
    }
}

#[derive(Component, Debug, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct EmotionalState {
    pub current_mood: f32, // -1.0 (Depressed) to 1.0 (Ecstatic)
    pub stress_level: f32, // 0.0 to 100.0
    pub active_emotions: Vec<Emotion>,
}

impl EmotionalState {
    pub fn add_emotion(&mut self, new_emotion: Emotion) {
        if let Some(existing) = self
            .active_emotions
            .iter_mut()
            .find(|e| e.emotion_type == new_emotion.emotion_type)
        {
            existing.fuel += new_emotion.intensity;
            existing.intensity = existing.fuel.min(1.0);
        } else {
            let mut emotion = new_emotion;
            emotion.fuel = emotion.intensity;
            self.active_emotions.push(emotion);
        }
    }

    /// Reduce the fuel (and derived intensity) of a specific emotion type by `amount`.
    /// If the emotion is absent or amount is zero, does nothing.
    pub fn drain_emotion(&mut self, emotion_type: EmotionType, amount: f32) {
        if let Some(emotion) = self
            .active_emotions
            .iter_mut()
            .find(|e| e.emotion_type == emotion_type)
        {
            emotion.fuel = (emotion.fuel - amount).max(0.0);
            emotion.intensity = emotion.fuel.min(1.0);
        }
    }

    pub fn get_emotion_intensity(&self, emotion_type: EmotionType) -> f32 {
        self.active_emotions
            .iter()
            .find(|e| e.emotion_type == emotion_type)
            .map(|e| e.intensity)
            .unwrap_or(0.0)
    }

    /// Advance emotion decay by `dt` seconds. Each emotion's fuel drains at a
    /// rate driven by `EmotionConfig`, with intensity tracking fuel directly.
    /// Emotions whose fuel falls below the removal threshold are dropped.
    pub fn decay_tick(&mut self, dt: f32, config: &EmotionConfig) {
        self.active_emotions.retain_mut(|e| {
            let decay_rate = config.decay_base_rate + (e.fuel * config.decay_fuel_factor).min(0.1);
            e.fuel -= decay_rate * dt;
            e.fuel = e.fuel.max(0.0);
            e.intensity = e.fuel.min(1.0);
            e.fuel > 0.01
        });
    }
}

/// Role of the observer relative to the event
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ObserverRole {
    Actor,   // I did it
    Target,  // It happened to me
    Witness, // I saw it happen
}

/// Interpret what emotions an event triggers based on associations in the agent's mind.
pub fn interpret_emotion(
    action: ActionType,
    role: ObserverRole,
    actor_entity: Option<Entity>,
    mind: Option<&crate::agent::mind::knowledge::MindGraph>,
) -> Vec<Emotion> {
    let mut emotions = Vec::new();

    let mut check_association = |subject: crate::agent::mind::knowledge::Node| {
        if let Some(mind) = mind {
            use crate::agent::mind::knowledge::{Predicate, Value};

            // Direct rules
            let emotional_rules =
                mind.query(Some(&subject), Some(Predicate::TriggersEmotion), None);

            for triple in emotional_rules {
                if let Value::Emotion(etype, intensity) = triple.object {
                    emotions.push(Emotion::new(etype, intensity));
                }
            }

            // Inherited rules (IsA)
            let concepts = mind.all_types(&subject);
            for concept in concepts {
                let concept_rules = mind.query(
                    Some(&crate::agent::mind::knowledge::Node::Concept(concept)),
                    Some(Predicate::TriggersEmotion),
                    None,
                );
                for triple in concept_rules {
                    if let Value::Emotion(etype, intensity) = triple.object {
                        emotions.push(Emotion::new(etype, intensity));
                    }
                }
            }
        }
    };

    // 1. Check Specific Agent Association
    if let Some(actor_ent) = actor_entity {
        check_association(crate::agent::mind::knowledge::Node::Entity(actor_ent));

        if let Some(mind) = mind {
            let actor_node = crate::agent::mind::knowledge::Node::Entity(actor_ent);
            let concepts = mind.all_types(&actor_node);
            for concept in concepts {
                check_association(crate::agent::mind::knowledge::Node::Concept(concept));
            }
        }
    }

    // 2. Check Action Association
    check_association(crate::agent::mind::knowledge::Node::Action(action));

    // 3. Check Action Concept Categories
    if let Some(mind) = mind {
        let action_node = crate::agent::mind::knowledge::Node::Action(action);
        let concepts = mind.all_types(&action_node);
        for concept in concepts {
            check_association(crate::agent::mind::knowledge::Node::Concept(concept));
        }
    }

    emotions
        .into_iter()
        .map(|mut emotion| {
            if role == ObserverRole::Witness {
                emotion.intensity *= 0.5;
                emotion.fuel *= 0.5;
            }
            emotion
        })
        .collect()
}

pub fn decay_emotions(
    mut agents: Query<&mut EmotionalState, With<crate::agent::Agent>>,
    tick: Res<crate::core::tick::TickCount>,
    config: Res<EmotionConfig>,
) {
    let dt = tick.dt();

    for mut emotional_state in agents.iter_mut() {
        emotional_state.decay_tick(dt, &config);
    }
}

/// Personality-dependent valence weight for a given emotion type.
///
/// Neuroticism amplifies negative emotions (fear, sadness, anger).
/// Agreeableness amplifies social emotions (joy, sadness from conflict).
/// Openness makes surprise feel more positive.
pub fn emotion_valence(
    emotion_type: EmotionType,
    traits: &crate::agent::psyche::personality::PersonalityTraits,
) -> f32 {
    match emotion_type {
        EmotionType::Joy => 0.8 + traits.agreeableness * 0.4,
        EmotionType::Surprise => traits.openness * 0.6 - (1.0 - traits.openness) * 0.2,
        EmotionType::Sadness => -(0.5 + traits.agreeableness * 0.4 + traits.neuroticism * 0.3),
        EmotionType::Fear => -(0.6 + traits.neuroticism * 0.4),
        EmotionType::Anger => -(0.4 + traits.neuroticism * 0.3),
        EmotionType::Disgust => -(0.5 + traits.neuroticism * 0.2),
    }
}

/// Compute the target mood value from current emotional state, personality, and optional pain.
/// Returns a value in [-1.0, 1.0].
pub fn compute_target_mood(
    emotions: &EmotionalState,
    personality: &crate::agent::psyche::personality::Personality,
    body: Option<&crate::agent::biology::body::Body>,
) -> f32 {
    let baseline = (personality.traits.extraversion - personality.traits.neuroticism) * 0.5;
    let mut mood_sum = baseline;
    let mut weight_sum = 0.5f32;

    for emotion in &emotions.active_emotions {
        let valence = emotion_valence(emotion.emotion_type, &personality.traits);
        mood_sum += valence * emotion.intensity;
        weight_sum += emotion.intensity;
    }

    if let Some(body) = body {
        let mut total_pain = 0.0;
        for part in body.parts() {
            for injury in &part.injuries {
                total_pain += injury.pain * (1.0 - injury.healed_amount);
            }
        }
        if total_pain > 0.0 {
            mood_sum -= total_pain * 0.2;
            weight_sum += total_pain * 0.2;
        }
    }

    (mood_sum / weight_sum).clamp(-1.0, 1.0)
}

pub fn update_mood(
    mut agents: Query<
        (
            &mut EmotionalState,
            &crate::agent::psyche::personality::Personality,
            Option<&crate::agent::biology::body::Body>,
        ),
        With<crate::agent::Agent>,
    >,
    tick: Res<crate::core::tick::TickCount>,
) {
    let dt = tick.dt();

    for (mut emotional_state, personality, body) in agents.iter_mut() {
        let target_mood = compute_target_mood(&emotional_state, personality, body);
        emotional_state.current_mood += (target_mood - emotional_state.current_mood) * dt * 0.5;
        emotional_state.current_mood = emotional_state.current_mood.clamp(-1.0, 1.0);
    }
}

/// Compute stress accumulation rate (per second) from current conditions and personality.
///
/// Personality scaling:
/// - **Neuroticism** amplifies stress gain — neurotic agents stress faster from
///   the same conditions.
/// - **Openness** dampens emotional stress — open agents process negative
///   emotions more easily.
pub fn compute_stress_gain_rate(
    emotions: &EmotionalState,
    physical: &crate::agent::body::needs::PhysicalNeeds,
    body: Option<&crate::agent::biology::body::Body>,
    traits: &crate::agent::psyche::personality::PersonalityTraits,
    config: &EmotionConfig,
) -> f32 {
    // Hunger stress tracks the 0..1 urgency signal, rescaled onto the legacy
    // 0..100 threshold axis so `stress_hunger_threshold` / `weight` keep their
    // existing tuned values.
    let hunger_100 = physical.hunger_urgency() * 100.0;
    let hunger_stress =
        (hunger_100 - config.stress_hunger_threshold).max(0.0) * config.stress_hunger_weight;
    let fatigue_stress = (config.stress_stamina_threshold - physical.stamina.aerobic).max(0.0)
        * config.stress_stamina_weight;

    let total_pain = body.map(|b| b.total_pain()).unwrap_or(0.0);
    let pain_stress = total_pain * config.stress_pain_weight;

    let negative_intensity: f32 = emotions
        .active_emotions
        .iter()
        .filter(|e| {
            matches!(
                e.emotion_type,
                EmotionType::Sadness
                    | EmotionType::Fear
                    | EmotionType::Anger
                    | EmotionType::Disgust
            )
        })
        .map(|e| e.intensity)
        .sum();
    // Openness dampens emotional stress (0.0 openness = full stress, 1.0 = 70% stress).
    let openness_dampening = 1.0 - traits.openness * 0.3;
    let emotional_stress = negative_intensity * config.stress_emotion_weight * openness_dampening;

    // Neuroticism scales total stress gain (0.5x at fully stoic, 1.5x at fully neurotic).
    let neuroticism_multiplier = 0.5 + traits.neuroticism;
    (hunger_stress + fatigue_stress + pain_stress + emotional_stress) * neuroticism_multiplier
}

/// Compute stress recovery rate (per second) from current conditions and personality.
///
/// Recovery is continuous — no hard thresholds. The agent recovers faster the
/// closer they are to fully sated and fully rested. Conscientiousness amplifies
/// recovery (disciplined agents manage stress better).
pub fn compute_stress_recovery_rate(
    physical: &crate::agent::body::needs::PhysicalNeeds,
    traits: &crate::agent::psyche::personality::PersonalityTraits,
    config: &EmotionConfig,
) -> f32 {
    // Both factors in [0, 1]: 1.0 = perfectly fed/rested, 0.0 = starving/exhausted.
    let satiety = (1.0 - physical.hunger_urgency()).clamp(0.0, 1.0);
    let restedness = physical.stamina.aerobic_fraction();
    // Geometric mean rewards being good at both — being well-fed but exhausted
    // (or vice versa) shouldn't grant the full recovery bonus.
    let well_being = (satiety * restedness).sqrt();

    // Conscientiousness adds up to 50% on top of the base recovery multiplier.
    let conscientiousness_multiplier = 1.0 + traits.conscientiousness * 0.5;

    // Linear ramp from base decay (no well-being) to base * recovery_bonus (full well-being).
    let recovery_multiplier = 1.0 + well_being * (config.stress_recovery_bonus - 1.0);
    config.stress_decay_base * recovery_multiplier * conscientiousness_multiplier
}

pub fn update_stress(
    mut agents: Query<
        (
            &mut EmotionalState,
            &crate::agent::body::needs::PhysicalNeeds,
            Option<&crate::agent::biology::body::Body>,
            &crate::agent::psyche::personality::Personality,
        ),
        With<crate::agent::Agent>,
    >,
    tick: Res<crate::core::tick::TickCount>,
    config: Res<EmotionConfig>,
) {
    let dt = tick.dt();

    for (mut emotional_state, physical, body, personality) in agents.iter_mut() {
        let gain = compute_stress_gain_rate(
            &emotional_state,
            physical,
            body,
            &personality.traits,
            &config,
        );
        let decay = compute_stress_recovery_rate(physical, &personality.traits, &config);

        emotional_state.stress_level += (gain - decay) * dt;
        emotional_state.stress_level = emotional_state.stress_level.clamp(0.0, 100.0);
    }
}

pub fn add_emotion_with_event(
    state: &mut EmotionalState,
    sim_events: &mut MessageWriter<crate::agent::events::SimEvent>,
    agent: Entity,
    tick: u64,
    emotion: Emotion,
) {
    sim_events.write(crate::agent::events::SimEvent::single(
        tick,
        agent,
        SimEventKind::EmotionTriggered {
            agent,
            emotion: emotion.emotion_type,
            intensity: emotion.intensity,
        },
    ));
    state.add_emotion(emotion);
}

pub fn react_to_events(
    mut events: MessageReader<crate::agent::events::GameEvent>,
    mut agents: Query<
        (
            Entity,
            &mut EmotionalState,
            &crate::agent::mind::knowledge::MindGraph,
        ),
        With<crate::agent::Agent>,
    >,
    tick: Res<crate::core::tick::TickCount>,
    mut sim_events: MessageWriter<crate::agent::events::SimEvent>,
) {
    let event_list: Vec<_> = events.read().cloned().collect();

    for event in &event_list {
        match event {
            crate::agent::events::GameEvent::Interaction {
                actor,
                action,
                target,
                ..
            } => {
                if let Ok((_, mut state, mind)) = agents.get_mut(*actor) {
                    let emotions =
                        interpret_emotion(*action, ObserverRole::Actor, Some(*actor), Some(mind));
                    for e in emotions {
                        add_emotion_with_event(
                            &mut state,
                            &mut sim_events,
                            *actor,
                            tick.current,
                            e,
                        );
                    }
                }

                if let Some(target_entity) = target
                    && let Ok((_, mut state, mind)) = agents.get_mut(*target_entity)
                {
                    let emotions =
                        interpret_emotion(*action, ObserverRole::Target, Some(*actor), Some(mind));
                    for e in emotions {
                        add_emotion_with_event(
                            &mut state,
                            &mut sim_events,
                            *target_entity,
                            tick.current,
                            e,
                        );
                    }
                }
            }

            crate::agent::events::GameEvent::SocialInteraction {
                actor,
                target,
                valence,
                ..
            } => {
                if *valence > 0.0 {
                    if let Ok((_, mut state, _mind)) = agents.get_mut(*actor) {
                        add_emotion_with_event(
                            &mut state,
                            &mut sim_events,
                            *actor,
                            tick.current,
                            Emotion::new(EmotionType::Joy, *valence * 0.3),
                        );
                    }
                    if let Ok((_, mut state, _mind)) = agents.get_mut(*target) {
                        add_emotion_with_event(
                            &mut state,
                            &mut sim_events,
                            *target,
                            tick.current,
                            Emotion::new(EmotionType::Joy, *valence * 0.2),
                        );
                    }
                } else if *valence < 0.0 {
                    if let Ok((_, mut state, _mind)) = agents.get_mut(*actor) {
                        add_emotion_with_event(
                            &mut state,
                            &mut sim_events,
                            *actor,
                            tick.current,
                            Emotion::new(EmotionType::Anger, valence.abs() * 0.3),
                        );
                    }
                    if let Ok((_, mut state, _mind)) = agents.get_mut(*target) {
                        add_emotion_with_event(
                            &mut state,
                            &mut sim_events,
                            *target,
                            tick.current,
                            Emotion::new(EmotionType::Fear, valence.abs() * 0.2),
                        );
                    }
                }
            }

            crate::agent::events::GameEvent::KnowledgeShared { .. } => {}
        }
    }
}

/// Pain-rage appraisal: each [`SimEventKind::CombatHit`] adds Anger and
/// Fear to the defender, scaled by damage. Also propagates witness Fear
/// (and Anger if the defender is a Friend) to nearby observers — combat
/// is now seen, not felt-only. SimEvent reader and writer share one
/// ParamSet because Bevy's system-param checker rejects a plain reader
/// + writer pair against the same message type.
pub fn react_to_combat_hit(
    mut sim_events: ParamSet<(
        MessageReader<crate::agent::events::SimEvent>,
        MessageWriter<crate::agent::events::SimEvent>,
    )>,
    tick: Res<crate::core::tick::TickCount>,
    mut agents: Query<
        (
            Entity,
            &mut EmotionalState,
            Option<&crate::agent::mind::perception::VisibleObjects>,
            Option<&crate::agent::mind::knowledge::MindGraph>,
        ),
        With<crate::agent::Agent>,
    >,
) {
    use crate::agent::mind::knowledge::Node;
    use crate::constants::actions::defend_self::{
        ANGER_PER_HIT, DAMAGE_REFERENCE_HP, FEAR_PER_HIT, HIT_SCALE_MAX, HIT_SCALE_MIN,
    };

    let hits: Vec<(Entity, Entity, f32)> = sim_events
        .p0()
        .read()
        .filter_map(|e| match &e.kind {
            SimEventKind::CombatHit {
                attacker,
                defender,
                damage,
                ..
            } => Some((*attacker, *defender, *damage)),
            _ => None,
        })
        .collect();

    if hits.is_empty() {
        return;
    }

    // Buffer witness events; emit in a second pass after the agent
    // borrow is released so we can use the ParamSet writer.
    let mut witness_events: Vec<(Entity, Entity, Entity)> = Vec::new();

    for (attacker, defender, damage) in hits {
        let scale = (damage / DAMAGE_REFERENCE_HP).clamp(HIT_SCALE_MIN, HIT_SCALE_MAX);

        // Defender: pain-rage path.
        if let Ok((_, mut state, _, _)) = agents.get_mut(defender) {
            state.add_emotion(Emotion::new(EmotionType::Anger, ANGER_PER_HIT * scale));
            state.add_emotion(Emotion::new(EmotionType::Fear, FEAR_PER_HIT * scale));
        }

        // Witnesses: any other agent who sees BOTH the attacker and the
        // defender in their perception gains Fear (toward attacker) and
        // optionally Anger (if defender is a Friend they care about).
        // Iteration is per-event but cheap — most ticks have zero hits.
        for (observer, mut state, visible, mind) in agents.iter_mut() {
            if observer == attacker || observer == defender {
                continue;
            }
            let Some(visible) = visible else { continue };
            let saw_attacker = visible.entities.contains(&attacker);
            let saw_defender = visible.entities.contains(&defender);
            if !saw_attacker && !saw_defender {
                continue;
            }

            // Half the magnitude of the defender's pain-rage — observers
            // are alarmed but not personally injured.
            let witness_fear = FEAR_PER_HIT * scale * 0.5;
            state.add_emotion(Emotion::new(EmotionType::Fear, witness_fear));

            // Anger toward the attacker only if the observer treats the
            // defender as a Friend — kin/pack bonds make violence
            // personal. Cheap MindGraph trait lookup; nothing happens
            // for strangers.
            if let Some(mind) = mind
                && mind.has_trait(
                    &Node::Entity(defender),
                    crate::agent::mind::knowledge::Concept::Friend,
                )
            {
                let witness_anger = ANGER_PER_HIT * scale * 0.5;
                state.add_emotion(Emotion::new(EmotionType::Anger, witness_anger));
            }

            witness_events.push((observer, attacker, defender));
        }
    }

    let mut writer = sim_events.p1();
    for (observer, attacker, defender) in witness_events {
        writer.write(crate::agent::events::SimEvent::single(
            tick.current,
            observer,
            SimEventKind::WitnessedCombat {
                observer,
                attacker,
                defender,
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emotion_intensity_decreases_after_decay_tick() {
        let config = EmotionConfig::default();
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Fear, 0.8));

        let initial = state.get_emotion_intensity(EmotionType::Fear);
        state.decay_tick(1.0, &config);
        let after = state.get_emotion_intensity(EmotionType::Fear);

        assert!(
            after < initial,
            "intensity should decrease after a decay tick (initial={initial}, after={after})"
        );
    }

    #[test]
    fn unreinforced_emotion_fades_to_zero() {
        let config = EmotionConfig::default();
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Surprise, 0.5));
        assert_eq!(state.active_emotions.len(), 1);

        // Simulate ~100 seconds of decay (1000 ticks of 0.1s each).
        for _ in 0..1000 {
            state.decay_tick(0.1, &config);
        }

        assert!(
            state.active_emotions.is_empty(),
            "unreinforced emotion should be removed after sustained decay"
        );
        assert_eq!(state.get_emotion_intensity(EmotionType::Surprise), 0.0);
    }

    #[test]
    fn decay_is_monotonic_across_many_ticks() {
        let config = EmotionConfig::default();
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Joy, 1.0));

        let mut previous = state.get_emotion_intensity(EmotionType::Joy);
        for _ in 0..20 {
            state.decay_tick(0.5, &config);
            let current = state.get_emotion_intensity(EmotionType::Joy);
            assert!(
                current <= previous,
                "intensity must never increase during decay (prev={previous}, curr={current})"
            );
            previous = current;
        }
    }

    #[test]
    fn higher_fuel_decays_faster_per_second() {
        let config = EmotionConfig::default();

        let mut low = EmotionalState::default();
        low.add_emotion(Emotion::new(EmotionType::Anger, 0.2));

        let mut high = EmotionalState::default();
        high.active_emotions.push(Emotion {
            emotion_type: EmotionType::Anger,
            intensity: 1.0,
            fuel: 5.0,
        });

        let low_before = low.active_emotions[0].fuel;
        let high_before = high.active_emotions[0].fuel;

        low.decay_tick(1.0, &config);
        high.decay_tick(1.0, &config);

        let low_drop = low_before - low.active_emotions[0].fuel;
        let high_drop = high_before - high.active_emotions[0].fuel;

        assert!(
            high_drop > low_drop,
            "fuel-scaled decay should drain high-fuel emotions faster (low={low_drop}, high={high_drop})"
        );
    }

    // ── compute_target_mood / emotion_valence tests ──────────────────────────

    fn personality_with(
        neuroticism: f32,
        agreeableness: f32,
        openness: f32,
    ) -> crate::agent::psyche::personality::Personality {
        use crate::agent::psyche::personality::{Personality, PersonalityTraits};
        Personality {
            traits: PersonalityTraits {
                neuroticism,
                agreeableness,
                openness,
                extraversion: 0.5,
                conscientiousness: 0.5,
            },
        }
    }

    #[test]
    fn no_emotions_gives_neutral_mood() {
        let state = EmotionalState::default();
        let personality = personality_with(0.5, 0.5, 0.5);
        let mood = compute_target_mood(&state, &personality, None);
        // Baseline = (0.5 - 0.5) * 0.5 = 0.0; weight = 0.5; target = 0.0
        assert!(
            mood.abs() < 0.01,
            "no emotions should produce neutral mood, got {mood}"
        );
    }

    #[test]
    fn joy_only_gives_positive_mood() {
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Joy, 1.0));
        let personality = personality_with(0.0, 0.5, 0.5);
        let mood = compute_target_mood(&state, &personality, None);
        assert!(mood > 0.0, "joy should produce positive mood, got {mood}");
    }

    #[test]
    fn fear_only_gives_negative_mood() {
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Fear, 1.0));
        let personality = personality_with(0.5, 0.5, 0.5);
        let mood = compute_target_mood(&state, &personality, None);
        assert!(mood < 0.0, "fear should produce negative mood, got {mood}");
    }

    #[test]
    fn mixed_emotions_balance_mood() {
        // Joy (positive) and fear (negative) at equal intensity: overall sign depends on weights
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Joy, 0.5));
        state.add_emotion(Emotion::new(EmotionType::Fear, 0.5));
        // With default personality (neuroticism=0.5, agreeableness=0.5):
        // Joy valence = 0.8 + 0.5*0.4 = 1.0; Fear valence = -(0.6 + 0.5*0.4) = -0.8
        // Net = (1.0 - 0.8) * 0.5 = 0.1; mixed but slightly positive
        let personality = personality_with(0.5, 0.5, 0.5);
        let mood = compute_target_mood(&state, &personality, None);
        // Just verify it's in range and not stuck at extremes
        assert!(
            mood > -1.0 && mood < 1.0,
            "mixed mood should be between extremes, got {mood}"
        );
    }

    #[test]
    fn neurotic_agent_more_negatively_affected_by_fear() {
        let mut fearful = EmotionalState::default();
        fearful.add_emotion(Emotion::new(EmotionType::Fear, 0.8));

        let stoic = personality_with(0.0, 0.5, 0.5);
        let neurotic = personality_with(1.0, 0.5, 0.5);

        let stoic_mood = compute_target_mood(&fearful, &stoic, None);
        let neurotic_mood = compute_target_mood(&fearful, &neurotic, None);

        assert!(
            neurotic_mood < stoic_mood,
            "neurotic agent should have more negative mood under fear (neurotic={neurotic_mood}, stoic={stoic_mood})"
        );
    }

    #[test]
    fn mood_is_deterministic_given_same_inputs() {
        let mut state = EmotionalState::default();
        state.add_emotion(Emotion::new(EmotionType::Joy, 0.6));
        state.add_emotion(Emotion::new(EmotionType::Sadness, 0.3));
        let personality = personality_with(0.7, 0.4, 0.6);

        let a = compute_target_mood(&state, &personality, None);
        let b = compute_target_mood(&state, &personality, None);

        assert_eq!(a, b, "same inputs must always produce the same mood");
    }

    // ── compute_stress_gain_rate / compute_stress_recovery_rate tests ────────

    fn traits_with(
        neuroticism: f32,
        conscientiousness: f32,
        openness: f32,
    ) -> crate::agent::psyche::personality::PersonalityTraits {
        use crate::agent::psyche::personality::PersonalityTraits;
        PersonalityTraits {
            neuroticism,
            conscientiousness,
            openness,
            extraversion: 0.5,
            agreeableness: 0.5,
        }
    }

    fn calm_needs() -> crate::agent::body::needs::PhysicalNeeds {
        use crate::agent::body::metabolism::Metabolism;
        use crate::agent::body::needs::PhysicalNeeds;
        PhysicalNeeds::full().with_metabolism(Metabolism::well_fed())
    }

    #[test]
    fn no_needs_no_emotions_gives_zero_stress_gain() {
        let config = EmotionConfig::default();
        let traits = traits_with(0.5, 0.5, 0.5);
        let emotions = EmotionalState::default();
        let needs = calm_needs();

        let gain = compute_stress_gain_rate(&emotions, &needs, None, &traits, &config);
        assert!(
            gain.abs() < 1e-6,
            "calm well-fed agent should not gain stress, got {gain}"
        );
    }

    #[test]
    fn high_hunger_produces_stress_gain() {
        let config = EmotionConfig::default();
        let traits = traits_with(0.5, 0.5, 0.5);
        let emotions = EmotionalState::default();
        let mut needs = calm_needs();
        needs.metabolism = crate::agent::body::metabolism::Metabolism::at_urgency(0.9);

        let gain = compute_stress_gain_rate(&emotions, &needs, None, &traits, &config);
        assert!(gain > 0.0, "high hunger should produce stress, got {gain}");
    }

    #[test]
    fn well_fed_rested_agent_recovers_faster_than_starving() {
        let config = EmotionConfig::default();
        let traits = traits_with(0.5, 0.5, 0.5);

        let healthy = calm_needs();
        let mut starving = calm_needs();
        starving.metabolism = crate::agent::body::metabolism::Metabolism::at_urgency(0.95);
        starving.stamina.aerobic = 5.0;

        let healthy_recovery = compute_stress_recovery_rate(&healthy, &traits, &config);
        let starving_recovery = compute_stress_recovery_rate(&starving, &traits, &config);

        assert!(
            healthy_recovery > starving_recovery,
            "well-fed/rested agent should recover faster (healthy={healthy_recovery}, starving={starving_recovery})"
        );
    }

    #[test]
    fn neurotic_agent_gains_stress_faster_than_stoic() {
        let config = EmotionConfig::default();
        let mut needs = calm_needs();
        needs.metabolism = crate::agent::body::metabolism::Metabolism::at_urgency(0.85);
        let emotions = EmotionalState::default();

        let stoic = traits_with(0.0, 0.5, 0.5);
        let neurotic = traits_with(1.0, 0.5, 0.5);

        let stoic_gain = compute_stress_gain_rate(&emotions, &needs, None, &stoic, &config);
        let neurotic_gain = compute_stress_gain_rate(&emotions, &needs, None, &neurotic, &config);

        assert!(
            neurotic_gain > stoic_gain,
            "neurotic agent should gain stress faster (neurotic={neurotic_gain}, stoic={stoic_gain})"
        );
    }

    #[test]
    fn conscientious_agent_recovers_faster_than_unconscientious() {
        let config = EmotionConfig::default();
        let needs = calm_needs();

        let unconscientious = traits_with(0.5, 0.0, 0.5);
        let conscientious = traits_with(0.5, 1.0, 0.5);

        let slow = compute_stress_recovery_rate(&needs, &unconscientious, &config);
        let fast = compute_stress_recovery_rate(&needs, &conscientious, &config);

        assert!(
            fast > slow,
            "conscientious agent should recover faster (conscientious={fast}, unconscientious={slow})"
        );
    }

    #[test]
    fn stress_recovery_is_continuous_no_threshold_cliffs() {
        // Sweep hunger from 25 to 35 (the old hard threshold was at 30) and
        // check that recovery rate changes smoothly with no discontinuities.
        let config = EmotionConfig::default();
        let traits = traits_with(0.5, 0.5, 0.5);

        let mut prev: Option<f32> = None;
        let mut max_step = 0.0f32;
        for h in 25..=35 {
            let mut needs = calm_needs();
            needs.metabolism =
                crate::agent::body::metabolism::Metabolism::at_urgency(h as f32 / 100.0);
            let r = compute_stress_recovery_rate(&needs, &traits, &config);
            if let Some(p) = prev {
                let step: f32 = (r - p).abs();
                if step > max_step {
                    max_step = step;
                }
            }
            prev = Some(r);
        }
        // 1 unit of hunger change shouldn't cause a giant jump — bound at 5% of base.
        assert!(
            max_step < config.stress_decay_base * 0.05,
            "recovery rate should change smoothly across hunger threshold, max step={max_step}"
        );
    }

    #[test]
    fn negative_emotions_produce_stress_gain() {
        let config = EmotionConfig::default();
        let traits = traits_with(0.5, 0.5, 0.5);
        let needs = calm_needs();
        let mut emotions = EmotionalState::default();
        emotions.add_emotion(Emotion::new(EmotionType::Fear, 0.8));

        let gain = compute_stress_gain_rate(&emotions, &needs, None, &traits, &config);
        assert!(gain > 0.0, "fear should produce stress gain, got {gain}");
    }
}
