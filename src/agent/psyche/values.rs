//! Schwartz 10 universal values with circumplex coherence.
//!
//! Each value is a float in `[0, 1]` denoting how strongly the agent endorses
//! that motivational goal. Values are derived from Big Five personality at
//! character creation, optionally biased by Background (T1.4, future), then
//! pulled through a circumplex-coherence pass so opposite values trade off.
//! See Schwartz (1992) "Universals in the content and structure of values".
//!
//! Reads: nothing (pure data)
//! Writes: Values (set once at spawn)
//! Upstream: psyche::personality (Big Five → values mapping)
//! Downstream: ui::character_sheet, future OCC Standards appraisal (#538)

use bevy::prelude::*;
use rand::Rng;
use rand_distr::{Distribution, Normal};

use crate::agent::psyche::personality::PersonalityTraits;

/// Standard deviation of per-value Gaussian noise applied after circumplex
/// coherence. Large enough to differentiate same-personality agents, small
/// enough to keep the trait→value signal dominant.
const VALUE_NOISE_STD: f32 = 0.04;

/// Strength of the circumplex coherence pass. With strength 1.0 and a pair
/// `(high=0.9, low=0.5)`, the low value is pulled down to `0.5 * (1 - 0.4) = 0.3`,
/// satisfying the "Power=0.9 → Universalism < 0.4" invariant from Schwartz.
const COHERENCE_STRENGTH: f32 = 1.0;

#[derive(Component, Debug, Clone, Reflect)]
pub struct Values {
    pub self_direction: f32,
    pub stimulation: f32,
    pub hedonism: f32,
    pub achievement: f32,
    pub power: f32,
    pub security: f32,
    pub conformity: f32,
    pub tradition: f32,
    pub benevolence: f32,
    pub universalism: f32,
}

impl Default for Values {
    fn default() -> Self {
        Self {
            self_direction: 0.5,
            stimulation: 0.5,
            hedonism: 0.5,
            achievement: 0.5,
            power: 0.5,
            security: 0.5,
            conformity: 0.5,
            tradition: 0.5,
            benevolence: 0.5,
            universalism: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, Reflect, PartialEq, Eq, Hash)]
pub enum Value {
    SelfDirection,
    Stimulation,
    Hedonism,
    Achievement,
    Power,
    Security,
    Conformity,
    Tradition,
    Benevolence,
    Universalism,
}

/// Display metadata per value, indexed by enum discriminant. Single source of
/// truth for `display_name` and `short_description` so adding a value is a
/// one-row change. Alignment with the enum is asserted in tests.
const VALUE_META: [(Value, &str, &str); 10] = [
    (
        Value::SelfDirection,
        "Self-Direction",
        "Independence of thought and action",
    ),
    (
        Value::Stimulation,
        "Stimulation",
        "Excitement, novelty, and challenge",
    ),
    (
        Value::Hedonism,
        "Hedonism",
        "Pleasure and sensory gratification",
    ),
    (
        Value::Achievement,
        "Achievement",
        "Personal success through competence",
    ),
    (Value::Power, "Power", "Social status, prestige, dominance"),
    (
        Value::Security,
        "Security",
        "Safety, harmony, and stability",
    ),
    (
        Value::Conformity,
        "Conformity",
        "Restraint of actions that upset others",
    ),
    (
        Value::Tradition,
        "Tradition",
        "Respect for customs and inherited ways",
    ),
    (Value::Benevolence, "Benevolence", "Welfare of close others"),
    (
        Value::Universalism,
        "Universalism",
        "Welfare of all people and nature",
    ),
];

impl Value {
    pub const ALL: [Value; 10] = [
        Self::SelfDirection,
        Self::Stimulation,
        Self::Hedonism,
        Self::Achievement,
        Self::Power,
        Self::Security,
        Self::Conformity,
        Self::Tradition,
        Self::Benevolence,
        Self::Universalism,
    ];

    pub fn display_name(&self) -> &'static str {
        VALUE_META[*self as usize].1
    }

    pub fn short_description(&self) -> &'static str {
        VALUE_META[*self as usize].2
    }

    pub fn get(&self, values: &Values) -> f32 {
        match self {
            Self::SelfDirection => values.self_direction,
            Self::Stimulation => values.stimulation,
            Self::Hedonism => values.hedonism,
            Self::Achievement => values.achievement,
            Self::Power => values.power,
            Self::Security => values.security,
            Self::Conformity => values.conformity,
            Self::Tradition => values.tradition,
            Self::Benevolence => values.benevolence,
            Self::Universalism => values.universalism,
        }
    }
}

