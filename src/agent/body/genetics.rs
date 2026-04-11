//! Genetics: diploid genome, derived phenotype, and founder population generation.
//!
//! Reads: Genome, SpeciesProfile (at spawn via develop_phenotype_system)
//! Writes: Phenotype, Personality (derived), Vision (updated range)
//! Upstream: agent spawning (world::deer, world::wolf, world::human)
//! Downstream: nervous_system::execution (speed), mind::perception (vision),
//!             nervous_system (Personality traits)

pub mod builder;
pub mod founder;
pub mod genome;
pub mod phenotype;
