//! Aspiration modulator: long-term goals bias moment-to-moment decisions.
//!
//! Without this, an agent can hold "Become a great hunter" as data forever
//! and never act on it — they'd only hunt when current Hunger demands it.
//! This module turns stored aspirations into a present-day bias on goal
//! selection (multiplier on proposal urgency before arbitration) and skill
//! XP allocation. Target/observation/conversation surfaces will be wired
//! in follow-up PRs.
//!
//! Reads: Aspirations, Personality, ActionType, SkillKind (pure functions)
//! Writes: nothing
//! Upstream: psyche::aspirations, psyche::personality, agent::skills
//! Downstream: brains::brain_system (proposal scoring), agent::skills (XP)

use crate::agent::actions::ActionType;
use crate::agent::psyche::aspirations::{
    Aspiration, AspirationKind, AspirationTarget, Aspirations,
};
use crate::agent::skills::{SkillKind, skill_for_action};
use bevy::prelude::Entity;

/// How much aspirations bend a decision at full intensity, full match,
/// neutral personality. A 30% boost flips ties without overriding clear
/// urgency differences — keep small so emergent behaviour stays
/// needs-driven first.
const BIAS_STRENGTH: f32 = 0.3;

/// Same shape as `BIAS_STRENGTH` but tuned separately because skill XP
/// compounds over time, while ranking decisions don't.
const SKILL_BIAS_STRENGTH: f32 = 0.4;

/// Score in `[0, 1]` for "how much does this action advance this aspiration."
pub fn aspiration_match(
    aspiration: &Aspiration,
    action_type: ActionType,
    action_target: Option<Entity>,
) -> f32 {
    match aspiration.kind {
        AspirationKind::BecomeGreatAt => {
            match (&aspiration.target, skill_for_action(action_type)) {
                (Some(AspirationTarget::Skill(target_skill)), Some(action_skill))
                    if *target_skill == action_skill =>
                {
                    1.0
                }
                _ => 0.0,
            }
        }
        // BuildHome enumerates the construction actions explicitly because
        // skill_for_action only covers the Build/Construct generics — the
        // specific BuildLeanTo/BuildHouse/BuildStorageChest variants don't
        // currently grant skill XP.
        AspirationKind::BuildHome => match action_type {
            ActionType::Build
            | ActionType::Construct
            | ActionType::BuildLeanTo
            | ActionType::BuildHouse
            | ActionType::BuildStorageChest => 1.0,
            _ => 0.0,
        },
        AspirationKind::Protect => match action_type {
            ActionType::DefendSelf => 0.7,
            ActionType::Attack | ActionType::Bite => 0.4,
            _ => 0.0,
        },
        AspirationKind::Avenge => match (action_type, &aspiration.target, action_target) {
            (
                ActionType::Attack | ActionType::Bite,
                Some(AspirationTarget::Entity(target)),
                Some(actual),
            ) if *target == actual => 1.0,
            _ => 0.0,
        },
        AspirationKind::FindBelonging => match action_type {
            ActionType::Converse | ActionType::InitiateConversation | ActionType::Wave => 0.6,
            _ => 0.0,
        },
        // Kinds without a clean current action mapping fall through to no
        // bias. They'll be wired as new actions/surfaces land — Understand
        // when an Examine action exists, GainStatus when reputation
        // mechanics arrive, etc.
        AspirationKind::FindPerson
        | AspirationKind::GainStatus
        | AspirationKind::EscapeFate
        | AspirationKind::Understand
        | AspirationKind::ProveSelf => 0.0,
    }
}

/// Multiplier applied to a candidate proposal's arbitration score. Always
/// `>= 1.0` so a non-aspiration goal is never penalised.
pub fn proposal_multiplier(
    aspirations: Option<&Aspirations>,
    conscientiousness: f32,
    action_type: ActionType,
    action_target: Option<Entity>,
) -> f32 {
    aspiration_bias(aspirations, conscientiousness, BIAS_STRENGTH, |g| {
        aspiration_match(g, action_type, action_target)
    })
}

/// Multiplier applied to per-tick skill XP gain. Same shape as
/// [`proposal_multiplier`] but a separate strength constant — XP compounds,
/// ranking decisions don't.
pub fn skill_xp_multiplier(
    aspirations: Option<&Aspirations>,
    conscientiousness: f32,
    skill: SkillKind,
) -> f32 {
    aspiration_bias(aspirations, conscientiousness, SKILL_BIAS_STRENGTH, |g| {
        skill_match(g, skill)
    })
}