/// Diametric circumplex pairs from Schwartz's 1992 model. Adjacent values on
/// the circle are compatible; opposite values conflict and pull each other
/// down during the coherence pass.
const CIRCUMPLEX_PAIRS: [(Value, Value); 5] = [
    (Value::Power, Value::Universalism),
    (Value::Achievement, Value::Benevolence),
    (Value::Hedonism, Value::Tradition),
    (Value::Stimulation, Value::Security),
    (Value::SelfDirection, Value::Conformity),
];

impl Values {
    /// Derive values from Big Five personality, then apply circumplex
    /// coherence and small Gaussian noise. Deterministic for a given `rng`
    /// state.
    pub fn from_personality(traits: &PersonalityTraits, rng: &mut impl Rng) -> Self {
        let mut v = Self::baseline_from_traits(traits);
        v.apply_circumplex_coherence();
        v.add_noise(rng);
        v.clamp_unit();
        v
    }

    /// Baseline value scores derived from Big Five trait correlations
    /// (Roccas et al. 2002 meta-analysis). Each value starts at 0.5 and is
    /// nudged by the trait deviations from neutral.
    pub fn baseline_from_traits(traits: &PersonalityTraits) -> Self {
        let o = traits.openness() - 0.5;
        let c = traits.conscientiousness() - 0.5;
        let e = traits.extraversion() - 0.5;
        let a = traits.agreeableness() - 0.5;
        let n = traits.neuroticism() - 0.5;

        Self {
            self_direction: 0.5 + 0.3 * o - 0.2 * n,
            stimulation: 0.5 + 0.3 * o + 0.2 * e - 0.2 * c - 0.2 * n,
            hedonism: 0.5 + 0.2 * e - 0.2 * c,
            achievement: 0.5 + 0.3 * c + 0.2 * e - 0.2 * a,
            power: 0.5 + 0.2 * e - 0.3 * a,
            security: 0.5 + 0.3 * c + 0.2 * n - 0.2 * o,
            conformity: 0.5 + 0.3 * c + 0.2 * a - 0.2 * o,
            tradition: 0.5 + 0.2 * a - 0.3 * o,
            benevolence: 0.5 + 0.4 * a,
            universalism: 0.5 + 0.3 * a + 0.2 * o,
        }
    }

    /// For each diametric pair, pull the lower value down proportional to
    /// the gap. The winner stays put; the loser is suppressed. This blocks
    /// psychologically incoherent profiles like high-Power high-Universalism.
    pub fn apply_circumplex_coherence(&mut self) {
        for (a, b) in CIRCUMPLEX_PAIRS {
            let va = a.get(self);
            let vb = b.get(self);
            if va > vb {
                let suppressed = vb * (1.0 - (va - vb) * COHERENCE_STRENGTH);
                self.set(b, suppressed);
            } else if vb > va {
                let suppressed = va * (1.0 - (vb - va) * COHERENCE_STRENGTH);
                self.set(a, suppressed);
            }
        }
    }

    fn add_noise(&mut self, rng: &mut impl Rng) {
        let normal = Normal::new(0.0, VALUE_NOISE_STD)
            .expect("VALUE_NOISE_STD is a valid finite positive constant");
        for v in Value::ALL {
            let noise: f32 = normal.sample(rng);
            self.set(v, v.get(self) + noise);
        }
    }

    fn clamp_unit(&mut self) {
        for v in Value::ALL {
            self.set(v, v.get(self).clamp(0.0, 1.0));
        }
    }

    fn set(&mut self, value: Value, score: f32) {
        match value {
            Value::SelfDirection => self.self_direction = score,
            Value::Stimulation => self.stimulation = score,
            Value::Hedonism => self.hedonism = score,
            Value::Achievement => self.achievement = score,
            Value::Power => self.power = score,
            Value::Security => self.security = score,
            Value::Conformity => self.conformity = score,
            Value::Tradition => self.tradition = score,
            Value::Benevolence => self.benevolence = score,
            Value::Universalism => self.universalism = score,
        }
    }

