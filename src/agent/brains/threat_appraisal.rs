//! Unified flight-vs-fight threat appraisal.
//!
//! Reads: PhysicalNeeds, Body, Personality, Cornered (component)
//! Writes: ThreatResponse (returned by [`appraise_threat`])
//! Upstream: brains::emotional (consumes appraisal output)
//! Downstream: brains::emotional proposal layer

use crate::agent::actions::channel::Channel;
use crate::agent::biology::body::{Body, TagChannelMapping};
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::psyche::personality::PersonalityTraits;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThreatResponse {
    Flee { urgency: f32 },
    StandGround,
    Fight { commitment: f32 },
}

pub struct ThreatAppraisalContext<'a> {
    pub physical: &'a PhysicalNeeds,
    pub body: Option<&'a Body>,
    pub personality: Option<&'a PersonalityTraits>,
    pub anger: f32,
    pub cornered: bool,
    pub attacker_body: Option<&'a Body>,
    /// Stub. Wire up when the Kin/Ward relationship layer lands.
    pub dependents_nearby: u32,
    /// Stub. Wire up when the territoriality system lands.
    pub on_home_turf: bool,
    /// Stub. Wire up when episodic recall over species can be queried.
    pub prior_experience: f32,
}

/// Effective power ratio at or above which a non-cornered agent fights.
/// Above 1.0 so that parity alone (defender == attacker) doesn't trigger
/// engagement — anger, dependents, boldness, or home turf must push it.
const POWER_RATIO_FIGHT_DEFAULT: f32 = 1.15;
const DEPENDENTS_FIGHT_BONUS: f32 = 0.08;
const HOME_TURF_FIGHT_BONUS: f32 = 0.15;
const PRIOR_EXPERIENCE_FIGHT_BONUS: f32 = 0.20;
const ANGER_FIGHT_BONUS: f32 = 0.50;
/// Centered on neutral personality default 0.5; bold pushes positive,
/// timid pushes negative.
const BOLDNESS_FIGHT_BONUS: f32 = 0.60;
/// Hunger pushes toward Fight separately from desperation. Gated by
/// `defender_power > 0.05` so hungry deer don't attack wolves.
const HUNGER_FIGHT_BONUS: f32 = 0.40;
const CORNERED_FIGHT_BIAS_THRESHOLD: f32 = 0.30;

// ════════════════════════════════════════════════════════════════════════════
// ENTRY POINT
// ════════════════════════════════════════════════════════════════════════════

/// Decide what to do about a perceived threat. Pure function; deterministic
/// for a given `ThreatAppraisalContext`. Called once per visible Dangerous
/// entity by the emotional brain.
pub fn appraise_threat(ctx: &ThreatAppraisalContext) -> ThreatResponse {
    let defender_power = combat_power(ctx.body, ctx.physical);
    let attacker_power = ctx.attacker_body.map(combat_power_body_only).unwrap_or(0.5);

    let power_ratio = if attacker_power > 0.0 {
        defender_power / attacker_power
    } else {
        // Threat with no body / no readable body → treat as moderate.
        1.0
    };

    let boldness = boldness_score(ctx.personality);
    let desperation = desperation_score(ctx.physical, ctx.body);

    // Fight bias: centered around 0 for a baseline-personality, calm,
    // unencumbered agent. Anger and dependents and home turf add;
    // timid personality subtracts. Lets the threshold do its job.
    let dependents_capped = ctx.dependents_nearby.min(4) as f32;
    let hunger = ctx.physical.metabolism.hunger_urgency();
    let mut fight_bias = ANGER_FIGHT_BONUS * ctx.anger.clamp(0.0, 1.5)
        + DEPENDENTS_FIGHT_BONUS * dependents_capped
        + PRIOR_EXPERIENCE_FIGHT_BONUS * ctx.prior_experience
        + (boldness - 0.5) * BOLDNESS_FIGHT_BONUS
        + desperation * 0.20
        + hunger * HUNGER_FIGHT_BONUS;

    if ctx.on_home_turf {
        fight_bias += HOME_TURF_FIGHT_BONUS;
    }

    // Cornered: flee is impossible by definition. Pick between Fight
    // and StandGround based on aggressive momentum, NOT power ratio —
    // a cornered animal with nothing pushing it to attack just freezes.
    if ctx.cornered {
        if fight_bias > CORNERED_FIGHT_BIAS_THRESHOLD {
            let commitment = (fight_bias + 0.5).clamp(0.3, 1.0);
            return ThreatResponse::Fight { commitment };
        }
        return ThreatResponse::StandGround;
    }

    // Not cornered: compare effective power ratio against the Fight
    // threshold. Above → engage; below → flee.
    let effective_ratio = power_ratio + fight_bias;
    if effective_ratio >= POWER_RATIO_FIGHT_DEFAULT && defender_power > 0.05 {
        let commitment = (effective_ratio / (POWER_RATIO_FIGHT_DEFAULT * 1.5)).clamp(0.3, 1.0);
        return ThreatResponse::Fight { commitment };
    }

    // Flee — urgency rises with how outmatched and how desperate.
    let outmatched_factor = (1.0 - power_ratio).clamp(0.0, 1.0);
    let urgency = (0.5 + outmatched_factor * 0.5 + desperation * 0.3).clamp(0.5, 1.5);
    ThreatResponse::Flee { urgency }
}

