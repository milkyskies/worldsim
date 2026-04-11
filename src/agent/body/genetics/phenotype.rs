//! Phenotype component: traits derived from Genome + SpeciesProfile.
//!
//! Reads: Genome, SpeciesProfile (at spawn, via Added<Genome> trigger)
//! Writes: Phenotype (inserted), Personality (overwritten), Vision (overwritten)
//! Upstream: genetics::genome (Genome), body::species (SpeciesProfile)
//! Downstream: nervous_system::execution (speed multiplier),
//!             mind::perception (vision range), nervous_system (Personality)

use bevy::prelude::*;

use crate::agent::body::genetics::genome::{
    AGREEABLENESS_START, CONSCIENTIOUSNESS_START, ENDURANCE_START, EXTRAVERSION_START, Genome,
    METABOLISM_START, NEUROTICISM_START, OPENNESS_START, SPEED_START, VISION_START,
};
use crate::agent::body::species::SpeciesProfile;
use crate::agent::events::SimEvent;
use crate::agent::mind::perception::Vision;
use crate::agent::psyche::personality::{Personality, PersonalityTraits};
use crate::core::tick::TickCount;

/// Heritability for physical traits (speed, vision, metabolism, endurance).
const H2_PHYSICAL: f32 = 0.7;

/// Heritability for Big Five personality traits.
const H2_PERSONALITY: f32 = 0.5;

/// Traits derived from the agent's genome, used by locomotion, perception, and drive systems.
///
/// Physical fields are multipliers on the species baseline (1.0 = exactly average).
/// Personality fields are 0..1 Big Five trait scores (0.5 = neutral).
///
/// Computed once at spawn by [`develop_phenotype_system`] and never modified
/// until reproduction wires in inheritance (issue #311).
#[derive(Component, Clone, Reflect, Debug)]
#[reflect(Component)]
pub struct Phenotype {
    // Physical multipliers (1.0 = species baseline)
    pub speed: f32,
    pub vision: f32,
    pub metabolism: f32,
    pub endurance: f32,
    // Personality scores (0..1, centered at 0.5)
    pub openness: f32,
    pub conscientiousness: f32,
    pub extraversion: f32,
    pub agreeableness: f32,
    pub neuroticism: f32,
}

impl Default for Phenotype {
    /// Neutral phenotype matching the species baseline exactly.
    fn default() -> Self {
        Self {
            speed: 1.0,
            vision: 1.0,
            metabolism: 1.0,
            endurance: 1.0,
            openness: 0.5,
            conscientiousness: 0.5,
            extraversion: 0.5,
            agreeableness: 0.5,
            neuroticism: 0.5,
        }
    }
}

impl Phenotype {
    /// Derive phenotype values from a genome.
    pub fn from_genome(genome: &Genome) -> Self {
        Self {
            speed: develop_physical(genome.locus_sum(SPEED_START)),
            vision: develop_physical(genome.locus_sum(VISION_START)),
            metabolism: develop_physical(genome.locus_sum(METABOLISM_START)),
            endurance: develop_physical(genome.locus_sum(ENDURANCE_START)),
            openness: develop_personality(genome.locus_sum(OPENNESS_START)),
            conscientiousness: develop_personality(genome.locus_sum(CONSCIENTIOUSNESS_START)),
            extraversion: develop_personality(genome.locus_sum(EXTRAVERSION_START)),
            agreeableness: develop_personality(genome.locus_sum(AGREEABLENESS_START)),
            neuroticism: develop_personality(genome.locus_sum(NEUROTICISM_START)),
        }
    }
}

/// Map a raw locus sum to a physical trait multiplier centered at 1.0.
///
/// `locus_sum` is the sum of 2×LOCI_PER_TRAIT values, typically in (-4.0, 4.0)
/// for founder populations. The result is a multiplier in roughly (0.65, 1.35)
/// for one-sigma genomes, shrunk toward 1.0 by the heritability mix.
fn develop_physical(locus_sum: f32) -> f32 {
    // genetic_value: multiplier in (0.5, 1.5), centered at 1.0
    let genetic_value = 1.0 + 0.5 * (locus_sum / 2.0).tanh();
    // Blend with species mean (1.0) by heritability
    (1.0 - H2_PHYSICAL) * 1.0 + H2_PHYSICAL * genetic_value
}

/// Map a raw locus sum to a personality trait score in [0, 1] centered at 0.5.
fn develop_personality(locus_sum: f32) -> f32 {
    // genetic_value in (0.0, 1.0), centered at 0.5
    let genetic_value = 0.5 + 0.5 * (locus_sum / 2.0).tanh();
    // Blend with neutral personality (0.5) by heritability
    (1.0 - H2_PERSONALITY) * 0.5 + H2_PERSONALITY * genetic_value
}