    /// All ten values sorted by score, descending. Used by the character
    /// sheet to surface an agent's core values; callers slice the prefix
    /// they need.
    pub fn sorted_descending(&self) -> [(Value, f32); 10] {
        let mut all = Value::ALL.map(|v| (v, v.get(self)));
        all.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::SeedableRng;

    fn neutral_traits() -> PersonalityTraits {
        PersonalityTraits::default()
    }

    fn rng() -> ChaCha8Rng {
        ChaCha8Rng::seed_from_u64(0xBEEF)
    }

    #[test]
    fn ten_values_stored_in_unit_interval() {
        let v = Values::from_personality(&neutral_traits(), &mut rng());
        for value in Value::ALL {
            let score = value.get(&v);
            assert!(
                (0.0..=1.0).contains(&score),
                "{} out of range: {score}",
                value.display_name()
            );
        }
    }

    #[test]
    fn coherence_suppresses_universalism_when_power_is_high() {
        let mut v = Values {
            power: 0.9,
            universalism: 0.5,
            ..Default::default()
        };
        v.apply_circumplex_coherence();
        assert!(
            v.universalism < 0.4,
            "Power=0.9 should suppress Universalism below 0.4, got {}",
            v.universalism
        );
        assert!(
            (v.power - 0.9).abs() < 1e-6,
            "Power should be untouched, got {}",
            v.power
        );
    }

    #[test]
    fn coherence_is_symmetric_across_pairs() {
        // High Universalism should likewise suppress Power.
        let mut v = Values {
            power: 0.5,
            universalism: 0.9,
            ..Default::default()
        };
        v.apply_circumplex_coherence();
        assert!(
            v.power < 0.4,
            "Universalism=0.9 should suppress Power below 0.4, got {}",
            v.power
        );
    }

    #[test]
    fn agreeable_personalities_score_higher_on_benevolence() {
        use crate::agent::psyche::personality::AgreeablenessFacets;
        let kind = PersonalityTraits {
            agreeableness: AgreeablenessFacets::uniform(0.95),
            ..Default::default()
        };
        let cold = PersonalityTraits {
            agreeableness: AgreeablenessFacets::uniform(0.05),
            ..Default::default()
        };
        let kv = Values::baseline_from_traits(&kind);
        let cv = Values::baseline_from_traits(&cold);
        assert!(
            kv.benevolence > cv.benevolence + 0.2,
            "agreeable should outscore cold on Benevolence: kind={}, cold={}",
            kv.benevolence,
            cv.benevolence
        );
    }

    #[test]
    fn open_personalities_score_higher_on_self_direction_and_universalism() {
        use crate::agent::psyche::personality::OpennessFacets;
        let curious = PersonalityTraits {
            openness: OpennessFacets::uniform(0.95),
            ..Default::default()
        };
        let conventional = PersonalityTraits {
            openness: OpennessFacets::uniform(0.05),
            ..Default::default()
        };
        let curious_v = Values::baseline_from_traits(&curious);
        let conv_v = Values::baseline_from_traits(&conventional);
        assert!(curious_v.self_direction > conv_v.self_direction);
        assert!(curious_v.universalism > conv_v.universalism);
        assert!(curious_v.tradition < conv_v.tradition);
    }

    #[test]
    fn same_personality_and_seed_produces_identical_values() {
        let traits = PersonalityTraits::uniform(0.6, 0.4, 0.7, 0.3, 0.5);
        let a = Values::from_personality(&traits, &mut ChaCha8Rng::seed_from_u64(123));
        let b = Values::from_personality(&traits, &mut ChaCha8Rng::seed_from_u64(123));
        for value in Value::ALL {
            assert_eq!(
                value.get(&a),
                value.get(&b),
                "{} drifted",
                value.display_name()
            );
        }
    }

    #[test]
    fn sorted_descending_orders_by_score() {
        let v = Values {
            power: 0.9,
            achievement: 0.8,
            benevolence: 0.7,
            ..Default::default()
        };
        let sorted = v.sorted_descending();
        assert_eq!(sorted[0].0, Value::Power);
        assert_eq!(sorted[1].0, Value::Achievement);
        assert_eq!(sorted[2].0, Value::Benevolence);
    }

    #[test]
    fn value_meta_table_aligns_with_enum() {
        for value in Value::ALL {
            assert_eq!(
                VALUE_META[value as usize].0, value,
                "VALUE_META misaligned at {value:?}; reorder the table to match the enum"
            );
        }
    }

    #[test]
    fn extreme_traits_still_clamp_to_unit_interval() {
        // Saturated personalities push baselines beyond [0, 1] before the
        // coherence + noise + clamp pipeline. This guards against any future
        // tweak that drops the clamp step.
        for o in [0.0, 1.0] {
            for c in [0.0, 1.0] {
                for e in [0.0, 1.0] {
                    for a in [0.0, 1.0] {
                        for n in [0.0, 1.0] {
                            let traits = PersonalityTraits::uniform(o, c, e, a, n);
                            let v = Values::from_personality(&traits, &mut rng());
                            for value in Value::ALL {
                                let s = value.get(&v);
                                assert!(
                                    (0.0..=1.0).contains(&s),
                                    "{} out of range at OCEAN=({o},{c},{e},{a},{n}): {s}",
                                    value.display_name()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