// ════════════════════════════════════════════════════════════════════════════
// INPUT REDUCTIONS
// ════════════════════════════════════════════════════════════════════════════

/// Defender-side combat strength. Folds Body health, available combat
/// channels, and current physiological condition into a single scalar.
/// Range: roughly [0, 1] for healthy agents, < 0.1 when crippled.
pub fn combat_power(body: Option<&Body>, physical: &PhysicalNeeds) -> f32 {
    let body_power = body.map(combat_power_body_only).unwrap_or(0.5);
    // Stamina shaves combat ability — exhausted agents don't fight well.
    let stamina_mult = 0.5 + 0.5 * physical.stamina.aerobic_fraction();
    body_power * stamina_mult
}

fn combat_power_body_only(body: &Body) -> f32 {
    let health_fraction = body.overall_health();
    // Take the better of the two melee channels — wolves use Bite, humans
    // use Manipulation.
    let mapping = TagChannelMapping::default();
    let bite = body.channel_capacity(Channel::Bite, &mapping);
    let manip = body.channel_capacity(Channel::Manipulation, &mapping);
    let weapon = bite.max(manip).clamp(0.0, 1.5);
    health_fraction * weapon
}

/// 0.0 = comfortable, 1.0 = on the edge of survival. Combines hunger,
/// fatigue, and pain. Same intuition as the existing `stress_level`
/// scalar but normalized to [0, 1] for use as a fight-bias multiplier.
pub fn desperation_score(physical: &PhysicalNeeds, body: Option<&Body>) -> f32 {
    let hunger = physical.metabolism.hunger_urgency();
    let fatigue = 1.0 - physical.stamina.aerobic_fraction();
    let pain = body.map(|b| b.total_pain().min(1.0)).unwrap_or(0.0);
    ((hunger * 0.4) + (fatigue * 0.3) + (pain * 0.3)).clamp(0.0, 1.0)
}

/// Personality boldness in [0, 1]: low neuroticism + low agreeableness =
/// bold; the inverse = timid. Default 0.5 when personality is missing.
pub fn boldness_score(personality: Option<&PersonalityTraits>) -> f32 {
    let Some(p) = personality else {
        return 0.5;
    };
    let timid = p.neuroticism() * 0.6 + p.agreeableness() * 0.4;
    (1.0 - timid).clamp(0.0, 1.0)
}

