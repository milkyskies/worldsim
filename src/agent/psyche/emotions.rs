use crate::agent::actions::ActionType;
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
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
    pub stress_energy_threshold: f32,
    pub stress_hunger_weight: f32,
    pub stress_energy_weight: f32,
    pub stress_pain_weight: f32,
    pub stress_emotion_weight: f32,
    pub stress_recovery_bonus: f32,
    pub stress_decay_base: f32,
    pub introversion_penalty: f32,
    pub neuroticism_amplifier: f32,
}

impl Default for EmotionConfig {
    fn default() -> Self {
        Self {
            decay_base_rate: 0.05,
            decay_fuel_factor: 0.01,
            stress_hunger_threshold: 50.0,
            stress_energy_threshold: 50.0,
            stress_hunger_weight: 0.02,
            stress_energy_weight: 0.02,
            stress_pain_weight: 0.1,
            stress_emotion_weight: 0.15,
            stress_recovery_bonus: 2.0,
            stress_decay_base: 0.5,
            introversion_penalty: 0.3,
            neuroticism_amplifier: 0.5,
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

    pub fn get_emotion_intensity(&self, emotion_type: EmotionType) -> f32 {
        self.active_emotions
            .iter()
            .find(|e| e.emotion_type == emotion_type)
            .map(|e| e.intensity)
            .unwrap_or(0.0)
    }
}

/// Role of the observer relative to the event
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ObserverRole {
    Actor,   // I did it
    Target,  // It happened to me
    Witness, // I saw it happen
}

/// Interpret what emotions an event triggers based on personality AND associations
pub fn interpret_emotion(
    action: ActionType,
    role: ObserverRole,
    actor_entity: Option<Entity>,
    personality: &crate::agent::psyche::personality::Personality,
    mind: Option<&crate::agent::mind::knowledge::MindGraph>,
    config: &EmotionConfig,
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

    // Apply Personality Modifiers
    let traits = &personality.traits;
    let mut final_emotions = Vec::new();

    for mut emotion in emotions {
        if role == ObserverRole::Witness {
            emotion.intensity *= 0.5;
            emotion.fuel *= 0.5;
        }

        // Introvert penalty (using config)
        if action == ActionType::Wave
            && emotion.emotion_type == EmotionType::Joy
            && traits.extraversion < 0.3
        {
            emotion.intensity *= config.introversion_penalty;
        }

        // Neurotic amplification (using config)
        if matches!(
            emotion.emotion_type,
            EmotionType::Fear | EmotionType::Sadness | EmotionType::Anger
        ) {
            emotion.intensity *= 1.0 + (traits.neuroticism * config.neuroticism_amplifier);
        }

        if emotion.emotion_type == EmotionType::Fear && traits.agreeableness < 0.3 {
            final_emotions.push(Emotion {
                emotion_type: EmotionType::Anger,
                intensity: emotion.intensity * 0.5,
                fuel: emotion.intensity * 0.5,
            });
        }

        final_emotions.push(emotion);
    }

    final_emotions
}

pub fn decay_emotions(
    mut agents: Query<&mut EmotionalState, With<crate::agent::Agent>>,
    time: Res<Time>,
    config: Res<EmotionConfig>,
) {
    let dt = time.delta_secs();

    for mut emotional_state in agents.iter_mut() {
        emotional_state.active_emotions.retain_mut(|e| {
            let decay_rate = config.decay_base_rate + (e.fuel * config.decay_fuel_factor).min(0.1);
            e.fuel -= decay_rate * dt;
            e.fuel = e.fuel.max(0.0);
            e.intensity = e.fuel.min(1.0);
            e.fuel > 0.01
        });
    }
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
    time: Res<Time>,
) {
    let dt = time.delta_secs();

    for (mut emotional_state, personality, body) in agents.iter_mut() {
        let mut mood_sum = 0.0;
        let mut weight_sum = 0.0;

        let baseline = (personality.traits.extraversion - personality.traits.neuroticism) * 0.5;
        mood_sum += baseline;
        weight_sum += 0.5;

        for emotion in &emotional_state.active_emotions {
            let valence = match emotion.emotion_type {
                EmotionType::Joy => 1.0,
                EmotionType::Surprise => 0.2,
                EmotionType::Sadness => -0.8,
                EmotionType::Fear => -1.0,
                EmotionType::Anger => -0.6,
                EmotionType::Disgust => -0.7,
            };
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

        let target_mood = if weight_sum > 0.0 {
            mood_sum / weight_sum
        } else {
            baseline
        };

        emotional_state.current_mood += (target_mood - emotional_state.current_mood) * dt * 0.5;
        emotional_state.current_mood = emotional_state.current_mood.clamp(-1.0, 1.0);
    }
}

pub fn update_stress(
    mut agents: Query<
        (
            &mut EmotionalState,
            &crate::agent::body::needs::PhysicalNeeds,
            Option<&crate::agent::biology::body::Body>,
        ),
        With<crate::agent::Agent>,
    >,
    time: Res<Time>,
    config: Res<EmotionConfig>,
) {
    let dt = time.delta_secs();

    for (mut emotional_state, physical, body) in agents.iter_mut() {
        let hunger = physical.hunger;
        let hunger_stress =
            (hunger - config.stress_hunger_threshold).max(0.0) * config.stress_hunger_weight;

        let energy = physical.energy;
        let fatigue_stress =
            (config.stress_energy_threshold - energy).max(0.0) * config.stress_energy_weight;

        let mut total_pain = 0.0;
        if let Some(body) = body {
            for part in body.parts() {
                for injury in &part.injuries {
                    total_pain += injury.pain * (1.0 - injury.healed_amount);
                }
            }
        }
        let pain_stress = total_pain * config.stress_pain_weight;

        let mut negative_emotion_intensity = 0.0;
        for emotion in &emotional_state.active_emotions {
            match emotion.emotion_type {
                EmotionType::Sadness
                | EmotionType::Fear
                | EmotionType::Anger
                | EmotionType::Disgust => {
                    negative_emotion_intensity += emotion.intensity;
                }
                _ => {}
            }
        }
        let emotional_stress = negative_emotion_intensity * config.stress_emotion_weight;

        let stress_gain = (hunger_stress + fatigue_stress + pain_stress + emotional_stress) * dt;

        let recovery_bonus = if hunger < 30.0 && energy > 70.0 {
            config.stress_recovery_bonus
        } else {
            1.0
        };
        let stress_decay = config.stress_decay_base * recovery_bonus * dt;

        emotional_state.stress_level += stress_gain - stress_decay;
        emotional_state.stress_level = emotional_state.stress_level.clamp(0.0, 100.0);
    }
}

pub fn react_to_events(
    mut events: MessageReader<crate::agent::events::GameEvent>,
    mut agents: Query<
        (
            Entity,
            &mut EmotionalState,
            &crate::agent::psyche::personality::Personality,
            &crate::agent::mind::knowledge::MindGraph,
        ),
        With<crate::agent::Agent>,
    >,
    config: Res<EmotionConfig>,
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
                if let Ok((_, mut state, personality, mind)) = agents.get_mut(*actor) {
                    let emotions = interpret_emotion(
                        *action,
                        ObserverRole::Actor,
                        Some(*actor),
                        personality,
                        Some(mind),
                        &config,
                    );
                    for e in emotions {
                        state.add_emotion(e);
                    }
                }

                if let Some(target_entity) = target
                    && let Ok((_, mut state, personality, mind)) = agents.get_mut(*target_entity)
                {
                    let emotions = interpret_emotion(
                        *action,
                        ObserverRole::Target,
                        Some(*actor),
                        personality,
                        Some(mind),
                        &config,
                    );
                    for e in emotions {
                        state.add_emotion(e);
                    }
                }
            }

            // Social interactions trigger emotions based on valence
            crate::agent::events::GameEvent::SocialInteraction {
                actor,
                target,

                valence,
                ..
            } => {
                // Actor feels joy/satisfaction from positive social interaction
                if *valence > 0.0 {
                    if let Ok((_, mut state, _personality, _mind)) = agents.get_mut(*actor) {
                        state.add_emotion(Emotion::new(EmotionType::Joy, *valence * 0.3));
                    }
                    if let Ok((_, mut state, _personality, _mind)) = agents.get_mut(*target) {
                        state.add_emotion(Emotion::new(EmotionType::Joy, *valence * 0.2));
                    }
                } else if *valence < 0.0 {
                    // Negative social interaction (hostility)
                    if let Ok((_, mut state, _personality, _mind)) = agents.get_mut(*actor) {
                        state.add_emotion(Emotion::new(EmotionType::Anger, valence.abs() * 0.3));
                    }
                    if let Ok((_, mut state, _personality, _mind)) = agents.get_mut(*target) {
                        state.add_emotion(Emotion::new(EmotionType::Fear, valence.abs() * 0.2));
                    }
                }
            }

            // KnowledgeShared events don't directly trigger emotions
            crate::agent::events::GameEvent::KnowledgeShared { .. } => {
                // Learning from others could trigger gratitude, but we skip for now
            }
        }
    }
}
