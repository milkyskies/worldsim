//! Diploid Genome component: two haplotypes (maternal + paternal) of N_LOCI loci each.
//!
//! Reads: nothing (pure data component)
//! Writes: Genome (set at agent spawn)
//! Upstream: founder population generation (genetics::founder)
//! Downstream: genetics::phenotype (develop_phenotype_system consumes locus sums)

use bevy::prelude::*;

/// Total number of loci per haplotype.
///
/// Layout: 4 loci × 6 physical traits (0..24) + 4 loci × 5 personality traits (24..44) = 44.
pub const N_LOCI: usize = 44;

/// Number of loci per trait per haplotype (4 loci × 2 haplotypes = 8 values summed per trait).
pub const LOCI_PER_TRAIT: usize = 4;

// Physical trait locus start offsets (loci 0..24)
pub const SPEED_START: usize = 0;
pub const VISION_START: usize = 4;
/// Digestion efficiency: scales digestion rate and absorption yield.
/// High = fast digester, low = slow but fuel-efficient.
pub const DIGESTION_START: usize = 8;
/// Base metabolic rate: scales BMR drain and recovery rates.
/// High = burns fast at rest but recovers stamina faster.
pub const BMR_START: usize = 12;
/// Aerobic capacity: scales aerobic stamina pool size.
/// High = sustains effort longer before exhaustion.
pub const AEROBIC_CAPACITY_START: usize = 16;
/// Anaerobic capacity: scales anaerobic stamina pool size.
/// High = longer sprints before oxygen debt.
pub const ANAEROBIC_CAPACITY_START: usize = 20;

/// Number of physical trait loci (6 traits × 4 loci each).
pub const N_PHYSICAL_LOCI: usize = 24;

// Personality trait locus start offsets (loci 24..44)
pub const OPENNESS_START: usize = 24;
pub const CONSCIENTIOUSNESS_START: usize = 28;
pub const EXTRAVERSION_START: usize = 32;
pub const AGREEABLENESS_START: usize = 36;
pub const NEUROTICISM_START: usize = 40;

/// Diploid genome: two haplotypes of [`N_LOCI`] loci each.
///
/// Each locus is a continuous additive value centered near 0.0. The neutral
/// allele (all zeros) maps to exactly the species baseline phenotype.
/// Traits are purely additive — both chromosomes are summed across the locus
/// range for a given trait, then normalized by `develop_phenotype_system`.
#[derive(Component, Clone, Reflect, Debug)]
#[reflect(Component)]
pub struct Genome {
    pub maternal: [f32; N_LOCI],
    pub paternal: [f32; N_LOCI],
}

impl Default for Genome {
    /// Neutral genome: all-zero loci produce exactly the species baseline phenotype.
    fn default() -> Self {
        Self {
            maternal: [0.0; N_LOCI],
            paternal: [0.0; N_LOCI],
        }
    }
}

impl Genome {
    /// Sum both alleles across all loci for a given trait.
    ///
    /// `start` is the first locus index; reads `LOCI_PER_TRAIT` loci per haplotype.
    /// A neutral genome returns 0.0 (maps to species mean in phenotype development).
    pub fn locus_sum(&self, start: usize) -> f32 {
        let end = (start + LOCI_PER_TRAIT).min(N_LOCI);
        self.maternal[start..end].iter().sum::<f32>()
            + self.paternal[start..end].iter().sum::<f32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_genome_locus_sum_is_zero() {
        let g = Genome::default();
        assert_eq!(g.locus_sum(SPEED_START), 0.0);
        assert_eq!(g.locus_sum(OPENNESS_START), 0.0);
    }

    #[test]
    fn locus_sum_counts_both_haplotypes() {
        let mut g = Genome::default();
        g.maternal[SPEED_START] = 1.0;
        g.paternal[SPEED_START] = 0.5;
        // Only the first locus per haplotype is set; remaining three are 0.
        assert!((g.locus_sum(SPEED_START) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn locus_sum_does_not_read_adjacent_trait() {
        let mut g = Genome::default();
        // Set loci in the VISION trait (starts at index 4)
        g.maternal[VISION_START] = 2.0;
        // SPEED sum should not include VISION loci
        assert_eq!(g.locus_sum(SPEED_START), 0.0);
        assert!((g.locus_sum(VISION_START) - 2.0).abs() < 1e-6);
    }
}