// ════════════════════════════════════════════════════════════════════════════
// TESTS
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_template<'a>(physical: &'a PhysicalNeeds) -> ThreatAppraisalContext<'a> {
        ThreatAppraisalContext {
            physical,
            body: None,
            personality: None,
            anger: 0.0,
            cornered: false,
            attacker_body: None,
            dependents_nearby: 0,
            on_home_turf: false,
            prior_experience: 0.0,
        }
    }

    #[test]
    fn calm_balanced_agent_flees_by_default() {
        let physical = PhysicalNeeds::default();
        let ctx = ctx_template(&physical);
        assert!(matches!(appraise_threat(&ctx), ThreatResponse::Flee { .. }));
    }

    #[test]
    fn cornered_angry_agent_fights() {
        let physical = PhysicalNeeds::default();
        let mut ctx = ctx_template(&physical);
        ctx.cornered = true;
        ctx.anger = 1.0;
        assert!(matches!(
            appraise_threat(&ctx),
            ThreatResponse::Fight { .. }
        ));
    }

    #[test]
    fn cornered_calm_agent_stands_ground() {
        let physical = PhysicalNeeds::default();
        let mut ctx = ctx_template(&physical);
        ctx.cornered = true;
        // Low anger, no boldness boost — flee is impossible, fight bias
        // insufficient, so stand.
        let response = appraise_threat(&ctx);
        assert!(matches!(response, ThreatResponse::StandGround));
    }

    #[test]
    fn dependents_push_toward_fight() {
        let physical = PhysicalNeeds::default();
        let mut ctx = ctx_template(&physical);
        let baseline = appraise_threat(&ctx);
        ctx.dependents_nearby = 4;
        let with_dependents = appraise_threat(&ctx);

        // With dependents, a borderline situation should tip to Fight.
        assert!(
            matches!(baseline, ThreatResponse::Flee { .. }),
            "baseline calm agent should flee"
        );
        // Not asserting Fight outcome (depends on power); just that the
        // urgency or response shifts toward engagement.
        let baseline_flee_urgency = match baseline {
            ThreatResponse::Flee { urgency } => urgency,
            _ => f32::INFINITY,
        };
        let with_dep_flee_urgency = match with_dependents {
            ThreatResponse::Flee { urgency } => urgency,
            ThreatResponse::Fight { .. } | ThreatResponse::StandGround => 0.0,
        };
        assert!(
            with_dep_flee_urgency < baseline_flee_urgency
                || matches!(with_dependents, ThreatResponse::Fight { .. }),
            "dependents should reduce flee urgency or flip to Fight"
        );
    }

    #[test]
    fn bold_personality_fights_at_lower_anger_than_timid() {
        let physical = PhysicalNeeds::default();
        use crate::agent::psyche::personality::{AgreeablenessFacets, NeuroticismFacets};
        let bold = PersonalityTraits {
            agreeableness: AgreeablenessFacets::uniform(0.05),
            neuroticism: NeuroticismFacets::uniform(0.05),
            ..Default::default()
        };
        let timid = PersonalityTraits {
            agreeableness: AgreeablenessFacets::uniform(0.95),
            neuroticism: NeuroticismFacets::uniform(0.95),
            ..Default::default()
        };

        // At moderate anger that's borderline, bold should fight where
        // timid still flees.
        let mut bold_ctx = ctx_template(&physical);
        bold_ctx.personality = Some(&bold);
        bold_ctx.anger = 0.5;
        let bold_response = appraise_threat(&bold_ctx);

        let mut timid_ctx = ctx_template(&physical);
        timid_ctx.personality = Some(&timid);
        timid_ctx.anger = 0.5;
        let timid_response = appraise_threat(&timid_ctx);

        let bold_fights = matches!(bold_response, ThreatResponse::Fight { .. });
        let timid_fights = matches!(timid_response, ThreatResponse::Fight { .. });
        assert!(
            bold_fights || !timid_fights,
            "bold should fight at least as readily as timid (bold={bold_response:?} timid={timid_response:?})"
        );
    }

    #[test]
    fn high_anger_alone_can_flip_baseline_to_fight() {
        let physical = PhysicalNeeds::default();
        let mut ctx = ctx_template(&physical);
        ctx.anger = 1.5;
        // Saturated anger should produce Fight even without other inputs.
        assert!(matches!(
            appraise_threat(&ctx),
            ThreatResponse::Fight { .. }
        ));
    }

    #[test]
    fn boldness_score_scales_inversely_with_neuroticism() {
        use crate::agent::psyche::personality::{AgreeablenessFacets, NeuroticismFacets};
        let timid = PersonalityTraits {
            agreeableness: AgreeablenessFacets::uniform(0.9),
            neuroticism: NeuroticismFacets::uniform(0.9),
            ..Default::default()
        };
        let bold = PersonalityTraits {
            agreeableness: AgreeablenessFacets::uniform(0.1),
            neuroticism: NeuroticismFacets::uniform(0.1),
            ..Default::default()
        };
        assert!(boldness_score(Some(&bold)) > boldness_score(Some(&timid)));
    }
}
