//! Deterministic simulation RNG resource.
//!
//! Reads: nothing
//! Writes: SimRng (seeded ChaCha8 generator for all logic-path randomness)
//! Upstream: TestWorld (inserts seed), CorePlugin (default seed for windowed game)
//! Downstream: execution systems that need randomness (target selection, etc.)

use bevy::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Seeded deterministic RNG resource used by all simulation logic systems.
///
/// Systems that need randomness take `ResMut<SimRng>` and call
/// `rng.inner_mut()` to obtain a `&mut ChaCha8Rng`. This makes every random
/// draw reproducible by seed, which is required for deterministic testing.
///
/// The windowed game initialises this via `CorePlugin` (default seed 0).
/// `TestWorld::with_seed_and_map` overrides it with the scenario seed before
/// any systems run.
#[derive(Resource)]
pub struct SimRng(ChaCha8Rng);

impl SimRng {
    pub fn from_seed(seed: u64) -> Self {
        Self(ChaCha8Rng::seed_from_u64(seed))
    }

    /// Mutable reference to the inner RNG for use with `rand::Rng` methods.
    pub fn inner_mut(&mut self) -> &mut ChaCha8Rng {
        &mut self.0
    }
}

impl Default for SimRng {
    fn default() -> Self {
        Self::from_seed(0)
    }
}
