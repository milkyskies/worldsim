//! Per-individual silhouette deformations driven deterministically by genes.
//!
//! Reads: Genome (at spawn, via `Markings::from_genome`)
//! Writes: Markings (component), `apply_markings` deforms a CreatureSilhouette
//! Upstream: world::{wolf,deer,human} spawn fns derive markings + apply at spawn time
//! Downstream: future tint variance, pattern overlays - extend this struct
//!
//! Spawn fn flow:
//! ```ignore
//! let markings = Markings::from_genome(&genome);
//! let silhouette = apply_markings(canonical_silhouette(), &markings);
//! commands.spawn((..., markings, silhouette));
//! ```
//!
//! Doing this at spawn time (rather than as a separate Bevy system) keeps the
//! renderer's `Added<CreatureSilhouette>` trigger simple and avoids re-render
//! flicker as markings settle.

use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::agent::body::genetics::genome::{Genome, N_LOCI};
use crate::silhouette::{CreatureSilhouette, PartRole};

/// Gene-driven per-individual silhouette deformations. Deterministic:
/// the same genome always produces the same markings.
#[derive(Component, Clone, Debug)]
pub struct Markings {
    /// Uniform size multiplier across all parts (0.85..1.15 typical).
    pub size_mult: f32,
    /// Per-feature length multipliers (0.80..1.20 typical) so a "leggy"
    /// creature can have long legs without long ears.
    pub ear_length_mult: f32,
    pub leg_length_mult: f32,
    pub tail_length_mult: f32,
    pub snout_length_mult: f32,
    /// Per-part offset jitter magnitude in pixels. Higher = more
    /// individual-looking asymmetric silhouette.
    pub asymmetry: f32,
    /// Seeded RNG state for any downstream consumer that wants
    /// deterministic per-individual variance (pattern picks, tint shifts).
    pub seed: u64,
}

impl Markings {
    pub fn from_genome(genome: &Genome) -> Self {
        let seed = hash_genome(genome);
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        Self {
            size_mult: rng.random_range(0.85..1.15),
            ear_length_mult: rng.random_range(0.80..1.20),
            leg_length_mult: rng.random_range(0.85..1.15),
            tail_length_mult: rng.random_range(0.80..1.25),
            snout_length_mult: rng.random_range(0.85..1.15),
            asymmetry: rng.random_range(0.0..0.6),
            seed,
        }
    }
}

/// Pure deterministic deformation: takes a canonical silhouette and a
/// markings spec, returns the per-individual silhouette.
pub fn apply_markings(
    mut silhouette: CreatureSilhouette,
    markings: &Markings,
) -> CreatureSilhouette {
    let mut rng = ChaCha8Rng::seed_from_u64(markings.seed);
    silhouette.shadow_size *= markings.size_mult;
    for part in &mut silhouette.parts {
        part.size *= markings.size_mult;
        part.offset *= markings.size_mult;
        match part.role {
            PartRole::Limb => part.size.y *= markings.leg_length_mult,
            PartRole::Ear => part.size.y *= markings.ear_length_mult,
            PartRole::Tail => part.size.x *= markings.tail_length_mult,
            PartRole::Snout => part.size.x *= markings.snout_length_mult,
            _ => {}
        }
        if markings.asymmetry > 0.0 {
            let dx = rng.random_range(-markings.asymmetry..markings.asymmetry);
            let dy = rng.random_range(-markings.asymmetry..markings.asymmetry);
            part.offset += Vec2::new(dx, dy);
        }
    }
    silhouette
}

/// Stable hash of a genome's haplotype bits. Two genomes with identical
/// loci produce identical seeds; any locus difference flips the seed.
fn hash_genome(genome: &Genome) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for i in 0..N_LOCI {
        genome.maternal[i].to_bits().hash(&mut hasher);
        genome.paternal[i].to_bits().hash(&mut hasher);
    }
    hasher.finish()
}
