//! Brain arbitration: selects the winning brain proposal by urgency and power levels.
//!
//! Reads: BrainProposal (from all brains), PhysicalNeeds, Consciousness, Body, EmotionalState, Personality
//! Writes: BrainPowers, BrainState (chosen action and winner)
//! Upstream: survival, emotional, and rational brain systems (proposal.rs)
//! Downstream: brain_system (consumes arbitrated BrainState), nervous_system execution

use std::collections::HashMap;

use super::proposal::{BrainPowers, BrainProposal, Intent};
use crate::agent::actions::channel::ChannelCapacities;
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

/// Deduplicate proposals by their `Intent`, keeping only the highest-scoring
/// proposal per non-`None` intent. `Intent::None` proposals (idle, ambient
/// behavior) are passed through untouched, since they don't compete for any
/// specific drive.
///
/// Order of returned proposals is not specified — the caller is expected to
/// re-sort by score afterwards.
fn deduplicate_by_intent(
    proposals: Vec<BrainProposal>,
    powers: &BrainPowers,
) -> Vec<BrainProposal> {
    let mut by_intent: HashMap<Intent, BrainProposal> = HashMap::new();
    let mut passthrough: Vec<BrainProposal> = Vec::new();

    for prop in proposals {
        if prop.intent == Intent::None {
            passthrough.push(prop);
            continue;
        }
        let score = score_proposal(&prop, powers);
        match by_intent.get(&prop.intent) {
            Some(existing) if score_proposal(existing, powers) >= score => {}
            _ => {
                by_intent.insert(prop.intent, prop);
            }
        }
    }

    let mut out: Vec<BrainProposal> = by_intent.into_values().collect();
    out.extend(passthrough);
    out
}

/// Multi-action arbitration: greedy admission of proposals into a parallel set.
///
/// 1. Deduplicate by `Intent` (see [`deduplicate_by_intent`]) so two brains
///    can't admit competing answers to the same drive.
/// 2. Sort remaining proposals by score (urgency * brain power), descending.
/// 3. For each proposal in score order, admit it if its body channels do not
///    hard-conflict with the already-admitted set, accounting for the agent's
///    body capacity (injuries / incapacitation / exhaustion).
/// 4. Soft conflicts are accepted - both contributing actions will degrade
///    proportionally during execution.
///
/// Returns the admitted proposals in score order. The first proposal in the
/// returned list is also the "winner" for legacy attribution.
pub fn arbitrate_parallel(
    proposals: &[Option<BrainProposal>],
    powers: &BrainPowers,
    capacities: &ChannelCapacities,
    registry: &crate::agent::actions::ActionRegistry,
) -> Vec<BrainProposal> {
    use crate::agent::actions::channel::ChannelLoad;

    let collected: Vec<BrainProposal> = proposals.iter().flatten().cloned().collect();
    let deduped = deduplicate_by_intent(collected, powers);

    let mut scored: Vec<(f32, BrainProposal)> = deduped
        .into_iter()
        .map(|p| (score_proposal(&p, powers), p))
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut admitted: Vec<BrainProposal> = Vec::new();
    let mut load = ChannelLoad::new();

    for (_score, proposal) in scored {
        let Some(action_def) = registry.get(proposal.action.action_type) else {
            continue;
        };

        if admitted
            .iter()
            .any(|a| a.action.action_type == proposal.action.action_type)
        {
            continue;
        }

        let requirements = action_def.body_channels();

        if load.would_hard_conflict(requirements, capacities) {
            continue;
        }

        load.add(requirements);
        admitted.push(proposal);
    }

    admitted
}

