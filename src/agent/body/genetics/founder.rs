//! Founder population genome generation.
//!
//! Reads: Species (determines per-species variance)
//! Writes: Genome (returned to caller for insertion at spawn)
//! Upstream: agent spawning (world::deer, world::wolf, world::human)
//! Downstream: genetics::phenotype (develop_phenotype_system reads the genome)

use rand::Rng;

use crate::agent::body::genetics::genome::{Genome, N_LOCI};
use crate::agent::body::species::Species;

/// Generate a random founder genome for the given species.
///
/// Each locus is drawn from a triangle-distributed random variable centered at 0.0,
/// approximating a Gaussian with standard deviation `std` (physical or personality).
/// Loci are independent across traits and between maternal/paternal haplotypes.
pub fn random_genome<R: Rng>(rng: &mut R, species: Species) -> Genome {
    // (physical_std, personality_std): higher values → more phenotype diversity.
    // All loci centered at 0.0 — the neutral allele maps to the species baseline.
    let (physical_std, personality_std): (f32, f32) = match species {
        Species::Human => (0.5, 0.6),
        Species::Deer => (0.4, 0.5),
        Species::Wolf => (0.4, 0.5),
        Species::Rabbit => (0.3, 0.4),
        Species::Bird => (0.4, 0.5),
    };

    let mut maternal = [0.0_f32; N_LOCI];
    let mut paternal = [0.0_f32; N_LOCI];

    // Physical trait loci (0..16)
    for i in 0..16 {
        maternal[i] = sample_locus(rng, physical_std);
        paternal[i] = sample_locus(rng, physical_std);
    }

    // Personality trait loci (16..36)
    for i in 16..N_LOCI {
        maternal[i] = sample_locus(rng, personality_std);
        paternal[i] = sample_locus(rng, personality_std);
    }

    Genome { maternal, paternal }
}

/// Sample a single locus value centered at 0.0 with the given approximate std deviation.
///
/// Uses the average of three uniform samples to approximate a Gaussian via the
/// central limit theorem. Range: roughly (-std*3, +std*3).
fn sample_locus<R: Rng>(rng: &mut R, std: f32) -> f32 {
    // Average 3 uniform samples in [-1, 1], scale to desired std.
    // A uniform[-1,1] has std = 1/sqrt(3); averaging 3 gives std = 1/3.
    // Scale by std * 3 to restore original std.
    let u1 = rng.random_range(-1.0_f32..=1.0);
    let u2 = rng.random_range(-1.0_f32..=1.0);
    let u3 = rng.random_range(-1.0_f32..=1.0);
    ((u1 + u2 + u3) / 3.0) * std * 3.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    #[test]
    fn random_genome_is_deterministic_with_seeded_rng() {
        let mut rng1 = ChaCha8Rng::seed_from_u64(42);
        let mut rng2 = ChaCha8Rng::seed_from_u64(42);
        let g1 = random_genome(&mut rng1, Species::Deer);
        let g2 = random_genome(&mut rng2, Species::Deer);
        assert_eq!(g1.maternal, g2.maternal);
        assert_eq!(g1.paternal, g2.paternal);
    }

    #[test]
    fn different_seeds_produce_different_genomes() {
        let mut rng1 = ChaCha8Rng::seed_from_u64(1);
        let mut rng2 = ChaCha8Rng::seed_from_u64(2);
        let g1 = random_genome(&mut rng1, Species::Deer);
        let g2 = random_genome(&mut rng2, Species::Deer);
        // Extremely unlikely to match; if it does, something is wrong
        assert_ne!(g1.maternal, g2.maternal);
    }

    #[test]
    fn founder_genome_loci_are_centered_near_zero() {
        let mut rng = ChaCha8Rng::seed_from_u64(99);
        let n = 1000;
        let sum: f32 = (0..n)
            .map(|_| {
                let g = random_genome(&mut rng, Species::Deer);
                g.locus_sum(0) // speed locus sum
            })
            .sum();
        let mean = sum / n as f32;
        // Mean should be close to 0 (neutral allele)
        assert!(mean.abs() < 0.3, "mean locus sum far from 0: {mean}");
    }
}
