//! Unified flight-vs-fight threat appraisal.
//!
//! Reads: PhysicalNeeds, Body, Personality, EmotionalState, Cornered (component)
//! Writes: ThreatResponse (returned by [`appraise_threat`])
//! Upstream: brains::emotional (consumes appraisal output)
//! Downstream: brains::emotional proposal layer
//!
//! Replaces three ad-hoc proposal functions in the emotional brain with
//! a single decision function. Inputs that the project doesn't yet
//! produce (dependents nearby, on-home-turf, prior experience) live as
//! `Option` fields with documented defaults — when those subsystems
//! land, plumbing them in is one new caller field, no decision-logic
//! rewrite.
//!
//! The function returns `ThreatResponse`; the emotional brain converts
//! it into a `BrainProposal` with the appropriate `ActionType` and
//! target. That keeps appraisal-the-decision separate from
//! action-selection-the-mapping, so we can swap action verbs (e.g.
//! Charge, Kick, Gore) without touching the appraisal core.

use crate::agent::actions::channel::Channel;
use crate::agent::biology::body::{Body, TagChannelMapping};
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::psyche::personality::PersonalityTraits;

/// Outcome of a threat appraisal — what the agent should *do* about
/// the threat, before action selection picks a concrete verb.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThreatResponse {
    /// Run away. Carries an urgency multiplier so callers can scale
    /// flee proposal intensity.
    Flee { urgency: f32 },
    /// Hold position — neither flee nor engage. Used when fleeing is
    /// pointless (cornered + can't fight) or when the threat is at the
    /// edge of attention but not yet pressing.
    StandGround,
    /// Engage. Carries a commitment value [0, 1] so action selection
    /// can scale damage intent / reckless engagement vs measured
    /// approach.
    Fight { commitment: f32 },
}

/// All inputs the appraisal needs. Group fields by data source so the
/// caller can't miss one when wiring up a new agent type.
pub struct ThreatAppraisalContext<'a> {
    // ── Defender state ──────────────────────────────────────────────
    pub physical: &'a PhysicalNeeds,
    pub body: Option<&'a Body>,
    pub personality: Option<&'a PersonalityTraits>,
    /// Currently-felt entity-or-general Anger toward the threat.
    /// Higher anger biases toward Fight even at low desperation.
    pub anger: f32,
    /// Set when `pick_flee_target` exhausted candidates last tick —
    /// halves the Fight threshold so trapped agents engage.
    pub cornered: bool,

    // ── Attacker state ──────────────────────────────────────────────
    /// Body of the threat, if perceivable. None means "don't know how
    /// strong they are" — defaults to a moderate baseline so the
    /// agent doesn't either freeze in indecision or recklessly engage.
    pub attacker_body: Option<&'a Body>,

    // ── Future inputs (stubbed today, real once their subsystems land) ──
    /// Number of dependents nearby (kin/young/packmates). Higher values
    /// bias toward Fight — protective aggression. Stub today.
    pub dependents_nearby: u32,
    /// True if the agent is on home turf (their cabin, the pack's kill,
    /// the herd's bedding area). Biases toward Fight. Stub today.
    pub on_home_turf: bool,
    /// Prior-experience modifier in [-1.0, 1.0]. Negative = "this
    /// species killed kin" (bias to Fight or extreme Flee depending
    /// on context); positive = "I've beaten them before" (bias to
    /// Fight). Stub today.
    pub prior_experience: f32,
}

// ════════════════════════════════════════════════════════════════════════════
// THRESHOLDS
// ════════════════════════════════════════════════════════════════════════════

/// Effective combat-power ratio (defender / attacker, plus modifiers)
/// at or above which a non-cornered agent fights instead of fleeing.
/// Set above 1.0 so that neither power parity nor a neutral personality
/// alone is enough — the agent needs a real edge or another bias source
/// (anger, dependents, boldness, home turf) to engage.
const POWER_RATIO_FIGHT_DEFAULT: f32 = 1.15;
/// Dependents-nearby Fight bonus (per dependent, capped at 4).
const DEPENDENTS_FIGHT_BONUS: f32 = 0.08;
/// Home-turf Fight bonus.
const HOME_TURF_FIGHT_BONUS: f32 = 0.15;
/// Prior-experience scalar.
const PRIOR_EXPERIENCE_FIGHT_BONUS: f32 = 0.20;
/// Anger contribution to Fight bias (per unit of Anger, clamped).
const ANGER_FIGHT_BONUS: f32 = 0.50;
/// Boldness contribution centered on personality default 0.5: bold
/// agents get a positive push, timid agents get a negative one.
const BOLDNESS_FIGHT_BONUS: f32 = 0.60;
/// Hunger pushes toward Fight independently of the broader desperation
/// score — a starving predator takes risks it normally wouldn't, and
/// is the bootstrap that lets cross-species combat ever start. Prey
/// without combat capability (Body without Bite/Manipulation channels)
/// is filtered out by the `defender_power > 0.05` gate, so this
/// doesn't make hungry deer attack wolves.
const HUNGER_FIGHT_BONUS: f32 = 0.40;
/// Fight bias above which a cornered agent commits to fighting.
/// Below this they hold ground (StandGround) instead of flailing.
const CORNERED_FIGHT_BIAS_THRESHOLD: f32 = 0.30;

// ════════════════════════════════════════════════════════════════════════════
// ENTRY POINT
// ════════════════════════════════════════════════════════════════════════════

/// Decide what to do about a perceived threat. Pure function; deterministic
/// for a given `ThreatAppraisalContext`. Called once per visible Dangerous
/// entity by the emotional brain.
pub fn appraise_threat(ctx: &ThreatAppraisalContext) -> ThreatResponse {
    let defender_power = combat_power(ctx.body, ctx.physical);
    let attacker_power = ctx
        .attacker_body
        .map(|b| combat_power_body_only(b))
        .unwrap_or(0.5);

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
    let timid = p.neuroticism * 0.6 + p.agreeableness * 0.4;
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
        let bold = PersonalityTraits {
            neuroticism: 0.05,
            agreeableness: 0.05,
            ..Default::default()
        };
        let timid = PersonalityTraits {
            neuroticism: 0.95,
            agreeableness: 0.95,
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
        let timid = PersonalityTraits {
            neuroticism: 0.9,
            agreeableness: 0.9,
            ..Default::default()
        };
        let bold = PersonalityTraits {
            neuroticism: 0.1,
            agreeableness: 0.1,
            ..Default::default()
        };
        assert!(boldness_score(Some(&bold)) > boldness_score(Some(&timid)));
    }
}
