//! Unique display-name generation for spawned agents.
//!
//! Reads: spawn index
//! Writes: deterministic species-appropriate display name, NameCounters resource
//! Upstream: world::human::spawn_person, world::deer::spawn_deer, world::wolf::spawn_wolf,
//!           testing::spawn::spawn_test_person and related test spawners
//! Downstream: Bevy `Name` component on the agent entity (used by logging,
//!             inspection tools, and the character sheet UI)

use bevy::prelude::Resource;

/// Pool of human given names. Chosen to be easily distinguishable in logs and
/// the UI; size comfortably exceeds typical game spawn counts.
pub const HUMAN_NAMES: &[&str] = &[
    "Alice", "Bram", "Cora", "Dax", "Elin", "Finn", "Greta", "Hob", "Iris", "Jori", "Kira", "Lyle",
    "Mira", "Nell", "Odin", "Petra", "Quinn", "Rhea", "Sable", "Tam", "Una", "Viggo", "Wren",
    "Xara", "Yanna", "Zeke", "Arden", "Brielle", "Cyrus", "Dune", "Esha", "Faro", "Gale", "Hilde",
    "Ivo", "Juno", "Keir", "Lux", "Marek", "Nox", "Orla", "Pax", "Rowan", "Sonja", "Talia",
    "Ulric", "Vela", "Wick", "Yara", "Zale",
];

/// Pool of deer display names. Leans pastoral/cervine to make deer easy to
/// pick out from wolves and humans in logs.
pub const DEER_NAMES: &[&str] = &[
    "Fern", "Moss", "Clover", "Willow", "Hazel", "Sorrel", "Bracken", "Thistle", "Juniper", "Ivy",
    "Aspen", "Birch", "Maple", "Laurel", "Nettle", "Sage", "Heather", "Yarrow", "Poppy", "Bramble",
    "Rowan", "Fawn", "Doe", "Buck", "Stag", "Meadow", "Briar", "Dusk", "Dawn", "Spruce",
];

/// Pool of wolf display names. Leans dark/fanged for quick identification.
pub const WOLF_NAMES: &[&str] = &[
    "Shadow", "Fang", "Ghost", "Vex", "Cinder", "Storm", "Onyx", "Rune", "Ash", "Talon", "Blight",
    "Hollow", "Ember", "Frost", "Grim", "Nyx", "Raven", "Sable", "Thorne", "Umbra", "Wraith",
    "Zephyr", "Dirge", "Howl", "Marrow", "Pelt", "Scar", "Vigil", "Yowl", "Ripper",
];

/// Pool of minnow display names. Tiny, glittery, school-flavoured.
pub const MINNOW_NAMES: &[&str] = &[
    "Glimmer", "Bubble", "Ripple", "Spark", "Shimmer", "Dart", "Skip", "Flick", "Drift", "Wisp",
    "Pebble", "Tide", "Pip", "Mote", "Snap", "Quirk", "Dab", "Spry", "Twirl", "Glint",
];

/// Pool of pike display names. Sharper, lurkier, predator-flavoured.
pub const PIKE_NAMES: &[&str] = &[
    "Snag", "Lurker", "Reed", "Murk", "Spear", "Bog", "Drag", "Gleam", "Strike", "Hush", "Chomp",
    "Slick", "Coil", "Rasp", "Vex", "Scour",
];

/// Returns a unique-ish display name for the given spawn index.
///
/// For index < pool.len(), returns the raw name. For larger indices, appends
/// a numeric suffix so every entity still gets a unique string — the pool is
/// a soft cap, not a hard one. Deterministic: same (pool, index) → same name.
fn pick_name(pool: &[&str], index: usize) -> String {
    let base = pool[index % pool.len()];
    let cycle = index / pool.len();
    if cycle == 0 {
        base.to_string()
    } else {
        format!("{base} {}", cycle + 1)
    }
}

pub fn human_name(index: usize) -> String {
    pick_name(HUMAN_NAMES, index)
}

pub fn deer_name(index: usize) -> String {
    pick_name(DEER_NAMES, index)
}

pub fn wolf_name(index: usize) -> String {
    pick_name(WOLF_NAMES, index)
}

pub fn minnow_name(index: usize) -> String {
    pick_name(MINNOW_NAMES, index)
}

pub fn pike_name(index: usize) -> String {
    pick_name(PIKE_NAMES, index)
}

/// Per-world monotonically increasing counters used by spawners that don't
/// already track a species-local spawn index. Inserted as a Bevy Resource so
/// every path — real game, headless runner, scenario tests — shares one
/// authoritative source of truth and cannot assign the same name twice.
#[derive(Resource, Default, Debug)]
pub struct NameCounters {
    humans: usize,
    deer: usize,
    wolves: usize,
}

impl NameCounters {
    pub fn next_human(&mut self) -> String {
        let name = human_name(self.humans);
        self.humans += 1;
        name
    }

    pub fn next_deer(&mut self) -> String {
        let name = deer_name(self.deer);
        self.deer += 1;
        name
    }

    pub fn next_wolf(&mut self) -> String {
        let name = wolf_name(self.wolves);
        self.wolves += 1;
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_names_are_unique_within_pool_size() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..HUMAN_NAMES.len() {
            assert!(seen.insert(human_name(i)), "duplicate name at index {i}");
        }
    }

    #[test]
    fn names_wrap_with_numeric_suffix_past_pool() {
        let base = HUMAN_NAMES[0];
        let first = human_name(0);
        let second_cycle = human_name(HUMAN_NAMES.len());
        assert_eq!(first, base);
        assert_eq!(second_cycle, format!("{base} 2"));
    }

    #[test]
    fn name_counters_assign_unique_sequential_names() {
        let mut counters = NameCounters::default();
        let a = counters.next_human();
        let b = counters.next_human();
        assert_ne!(a, b);
        assert_eq!(a, HUMAN_NAMES[0]);
        assert_eq!(b, HUMAN_NAMES[1]);
    }

    #[test]
    fn name_counters_track_species_independently() {
        let mut counters = NameCounters::default();
        let human = counters.next_human();
        let deer = counters.next_deer();
        let wolf = counters.next_wolf();
        assert_eq!(human, HUMAN_NAMES[0]);
        assert_eq!(deer, DEER_NAMES[0]);
        assert_eq!(wolf, WOLF_NAMES[0]);
    }
}
