//! Fluent genome builders — outcome-oriented, test-friendly.
//!
//! Reads: Phenotype (as the target spec), Genome (produced via from_phenotype)
//! Writes: Genome (final product)
//! Upstream: tests, scenario builders
//! Downstream: TestWorld spawn paths, AgentConfig.genome
//!
//! Tests and scenarios almost never care about raw loci — they care about
//! observable agent traits ("low conscientiousness", "fast deer"). These
//! builders expose that surface directly and hide the locus-level details.
//!
//! Three entry points, each returning a builder that implements `Into<Genome>`:
//!
//! - [`personality()`] — set personality fields, leave physical at baseline
//! - [`physical()`] — set physical fields, leave personality neutral
//! - [`genome()`] — set anything across both groups (full control)
//!
//! ```ignore
//! use worldsim::agent::body::genetics::builder::{personality, physical, genome};
//!
//! // Personality-only (most common test case)
//! let g = personality().conscientiousness(0.0).into();
//!
//! // Physical-only
//! let g = physical().speed(1.3).into();
//!
//! // Full control
//! let g = genome().speed(1.3).openness(0.9).into();
//! ```
//!
//! As the genetic model grows (pleiotropy, epistasis, emergent traits), these
//! builders stay stable — they're a deliberate outcome-oriented surface, not a
//! reflection of the underlying locus layout. New phenotype fields add new
//! setters; the existing ones keep working.

use crate::agent::body::genetics::genome::Genome;
use crate::agent::body::genetics::phenotype::Phenotype;

/// Entry point for personality-only genome construction. Physical traits stay
/// at species baseline. Use when a test only cares about Big Five values.
pub fn personality() -> PersonalityBuilder {
    PersonalityBuilder::default()
}

/// Entry point for physical-only genome construction. Personality stays neutral.
/// Use when a test only cares about speed / vision / metabolism / endurance.
pub fn physical() -> PhysicalBuilder {
    PhysicalBuilder::default()
}

/// Entry point for full-control genome construction — physical and personality
/// setters on a single builder.
pub fn genome() -> GenomeBuilder {
    GenomeBuilder::default()
}

/// Fluent builder for personality-centric genomes. Physical fields stay at
/// species baseline (all multipliers = 1.0).
#[derive(Default)]
pub struct PersonalityBuilder {
    target: Phenotype,
}

impl PersonalityBuilder {
    pub fn openness(mut self, v: f32) -> Self {
        self.target.openness = v.clamp(0.0, 1.0);
        self
    }

    pub fn conscientiousness(mut self, v: f32) -> Self {
        self.target.conscientiousness = v.clamp(0.0, 1.0);
        self
    }

    pub fn extraversion(mut self, v: f32) -> Self {
        self.target.extraversion = v.clamp(0.0, 1.0);
        self
    }

    pub fn agreeableness(mut self, v: f32) -> Self {
        self.target.agreeableness = v.clamp(0.0, 1.0);
        self
    }

    pub fn neuroticism(mut self, v: f32) -> Self {
        self.target.neuroticism = v.clamp(0.0, 1.0);
        self
    }
}

impl From<PersonalityBuilder> for Genome {
    fn from(b: PersonalityBuilder) -> Self {
        Genome::from_phenotype(&b.target)
    }
}

/// Fluent builder for physical-centric genomes. Personality stays at neutral
/// (all traits = 0.5).
#[derive(Default)]
pub struct PhysicalBuilder {
    target: Phenotype,
}

impl PhysicalBuilder {
    pub fn speed(mut self, v: f32) -> Self {
        self.target.speed = v;
        self
    }

    pub fn vision(mut self, v: f32) -> Self {
        self.target.vision = v;
        self
    }

    pub fn digestion(mut self, v: f32) -> Self {
        self.target.digestion = v;
        self
    }

    pub fn bmr(mut self, v: f32) -> Self {
        self.target.bmr = v;
        self
    }

    pub fn aerobic_capacity(mut self, v: f32) -> Self {
        self.target.aerobic_capacity = v;
        self
    }

    pub fn anaerobic_capacity(mut self, v: f32) -> Self {
        self.target.anaerobic_capacity = v;
        self
    }
}

impl From<PhysicalBuilder> for Genome {
    fn from(b: PhysicalBuilder) -> Self {
        Genome::from_phenotype(&b.target)
    }
}

/// Full-control builder — all physical and personality setters on one type.
/// Use when a test needs to mix physical and personality targets.
#[derive(Default)]
pub struct GenomeBuilder {
    target: Phenotype,
}

impl GenomeBuilder {
    pub fn speed(mut self, v: f32) -> Self {
        self.target.speed = v;
        self
    }

    pub fn vision(mut self, v: f32) -> Self {
        self.target.vision = v;
        self
    }

    pub fn digestion(mut self, v: f32) -> Self {
        self.target.digestion = v;
        self
    }

    pub fn bmr(mut self, v: f32) -> Self {
        self.target.bmr = v;
        self
    }

    pub fn aerobic_capacity(mut self, v: f32) -> Self {
        self.target.aerobic_capacity = v;
        self
    }

    pub fn anaerobic_capacity(mut self, v: f32) -> Self {
        self.target.anaerobic_capacity = v;
        self
    }

    pub fn openness(mut self, v: f32) -> Self {
        self.target.openness = v.clamp(0.0, 1.0);
        self
    }

    pub fn conscientiousness(mut self, v: f32) -> Self {
        self.target.conscientiousness = v.clamp(0.0, 1.0);
        self
    }

    pub fn extraversion(mut self, v: f32) -> Self {
        self.target.extraversion = v.clamp(0.0, 1.0);
        self
    }

    pub fn agreeableness(mut self, v: f32) -> Self {
        self.target.agreeableness = v.clamp(0.0, 1.0);
        self
    }

    pub fn neuroticism(mut self, v: f32) -> Self {
        self.target.neuroticism = v.clamp(0.0, 1.0);
        self
    }
}

impl From<GenomeBuilder> for Genome {
    fn from(b: GenomeBuilder) -> Self {
        Genome::from_phenotype(&b.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personality_builder_leaves_physical_neutral() {
        let g: Genome = personality().conscientiousness(0.2).into();
        let p = Phenotype::from_genome(&g);
        assert!((p.speed - 1.0).abs() < 1e-5);
        assert!((p.vision - 1.0).abs() < 1e-5);
        assert!(p.conscientiousness < 0.5);
    }

    #[test]
    fn physical_builder_leaves_personality_neutral() {
        let g: Genome = physical().speed(1.2).into();
        let p = Phenotype::from_genome(&g);
        assert!((p.openness - 0.5).abs() < 1e-5);
        assert!((p.conscientiousness - 0.5).abs() < 1e-5);
        assert!(p.speed > 1.0);
    }

    #[test]
    fn genome_builder_mixes_physical_and_personality() {
        let g: Genome = genome().speed(1.2).openness(0.7).into();
        let p = Phenotype::from_genome(&g);
        assert!(p.speed > 1.0);
        assert!(p.openness > 0.5);
        // Other traits stay neutral
        assert!((p.vision - 1.0).abs() < 1e-5);
        assert!((p.conscientiousness - 0.5).abs() < 1e-5);
    }

    #[test]
    fn default_personality_builder_produces_neutral_genome() {
        let g: Genome = personality().into();
        let p = Phenotype::from_genome(&g);
        assert!((p.openness - 0.5).abs() < 1e-5);
        assert!((p.speed - 1.0).abs() < 1e-5);
    }
}
