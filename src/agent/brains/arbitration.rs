use super::proposal::{BrainPowers, BrainProposal, BrainType};
use crate::agent::biology::body::Body;
use crate::agent::body::needs::{Consciousness, PhysicalNeeds};
use crate::agent::psyche::emotions::EmotionalState;
use crate::agent::psyche::personality::Personality;

/// Calculate the current power level of each brain
///
/// Brain power represents how much "say" each brain has in decision-making
/// based on the agent's current state and personality.
pub fn calculate_brain_powers(
    physical: &PhysicalNeeds,
    consciousness: &Consciousness,
    body: Option<&Body>,
    emotions: &EmotionalState,
    personality: &Personality,
) -> BrainPowers {
    // === SURVIVAL POWER ===
    // Kicks in HARD when critical needs arise (exponential curves)
    let hunger_factor = (physical.hunger / 100.0).clamp(0.0, 1.0).powf(2.0);
    // Pain is hard to normalize without a max, assuming 100 for now as "extreme pain"
    let pain_val = body.map(|b| b.total_pain()).unwrap_or(0.0);
    let pain_factor = (pain_val / 100.0).clamp(0.0, 1.0).powf(2.0);

    // Fatigue: Energy is 0-100 (100 = full energy), so fatigue is 1 - energy%
    let fatigue_factor = (1.0 - (physical.energy / 100.0).clamp(0.0, 1.0)).powf(3.0);

    // Fear
    let fear_factor =
        emotions.get_emotion_intensity(crate::agent::psyche::emotions::EmotionType::Fear);

    let survival_power =
        hunger_factor * 100.0 + pain_factor * 100.0 + fatigue_factor * 80.0 + fear_factor * 50.0;

    // === EMOTIONAL POWER ===
    // Base instinctual power (social, curiosity, etc.) - allows emotional brain
    // to act on drives even without active emotions
    let instinct_base = 25.0;

    // Additional power from active emotions
    let emotional_intensity: f32 = emotions.active_emotions.iter().map(|e| e.intensity).sum();

    // Neuroticism makes emotions more powerful
    let neuroticism_multiplier = 0.5 + personality.traits.neuroticism * 0.5;

    // Stress amplifies emotional responses
    let stress_factor = (emotions.stress_level / 100.0).clamp(0.0, 1.0);
    let stress_multiplier = 1.0 + stress_factor * 0.5;

    let emotional_power =
        instinct_base + (emotional_intensity * 50.0 * neuroticism_multiplier * stress_multiplier);

    // === RATIONAL POWER ===
    // Baseline from conscientiousness, reduced by stress and critical needs
    let base_rational = 30.0 + personality.traits.conscientiousness * 40.0;

    // Can't think straight when stressed
    let stress_penalty = stress_factor * 0.5;

    // Can't focus when starving or in pain
    let needs_penalty = (hunger_factor + pain_factor) * 0.3;

    // Low alertness (sleepiness) kills rational thought
    let alertness_penalty = if consciousness.alertness < 0.5 {
        (0.5 - consciousness.alertness) * 2.0 // 0.5 -> 0.0, 0.0 -> 1.0
    } else {
        0.0
    };

    let rational_power =
        base_rational * (1.0 - stress_penalty) * (1.0 - needs_penalty) * (1.0 - alertness_penalty);

    BrainPowers {
        survival: survival_power,
        emotional: emotional_power,
        rational: rational_power,
    }
}

/// Arbitrate between brain proposals to determine which wins
pub fn arbitrate(
    proposals: &[Option<BrainProposal>],
    powers: &BrainPowers,
) -> Option<(BrainType, BrainProposal)> {
    let mut best_score = 0.0;
    let mut winner = None;

    for proposal in proposals.iter().flatten() {
        let power = match proposal.brain {
            BrainType::Survival => powers.survival,
            BrainType::Emotional => powers.emotional,
            BrainType::Rational => powers.rational,
        };

        let score = proposal.urgency * power;

        if score > best_score {
            best_score = score;
            winner = Some((proposal.brain, proposal.clone()));
        }
    }

    winner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::brains::thinking::ActionTemplate;
    use crate::agent::psyche::emotions::{Emotion, EmotionType};

    #[test]
    fn test_survival_power_hunger() {
        let mut physical = PhysicalNeeds::default();
        physical.hunger = 90.0; // Critical hunger
        let consciousness = Consciousness::default();
        let emotions = EmotionalState::default();
        let personality = Personality::default();

        let powers =
            calculate_brain_powers(&physical, &consciousness, None, &emotions, &personality);

        assert!(
            powers.survival > 70.0,
            "Survival power should be high when starving"
        );
        assert!(
            powers.survival > powers.rational,
            "Survival should dominate rational when starving"
        );
    }

    #[test]
    fn test_emotional_power_neuroticism() {
        let physical = PhysicalNeeds::default();
        let consciousness = Consciousness::default();
        let mut emotions = EmotionalState::default();
        emotions.add_emotion(Emotion::new(EmotionType::Fear, 0.8));

        let mut personality = Personality::default();
        personality.traits.neuroticism = 1.0;

        let powers =
            calculate_brain_powers(&physical, &consciousness, None, &emotions, &personality);

        assert!(
            powers.emotional > 35.0,
            "Emotional power should be high for neurotic + fearful"
        );
    }
}