fn aspiration_bias(
    aspirations: Option<&Aspirations>,
    conscientiousness: f32,
    strength: f32,
    score: impl Fn(&Aspiration) -> f32,
) -> f32 {
    let Some(asp) = aspirations else {
        return 1.0;
    };
    if asp.goals.is_empty() {
        return 1.0;
    }
    let pull = personality_pull(conscientiousness);
    let best = asp
        .goals
        .iter()
        .filter(|g| !g.is_fulfilled())
        .map(|g| g.intensity * score(g))
        .fold(0.0_f32, f32::max);
    1.0 + strength * pull * best
}

fn skill_match(aspiration: &Aspiration, skill: SkillKind) -> f32 {
    match (&aspiration.kind, &aspiration.target) {
        (AspirationKind::BecomeGreatAt, Some(AspirationTarget::Skill(target)))
            if *target == skill =>
        {
            1.0
        }
        (AspirationKind::Protect, _) if skill == SkillKind::Combat => 0.5,
        (AspirationKind::BuildHome, _) if skill == SkillKind::Building => 0.7,
        (AspirationKind::FindBelonging, _) if skill == SkillKind::Socializing => 0.5,
        _ => 0.0,
    }
}

/// Conscientiousness scales how readily an aspiration translates into
/// action. High-C agents pursue aspirations diligently; low-C agents hold
/// the same aspiration but rarely act. Returns 0.5 at C=0, 1.5 at C=1.
fn personality_pull(conscientiousness: f32) -> f32 {
    0.5 + conscientiousness
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::psyche::aspirations::{
        Aspiration, AspirationKind, AspirationTarget, Aspirations,
    };
    use smallvec::smallvec;

    const HIGH_C: f32 = 1.0;
    const LOW_C: f32 = 0.0;

    fn aspirations_with(goals: Vec<Aspiration>) -> Aspirations {
        Aspirations {
            goals: goals.into_iter().collect(),
        }
    }

    #[test]
    fn no_aspirations_means_neutral_multiplier() {
        let m = proposal_multiplier(None, HIGH_C, ActionType::Attack, None);
        assert_eq!(m, 1.0);
    }

    #[test]
    fn empty_aspirations_means_neutral_multiplier() {
        let asp = Aspirations { goals: smallvec![] };
        let m = proposal_multiplier(Some(&asp), HIGH_C, ActionType::Attack, None);
        assert_eq!(m, 1.0);
    }

    #[test]
    fn matching_become_great_at_skill_boosts_proposal() {
        let asp = aspirations_with(vec![Aspiration {
            kind: AspirationKind::BecomeGreatAt,
            target: Some(AspirationTarget::Skill(SkillKind::Combat)),
            progress: 0.0,
            formed_at: 0,
            intensity: 1.0,
        }]);
        let m = proposal_multiplier(Some(&asp), HIGH_C, ActionType::Attack, None);
        assert!(m > 1.0, "matching aspiration should boost, got {m}");
    }

    #[test]
    fn non_matching_action_yields_neutral_multiplier() {
        let asp = aspirations_with(vec![Aspiration {
            kind: AspirationKind::BecomeGreatAt,
            target: Some(AspirationTarget::Skill(SkillKind::Combat)),
            progress: 0.0,
            formed_at: 0,
            intensity: 1.0,
        }]);
        let m = proposal_multiplier(Some(&asp), HIGH_C, ActionType::Harvest, None);
        assert_eq!(m, 1.0, "harvest doesn't match Combat aspiration; got {m}");
    }

    #[test]
    fn higher_intensity_yields_larger_boost() {
        let weak = aspirations_with(vec![Aspiration {
            kind: AspirationKind::BecomeGreatAt,
            target: Some(AspirationTarget::Skill(SkillKind::Combat)),
            progress: 0.0,
            formed_at: 0,
            intensity: 0.2,
        }]);
        let strong = aspirations_with(vec![Aspiration {
            kind: AspirationKind::BecomeGreatAt,
            target: Some(AspirationTarget::Skill(SkillKind::Combat)),
            progress: 0.0,
            formed_at: 0,
            intensity: 1.0,
        }]);
        let weak_m = proposal_multiplier(Some(&weak), HIGH_C, ActionType::Attack, None);
        let strong_m = proposal_multiplier(Some(&strong), HIGH_C, ActionType::Attack, None);
        assert!(strong_m > weak_m, "strong={strong_m} weak={weak_m}");
    }

    #[test]
    fn low_intensity_aspiration_does_not_detectably_bias() {
        let dormant = aspirations_with(vec![Aspiration {
            kind: AspirationKind::BecomeGreatAt,
            target: Some(AspirationTarget::Skill(SkillKind::Combat)),
            progress: 0.0,
            formed_at: 0,
            intensity: 0.05,
        }]);
        let m = proposal_multiplier(Some(&dormant), HIGH_C, ActionType::Attack, None);
        // 1 + 0.3 * 1.5 * (0.05 * 1.0) = 1.0225 — under 3% bump.
        assert!(m < 1.05, "dormant aspiration should barely bias, got {m}");
    }

    #[test]
    fn fulfilled_aspirations_no_longer_bias() {
        let fulfilled = aspirations_with(vec![Aspiration {
            kind: AspirationKind::BecomeGreatAt,
            target: Some(AspirationTarget::Skill(SkillKind::Combat)),
            progress: 1.0,
            formed_at: 0,
            intensity: 1.0,
        }]);
        let m = proposal_multiplier(Some(&fulfilled), HIGH_C, ActionType::Attack, None);
        assert_eq!(m, 1.0, "fulfilled aspirations shouldn't keep biasing");
    }

    #[test]
    fn high_conscientiousness_pulls_harder_than_low() {
        let asp = aspirations_with(vec![Aspiration {
            kind: AspirationKind::BecomeGreatAt,
            target: Some(AspirationTarget::Skill(SkillKind::Combat)),
            progress: 0.0,
            formed_at: 0,
            intensity: 1.0,
        }]);
        let low = proposal_multiplier(Some(&asp), LOW_C, ActionType::Attack, None);
        let high = proposal_multiplier(Some(&asp), HIGH_C, ActionType::Attack, None);
        assert!(
            high > low,
            "high-C should pull harder: low={low} high={high}"
        );
    }

    #[test]
    fn avenge_only_fires_on_correct_target() {
        let target = Entity::from_bits(42);
        let other = Entity::from_bits(43);
        let asp = aspirations_with(vec![Aspiration {
            kind: AspirationKind::Avenge,
            target: Some(AspirationTarget::Entity(target)),
            progress: 0.0,
            formed_at: 0,
            intensity: 1.0,
        }]);
        let on_target = proposal_multiplier(Some(&asp), HIGH_C, ActionType::Attack, Some(target));
        let on_other = proposal_multiplier(Some(&asp), HIGH_C, ActionType::Attack, Some(other));
        assert!(on_target > 1.0);
        assert_eq!(on_other, 1.0);
    }

    #[test]
    fn build_home_matches_any_construction_action() {
        let asp = aspirations_with(vec![Aspiration {
            kind: AspirationKind::BuildHome,
            target: None,
            progress: 0.0,
            formed_at: 0,
            intensity: 1.0,
        }]);
        for action in [
            ActionType::Build,
            ActionType::Construct,
            ActionType::BuildLeanTo,
            ActionType::BuildHouse,
            ActionType::BuildStorageChest,
        ] {
            let m = proposal_multiplier(Some(&asp), HIGH_C, action, None);
            assert!(m > 1.0, "{action:?} should match BuildHome, got {m}");
        }
    }

    #[test]
    fn skill_xp_multiplier_boosts_aspired_skill() {
        let asp = aspirations_with(vec![Aspiration {
            kind: AspirationKind::BecomeGreatAt,
            target: Some(AspirationTarget::Skill(SkillKind::Combat)),
            progress: 0.0,
            formed_at: 0,
            intensity: 1.0,
        }]);
        let combat = skill_xp_multiplier(Some(&asp), HIGH_C, SkillKind::Combat);
        let harvest = skill_xp_multiplier(Some(&asp), HIGH_C, SkillKind::Harvesting);
        assert!(combat > harvest);
        assert_eq!(harvest, 1.0);
    }

    #[test]
    fn aspiring_hunter_outscores_harvester_at_equal_urgency() {
        // Headline scenario reduced to a unit assertion: two equal-urgency
        // candidate goals, the agent's aspiration tilts the score toward
        // the matching action.
        let asp = aspirations_with(vec![Aspiration {
            kind: AspirationKind::BecomeGreatAt,
            target: Some(AspirationTarget::Skill(SkillKind::Combat)),
            progress: 0.0,
            formed_at: 0,
            intensity: 0.9,
        }]);
        let attack_score = 50.0 * proposal_multiplier(Some(&asp), HIGH_C, ActionType::Attack, None);
        let harvest_score =
            50.0 * proposal_multiplier(Some(&asp), HIGH_C, ActionType::Harvest, None);
        assert!(
            attack_score > harvest_score,
            "aspiring hunter should outscore harvester at equal base urgency: \
             attack={attack_score} harvest={harvest_score}"
        );
    }
}