fn score_proposal(proposal: &BrainProposal, powers: &BrainPowers) -> f32 {
    proposal.urgency * proposal.brain.power(powers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::{ActionRegistry, ActionType};
    use crate::agent::brains::proposal::BrainType;
    use crate::agent::brains::thinking::ActionTemplate;
    use crate::agent::psyche::emotions::{Emotion, EmotionType};

    fn make_proposal(
        brain: BrainType,
        action_type: ActionType,
        urgency: f32,
        intent: Intent,
    ) -> BrainProposal {
        BrainProposal {
            brain,
            action: ActionTemplate {
                name: format!("{action_type:?}"),
                action_type,
                target_entity: None,
                target_position: None,
                preconditions: vec![],
                effects: vec![],
                consumes: vec![],
                base_cost: 1.0,
            },
            urgency,
            intent,
            reasoning: String::new(),
        }
    }

    /// Equal-weight powers so the score reduces to urgency directly.
    fn unit_powers() -> BrainPowers {
        BrainPowers {
            survival: 1.0,
            emotional: 1.0,
            rational: 1.0,
        }
    }

    #[test]
    fn dedup_keeps_highest_score_for_same_intent() {
        let powers = unit_powers();
        let walk = make_proposal(
            BrainType::Rational,
            ActionType::Walk,
            60.0,
            Intent::SatisfyHunger,
        );
        let explore = make_proposal(
            BrainType::Survival,
            ActionType::Explore,
            40.0,
            Intent::SatisfyHunger,
        );

        let deduped = deduplicate_by_intent(vec![walk, explore], &powers);

        assert_eq!(deduped.len(), 1, "same-intent proposals must collapse to 1");
        assert_eq!(deduped[0].action.action_type, ActionType::Walk);
    }

    #[test]
    fn dedup_preserves_proposals_with_different_intents() {
        let powers = unit_powers();
        let walk = make_proposal(
            BrainType::Rational,
            ActionType::Walk,
            60.0,
            Intent::SatisfyHunger,
        );
        let flee = make_proposal(
            BrainType::Emotional,
            ActionType::Flee,
            50.0,
            Intent::SatisfySafety,
        );

        let deduped = deduplicate_by_intent(vec![walk, flee], &powers);

        assert_eq!(deduped.len(), 2);
        let kinds: Vec<_> = deduped.iter().map(|p| p.action.action_type).collect();
        assert!(kinds.contains(&ActionType::Walk));
        assert!(kinds.contains(&ActionType::Flee));
    }

    #[test]
    fn dedup_three_brains_same_intent_keeps_highest() {
        let powers = unit_powers();
        let survival = make_proposal(
            BrainType::Survival,
            ActionType::Explore,
            42.0,
            Intent::SatisfyHunger,
        );
        let emotional = make_proposal(
            BrainType::Emotional,
            ActionType::Wander,
            30.0,
            Intent::SatisfyHunger,
        );
        let rational = make_proposal(
            BrainType::Rational,
            ActionType::Walk,
            56.0,
            Intent::SatisfyHunger,
        );

        let deduped = deduplicate_by_intent(vec![survival, emotional, rational], &powers);

        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].action.action_type, ActionType::Walk);
        assert_eq!(deduped[0].brain, BrainType::Rational);
    }

    #[test]
    fn dedup_passes_through_intent_none_proposals_independently() {
        let powers = unit_powers();
        // Two ambient/idle proposals with Intent::None should both survive —
        // they're not competing for any specific drive.
        let wander = make_proposal(BrainType::Rational, ActionType::Wander, 5.0, Intent::None);
        let idle = make_proposal(BrainType::Survival, ActionType::Idle, 3.0, Intent::None);

        let deduped = deduplicate_by_intent(vec![wander, idle], &powers);

        assert_eq!(
            deduped.len(),
            2,
            "Intent::None proposals must not deduplicate"
        );
    }

    #[test]
    fn arbitrate_parallel_resolves_walk_explore_hunger_deadlock() {
        // Original deadlock: Rational proposes Walk(SatisfyHunger),
        // Survival proposes Explore(SatisfyHunger). Both ride the Legs channel
        // and the channel system would historically admit both, leaving the
        // agent flip-flopping. With Intent dedup, only the higher-scoring
        // Walk survives before channel admission runs.
        let powers = unit_powers();
        let registry = ActionRegistry::new();
        let capacities = ChannelCapacities::full();

        let walk = make_proposal(
            BrainType::Rational,
            ActionType::Walk,
            60.0,
            Intent::SatisfyHunger,
        );
        let explore = make_proposal(
            BrainType::Survival,
            ActionType::Explore,
            42.0,
            Intent::SatisfyHunger,
        );

        let proposals = [Some(walk), Some(explore), None];
        let admitted = arbitrate_parallel(&proposals, &powers, &capacities, &registry);

        assert_eq!(
            admitted.len(),
            1,
            "intent dedup must drop competing Explore"
        );
        assert_eq!(admitted[0].action.action_type, ActionType::Walk);
    }

    #[test]
    fn arbitrate_parallel_admits_different_intents_in_parallel() {
        // Walk(SatisfySocial) and Eat(SatisfyHunger) target different drives
        // and different body channels (Legs vs Mouth) — both should survive
        // intent dedup *and* channel admission.
        let powers = unit_powers();
        let registry = ActionRegistry::new();
        let capacities = ChannelCapacities::full();

        let walk = make_proposal(
            BrainType::Rational,
            ActionType::Walk,
            60.0,
            Intent::SatisfySocial,
        );
        let eat = make_proposal(
            BrainType::Survival,
            ActionType::Eat,
            40.0,
            Intent::SatisfyHunger,
        );

        let proposals = [Some(walk), Some(eat), None];
        let admitted = arbitrate_parallel(&proposals, &powers, &capacities, &registry);

        let kinds: Vec<_> = admitted.iter().map(|p| p.action.action_type).collect();
        assert!(
            kinds.contains(&ActionType::Walk),
            "Walk should be admitted, got {kinds:?}"
        );
        assert!(
            kinds.contains(&ActionType::Eat),
            "Eat should be admitted, got {kinds:?}"
        );
    }

    #[test]
    fn test_survival_power_hunger() {
        let physical = PhysicalNeeds {
            hunger: 90.0, // Critical hunger
            ..Default::default()
        };
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