/// Runs once per entity when a [`Genome`] is added (at spawn).
///
/// Inserts:
/// - [`Phenotype`] derived from the genome
/// - [`Vision`] with range scaled by `species.vision_range * phenotype.vision`
/// - [`Personality`] derived from the phenotype's personality fields
/// - Adjusted stamina aerobic max from `phenotype.endurance`
///
/// Emits [`SimEvent::PhenotypeDeveloped`] for observability.
pub fn develop_phenotype_system(
    mut commands: Commands,
    query: Query<(Entity, &Genome, &SpeciesProfile), Added<Genome>>,
    tick: Res<TickCount>,
    mut sim_events: MessageWriter<SimEvent>,
) {
    for (entity, genome, species) in query.iter() {
        let phenotype = Phenotype::from_genome(genome);

        let vision_range = species.vision_range * phenotype.vision;

        let personality = Personality {
            traits: PersonalityTraits {
                openness: phenotype.openness,
                conscientiousness: phenotype.conscientiousness,
                extraversion: phenotype.extraversion,
                agreeableness: phenotype.agreeableness,
                neuroticism: phenotype.neuroticism,
            },
        };

        sim_events.write(SimEvent::PhenotypeDeveloped {
            agent: entity,
            tick: tick.current,
            speed: phenotype.speed,
            vision: phenotype.vision,
            metabolism: phenotype.metabolism,
            endurance: phenotype.endurance,
        });

        commands
            .entity(entity)
            .insert(phenotype)
            .insert(Vision {
                range: vision_range,
            })
            .insert(personality);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::body::genetics::genome::Genome;

    #[test]
    fn neutral_genome_produces_baseline_phenotype() {
        let g = Genome::default();
        let p = Phenotype::from_genome(&g);
        // All physical traits should be exactly 1.0 (species baseline)
        assert!((p.speed - 1.0).abs() < 1e-6, "speed={}", p.speed);
        assert!((p.vision - 1.0).abs() < 1e-6, "vision={}", p.vision);
        assert!(
            (p.metabolism - 1.0).abs() < 1e-6,
            "metabolism={}",
            p.metabolism
        );
        assert!(
            (p.endurance - 1.0).abs() < 1e-6,
            "endurance={}",
            p.endurance
        );
        // All personality traits should be exactly 0.5 (neutral)
        assert!((p.openness - 0.5).abs() < 1e-6, "openness={}", p.openness);
        assert!((p.conscientiousness - 0.5).abs() < 1e-6);
        assert!((p.extraversion - 0.5).abs() < 1e-6);
        assert!((p.agreeableness - 0.5).abs() < 1e-6);
        assert!((p.neuroticism - 0.5).abs() < 1e-6);
    }

    #[test]
    fn positive_loci_produce_above_baseline_speed() {
        let mut g = Genome::default();
        // Set all 4 speed loci on both haplotypes to +1.0
        for i in 0..4 {
            g.maternal[i] = 1.0;
            g.paternal[i] = 1.0;
        }
        let p = Phenotype::from_genome(&g);
        assert!(
            p.speed > 1.0,
            "positive loci should produce speed > 1.0, got {}",
            p.speed
        );
    }

    #[test]
    fn negative_loci_produce_below_baseline_speed() {
        let mut g = Genome::default();
        for i in 0..4 {
            g.maternal[i] = -1.0;
            g.paternal[i] = -1.0;
        }
        let p = Phenotype::from_genome(&g);
        assert!(
            p.speed < 1.0,
            "negative loci should produce speed < 1.0, got {}",
            p.speed
        );
    }

    #[test]
    fn phenotype_speed_stays_in_reasonable_multiplier_range() {
        // Even with maximum possible locus values (+/- 10.0 per locus)
        let mut g = Genome::default();
        for i in 0..4 {
            g.maternal[i] = 10.0;
            g.paternal[i] = 10.0;
        }
        let p_max = Phenotype::from_genome(&g);
        for i in 0..4 {
            g.maternal[i] = -10.0;
            g.paternal[i] = -10.0;
        }
        let p_min = Phenotype::from_genome(&g);
        // Bounded by tanh: max is 1.0 + 0.35 = 1.35, min is 1.0 - 0.35 = 0.65
        assert!(
            p_max.speed <= 1.36,
            "speed should be bounded above: {}",
            p_max.speed
        );
        assert!(
            p_min.speed >= 0.64,
            "speed should be bounded below: {}",
            p_min.speed
        );
    }

    #[test]
    fn personality_loci_do_not_affect_physical_traits() {
        let mut g = Genome::default();
        // Set all personality loci to max
        for i in 16..36 {
            g.maternal[i] = 10.0;
            g.paternal[i] = 10.0;
        }
        let p = Phenotype::from_genome(&g);
        assert!(
            (p.speed - 1.0).abs() < 1e-6,
            "personality loci bled into speed"
        );
        assert!(
            (p.vision - 1.0).abs() < 1e-6,
            "personality loci bled into vision"
        );
    }
}
