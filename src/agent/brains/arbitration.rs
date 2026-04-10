//! Brain arbitration: selects the winning brain proposal by urgency and power levels.
//!
//! Reads: BrainProposal (from all brains), CentralNervousSystem, Consciousness, EmotionalState, Personality
//! Writes: BrainPowers, BrainState (chosen action and winner)
//! Upstream: survival, emotional, and rational brain systems (proposal.rs)
//! Downstream: brain_system (consumes arbitrated BrainState), nervous_system execution

use std::collections::HashMap;

use super::active_plan::{ActivePlans, PlanOwner};
use super::proposal::{BrainPowers, BrainProposal, Intent};
use crate::agent::actions::channel::ChannelCapacities;
use crate::agent::body::needs::Consciousness;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::agent::psyche::emotions::EmotionalState;
use crate::agent::psyche::personality::Personality;

/// Calculate the current power level of each brain.
///
/// Brain power represents how much "say" each brain has in decision-making.
/// Survival and rational power derive from pre-computed urgency scores in the
/// CNS rather than raw needs — personality curves and modifiers already baked in.
pub fn calculate_brain_powers(
    cns: &CentralNervousSystem,
    consciousness: &Consciousness,
    emotions: &EmotionalState,
    personality: &Personality,
) -> BrainPowers {
    // === SURVIVAL POWER + RATIONAL NEEDS PENALTY ===
    // Single pass over urgencies: accumulate survival power (how much the survival
    // brain dominates) and the raw deprivation sum (how much it impairs rational
    // thought). Weights match rough priority: physical deprivation > fear.
    let (survival_power, survival_urgency) = {
        let mut power = 0.0f32;
        let mut deprivation = 0.0f32;
        for u in &cns.urgencies {
            match u.source {
                UrgencySource::Hunger | UrgencySource::Thirst | UrgencySource::Pain => {
                    power += u.value * 100.0;
                    deprivation += u.value;
                }
                UrgencySource::Stamina => power += u.value * 80.0,
                UrgencySource::Fear => power += u.value * 50.0,
                _ => {}
            }
        }
        (power, deprivation.min(1.0))
    };

    // === EMOTIONAL POWER ===
    // Base instinctual drive (social, curiosity) keeps emotional brain active
    // even without acute emotions.
    let instinct_base = 25.0;
    let emotional_intensity: f32 = emotions.active_emotions.iter().map(|e| e.intensity).sum();
    let neuroticism_multiplier = 0.5 + personality.traits.neuroticism * 0.5;
    let stress_factor = (emotions.stress_level / 100.0).clamp(0.0, 1.0);
    let stress_multiplier = 1.0 + stress_factor * 0.5;

    let emotional_power =
        instinct_base + (emotional_intensity * 50.0 * neuroticism_multiplier * stress_multiplier);

    // === RATIONAL POWER ===
    // Baseline from conscientiousness, reduced by stress and survival urgency.
    let base_rational = 30.0 + personality.traits.conscientiousness * 40.0;
    let stress_penalty = stress_factor * 0.5;

    // High survival urgency makes it hard to think straight.
    let needs_penalty = survival_urgency * 0.3;

    let alertness_penalty = if consciousness.alertness < 0.5 {
        (0.5 - consciousness.alertness) * 2.0
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
    commitment: Option<&CommitmentContext>,
) -> Vec<BrainProposal> {
    let mut by_intent: HashMap<Intent, BrainProposal> = HashMap::new();
    let mut passthrough: Vec<BrainProposal> = Vec::new();

    for prop in proposals {
        if prop.intent == Intent::None {
            passthrough.push(prop);
            continue;
        }
        let score = score_proposal(&prop, powers, commitment);
        match by_intent.get(&prop.intent) {
            Some(existing) if score_proposal(existing, powers, commitment) >= score => {}
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
    arbitrate_parallel_with_commitment(proposals, powers, capacities, registry, None)
}

/// Arbitrate with optional commitment context for plan inertia.
pub fn arbitrate_parallel_with_commitment(
    proposals: &[Option<BrainProposal>],
    powers: &BrainPowers,
    capacities: &ChannelCapacities,
    registry: &crate::agent::actions::ActionRegistry,
    commitment: Option<&CommitmentContext>,
) -> Vec<BrainProposal> {
    use crate::agent::actions::channel::ChannelLoad;

    let collected: Vec<BrainProposal> = proposals.iter().flatten().cloned().collect();
    let deduped = deduplicate_by_intent(collected, powers, commitment);

    let mut scored: Vec<(f32, BrainProposal)> = deduped
        .into_iter()
        .map(|p| (score_proposal(&p, powers, commitment), p))
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut admitted: Vec<BrainProposal> = Vec::new();
    let mut load = ChannelLoad::new();
    let mut movement_admitted = false;

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

        let kind = action_def.kind();

        // #223: Two ActionKind::Movement actions cannot coexist (the agent has
        // exactly one transform; two simultaneous moves toward different
        // targets fight over it). Skip any further Movement proposals once
        // one has already been admitted this tick. The highest-scoring
        // Movement wins because the loop iterates in score order.
        if matches!(kind, crate::agent::actions::ActionKind::Movement) && movement_admitted {
            continue;
        }

        let requirements = action_def.body_channels();

        if load.would_hard_conflict(requirements, capacities) {
            continue;
        }

        load.add(requirements);
        if matches!(kind, crate::agent::actions::ActionKind::Movement) {
            movement_admitted = true;
        }
        admitted.push(proposal);
    }

    admitted
}

/// Context for applying plan commitment inertia to proposal scores.
pub struct CommitmentContext<'a> {
    pub active_plans: &'a ActivePlans,
    pub conscientiousness: f32,
    pub current_tick: u64,
}

fn score_proposal(
    proposal: &BrainProposal,
    powers: &BrainPowers,
    commitment: Option<&CommitmentContext>,
) -> f32 {
    let base_score = proposal.urgency * proposal.brain.power(powers);

    let commitment_bonus = commitment
        .map(|ctx| {
            ctx.active_plans.commitment_bonus(
                proposal.intent,
                proposal.action.action_type,
                &PlanOwner::Brain(proposal.brain),
                ctx.conscientiousness,
                ctx.current_tick,
            )
        })
        .unwrap_or(0.0);

    base_score * (1.0 + commitment_bonus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::{ActionRegistry, ActionType};
    use crate::agent::brains::proposal::BrainType;
    use crate::agent::brains::thinking::ActionTemplate;
    use crate::agent::nervous_system::urgency::Urgency;
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

        let deduped = deduplicate_by_intent(vec![walk, explore], &powers, None);

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

        let deduped = deduplicate_by_intent(vec![walk, flee], &powers, None);

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

        let deduped = deduplicate_by_intent(vec![survival, emotional, rational], &powers, None);

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

        let deduped = deduplicate_by_intent(vec![wander, idle], &powers, None);

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

    /// #223: At most one ActionKind::Movement proposal can be admitted per
    /// tick. Two competing Movement proposals (e.g. Walk and Wander, with
    /// different intents so they survive intent dedup) must collapse to one,
    /// because the agent has exactly one transform and can't move toward two
    /// targets at once. The higher-scoring Movement wins.
    #[test]
    fn arbitrate_parallel_admits_at_most_one_movement_per_tick() {
        let powers = unit_powers();
        let registry = ActionRegistry::new();
        let capacities = ChannelCapacities::full();

        // Walk (intent: SatisfyHunger) — higher urgency, wins.
        let walk = make_proposal(
            BrainType::Rational,
            ActionType::Walk,
            60.0,
            Intent::SatisfyHunger,
        );
        // Wander (intent: None — passes through dedup, doesn't collide on
        // intent) — second Movement, must be skipped despite no channel
        // conflict.
        let wander = make_proposal(BrainType::Rational, ActionType::Wander, 30.0, Intent::None);

        let proposals = [Some(walk), Some(wander), None];
        let admitted = arbitrate_parallel(&proposals, &powers, &capacities, &registry);

        let movement_count = admitted
            .iter()
            .filter(|p| {
                matches!(
                    registry.get(p.action.action_type).map(|d| d.kind()),
                    Some(crate::agent::actions::ActionKind::Movement)
                )
            })
            .count();
        assert_eq!(
            movement_count, 1,
            "exactly one Movement may be admitted per tick, got {admitted:?}"
        );
        assert_eq!(
            admitted[0].action.action_type,
            ActionType::Walk,
            "highest-scoring Movement should win, expected Walk"
        );
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

    fn cns_with_urgency(source: UrgencySource, value: f32) -> CentralNervousSystem {
        let mut cns = CentralNervousSystem::default();
        cns.urgencies.push(Urgency::new(source, value));
        cns
    }

    #[test]
    fn high_hunger_urgency_gives_high_survival_power() {
        // Hunger urgency 0.9 → survival power = 0.9 * 100 = 90
        let cns = cns_with_urgency(UrgencySource::Hunger, 0.9);
        let consciousness = Consciousness::default();
        let emotions = EmotionalState::default();
        let personality = Personality::default();

        let powers = calculate_brain_powers(&cns, &consciousness, &emotions, &personality);

        assert!(
            powers.survival > 70.0,
            "Survival power should be high when hunger urgency is 0.9, got {:.1}",
            powers.survival
        );
        assert!(
            powers.survival > powers.rational,
            "Survival should dominate rational when starving"
        );
    }

    #[test]
    fn no_urgency_gives_low_survival_power() {
        let cns = CentralNervousSystem::default(); // no urgencies
        let consciousness = Consciousness::default();
        let emotions = EmotionalState::default();
        let personality = Personality::default();

        let powers = calculate_brain_powers(&cns, &consciousness, &emotions, &personality);

        assert!(
            powers.survival < 1.0,
            "Survival power should be near zero with no urgencies, got {:.1}",
            powers.survival
        );
    }

    #[test]
    fn emotional_power_scales_with_neuroticism_and_fear() {
        let cns = CentralNervousSystem::default();
        let consciousness = Consciousness::default();
        let mut emotions = EmotionalState::default();
        emotions.add_emotion(Emotion::new(EmotionType::Fear, 0.8));

        let mut personality = Personality::default();
        personality.traits.neuroticism = 1.0;

        let powers = calculate_brain_powers(&cns, &consciousness, &emotions, &personality);

        assert!(
            powers.emotional > 35.0,
            "Emotional power should be high for neurotic + fearful agent, got {:.1}",
            powers.emotional
        );
    }

    // === Plan ownership / commitment inertia tests (#166) ===

    #[test]
    fn commitment_inertia_prevents_flip_flop() {
        // Rational proposes Walk(SatisfyHunger) with a slightly lower base score than
        // Survival's Explore(SatisfyHunger). Without commitment, Explore wins.
        // With an active Walk plan and high conscientiousness, Walk should win.
        use crate::agent::brains::active_plan::ActivePlans;

        let powers = BrainPowers {
            survival: 1.0,
            emotional: 1.0,
            rational: 0.9, // slightly lower — simulates a stress dip
        };

        let walk = make_proposal(
            BrainType::Rational,
            ActionType::Walk,
            50.0,
            Intent::SatisfyHunger,
        );
        let explore = make_proposal(
            BrainType::Survival,
            ActionType::Explore,
            48.0,
            Intent::SatisfyHunger,
        );

        // Without commitment: Explore wins (48 * 1.0 = 48 > 50 * 0.9 = 45)
        let deduped_no_commit =
            deduplicate_by_intent(vec![walk.clone(), explore.clone()], &powers, None);
        assert_eq!(
            deduped_no_commit[0].action.action_type,
            ActionType::Explore,
            "without commitment, Explore should win"
        );

        // With commitment: Walk has inertia bonus
        let mut active_plans = ActivePlans::default();
        active_plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );
        let ctx = CommitmentContext {
            active_plans: &active_plans,
            conscientiousness: 0.8,
            current_tick: 5,
        };

        let deduped_with_commit = deduplicate_by_intent(vec![walk, explore], &powers, Some(&ctx));
        assert_eq!(
            deduped_with_commit[0].action.action_type,
            ActionType::Walk,
            "with commitment inertia, Walk should win despite lower base score"
        );
    }

    #[test]
    fn high_conscientiousness_stays_on_plan_longer_than_low() {
        // Same scenario, different conscientiousness values.
        use crate::agent::brains::active_plan::ActivePlans;

        let powers = BrainPowers {
            survival: 1.0,
            emotional: 1.0,
            rational: 0.85,
        };

        let walk = make_proposal(
            BrainType::Rational,
            ActionType::Walk,
            50.0,
            Intent::SatisfyHunger,
        );
        let explore = make_proposal(
            BrainType::Survival,
            ActionType::Explore,
            50.0,
            Intent::SatisfyHunger,
        );

        let mut active_plans = ActivePlans::default();
        active_plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );

        // High conscientiousness: Walk should win
        let ctx_high = CommitmentContext {
            active_plans: &active_plans,
            conscientiousness: 1.0,
            current_tick: 5,
        };
        let deduped_high = deduplicate_by_intent(
            vec![walk.clone(), explore.clone()],
            &powers,
            Some(&ctx_high),
        );
        assert_eq!(
            deduped_high[0].action.action_type,
            ActionType::Walk,
            "high conscientiousness agent should stay on Walk"
        );

        // Low conscientiousness: Explore should win (bonus too small to overcome)
        let ctx_low = CommitmentContext {
            active_plans: &active_plans,
            conscientiousness: 0.1,
            current_tick: 5,
        };
        let deduped_low = deduplicate_by_intent(vec![walk, explore], &powers, Some(&ctx_low));
        assert_eq!(
            deduped_low[0].action.action_type,
            ActionType::Explore,
            "low conscientiousness agent should switch to Explore"
        );
    }

    #[test]
    fn survival_override_at_critical_urgency() {
        // Active Walk plan, but wolf appears — Survival proposes Flee at very high urgency.
        // Flee should win regardless of commitment.
        use crate::agent::brains::active_plan::ActivePlans;

        let powers = BrainPowers {
            survival: 5.0, // high survival power (wolf detected, fear spike)
            emotional: 1.0,
            rational: 1.0,
        };

        let walk = make_proposal(
            BrainType::Rational,
            ActionType::Walk,
            50.0,
            Intent::SatisfyHunger,
        );
        let flee = make_proposal(
            BrainType::Survival,
            ActionType::Flee,
            90.0,
            Intent::SatisfySafety,
        );

        let mut active_plans = ActivePlans::default();
        active_plans.activate(
            PlanOwner::Brain(BrainType::Rational),
            Intent::SatisfyHunger,
            ActionType::Walk,
            0,
        );
        let ctx = CommitmentContext {
            active_plans: &active_plans,
            conscientiousness: 1.0,
            current_tick: 5,
        };

        let registry = ActionRegistry::new();
        let capacities = ChannelCapacities::full();
        let proposals = [Some(walk), Some(flee), None];
        let admitted = arbitrate_parallel_with_commitment(
            &proposals,
            &powers,
            &capacities,
            &registry,
            Some(&ctx),
        );

        // Flee is a different intent (SatisfySafety) — it doesn't compete with Walk's
        // SatisfyHunger. Both can be admitted. But Flee has overwhelmingly higher score
        // and should be the top-ranked (winner).
        assert!(
            !admitted.is_empty(),
            "at least one action should be admitted"
        );
        assert_eq!(
            admitted[0].action.action_type,
            ActionType::Flee,
            "Survival Flee at critical urgency should be the winner, overriding Walk commitment"
        );
    }
}
