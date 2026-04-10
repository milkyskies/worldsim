//! Named constants for all magic numbers used throughout the simulation.
//!
//! Organized by domain. Import the relevant submodule where needed:
//! `use crate::constants::movement::BASE_SPEED_PER_TICK;`

/// World spawning configuration
pub mod world {
    pub const HUMAN_SPAWN_COUNT: usize = 6;
    /// Number of humans in the second cluster spawned across the river.
    pub const SECOND_GROUP_SPAWN_COUNT: usize = 4;
    pub const APPLE_TREE_SPAWN_COUNT: usize = 24;
    pub const BERRY_BUSH_SPAWN_COUNT: usize = 32;
    pub const DEER_SPAWN_COUNT: usize = 8;
    pub const WOLF_SPAWN_COUNT: usize = 6;
    pub const STONE_NODE_SPAWN_COUNT: usize = 20;
    pub const WOOD_LOG_SPAWN_COUNT: usize = 20;
    /// Maximum attempts to find a walkable spawn position before giving up
    pub const MAX_SPAWN_ATTEMPTS: usize = 200;

    /// Tile radius around the settlement center where humans cluster. Tuned so
    /// every starting human falls inside the agent vision range and can see
    /// each other immediately on spawn.
    pub const HUMAN_CLUSTER_RADIUS_TILES: u32 = 4;

    /// Tile radius around the settlement center where the food garden of
    /// berry bushes is planted.
    pub const SETTLEMENT_FOOD_RADIUS_TILES: u32 = 6;

    /// Number of berry bushes seeded near the settlement (from the total
    /// `BERRY_BUSH_SPAWN_COUNT`). Remaining bushes scatter across the map.
    pub const SETTLEMENT_BERRY_BUSH_COUNT: usize = 6;

    /// Number of deer per herd. Total deer split across `DEER_SPAWN_COUNT /
    /// DEER_HERD_SIZE` herds.
    pub const DEER_HERD_SIZE: usize = 3;

    /// Tile radius for deer-herd cluster spawning.
    pub const DEER_HERD_RADIUS_TILES: u32 = 3;

    /// Minimum tile distance between a deer herd anchor and the human
    /// settlement center.
    pub const DEER_MIN_DISTANCE_FROM_SETTLEMENT: u32 = 12;

    /// Number of wolves per pack.
    pub const WOLF_PACK_SIZE: usize = 3;

    /// Tile radius for wolf-pack cluster spawning.
    pub const WOLF_PACK_RADIUS_TILES: u32 = 4;

    /// Minimum tile distance between a wolf pack anchor and the human settlement.
    pub const WOLF_MIN_DISTANCE_FROM_SETTLEMENT: u32 = 18;
}

/// Agent movement parameters
pub mod movement {
    /// Pixels per tick at normal energy
    pub const BASE_SPEED_PER_TICK: f32 = 0.8;
    pub const TIRED_ENERGY_THRESHOLD: f32 = 20.0;
    pub const TIRED_SPEED_MULTIPLIER: f32 = 0.5;
    pub const EXHAUSTED_ENERGY_THRESHOLD: f32 = 5.0;
    pub const EXHAUSTED_SPEED_MULTIPLIER: f32 = 0.2;
    /// Floor on movement even with fully destroyed legs (can always crawl)
    pub const MIN_INJURY_MOBILITY: f32 = 0.1;
    /// Upper range of leg-function contribution to movement (maps 0..1 → MIN..MIN+RANGE)
    pub const INJURY_MOBILITY_RANGE: f32 = 0.9;
}

/// Per-action constants (durations, costs, runtime effects)
pub mod actions {
    pub mod eat {
        pub const DURATION_TICKS: u32 = 20;
        pub const HUNGER_REDUCTION: f32 = 50.0;
        pub const ENERGY_GAIN: f32 = 10.0;
    }

    pub mod drink {
        pub const DURATION_TICKS: u32 = 15;
        pub const THIRST_REDUCTION: f32 = 50.0;
        pub const ENERGY_GAIN: f32 = 5.0;
    }

    pub mod harvest {
        pub const DURATION_TICKS: u32 = 30;
        pub const ENERGY_PER_SEC: f32 = -0.2;
        pub const HUNGER_PER_SEC: f32 = 2.0;
    }

    pub mod deposit {
        /// Ticks to transfer items into a target entity's slots.
        pub const DURATION_TICKS: u32 = 15;
        /// Energy drained per second while depositing.
        pub const ENERGY_PER_SEC: f32 = -0.1;
        /// Hunger cost per second while depositing.
        pub const HUNGER_PER_SEC: f32 = 0.5;
    }

    pub mod take {
        /// Ticks to extract items from a target entity's slots.
        pub const DURATION_TICKS: u32 = 15;
        /// Energy drained per second while taking.
        pub const ENERGY_PER_SEC: f32 = -0.1;
        /// Hunger cost per second while taking.
        pub const HUNGER_PER_SEC: f32 = 0.5;
    }

    pub mod build {
        /// Ticks to build a campfire (matches design doc: ~120 ticks).
        pub const CAMPFIRE_DURATION_TICKS: u32 = 120;
        /// Wood required to build a campfire.
        pub const CAMPFIRE_WOOD_REQUIRED: u32 = 3;
        /// Ticks to build a lean-to shelter.
        pub const LEAN_TO_DURATION_TICKS: u32 = 180;
        /// Wood required to build a lean-to.
        pub const LEAN_TO_WOOD_REQUIRED: u32 = 5;
        /// Large leaves required to build a lean-to.
        pub const LEAN_TO_LEAVES_REQUIRED: u32 = 2;
        /// Energy drained per second while building.
        pub const ENERGY_PER_SEC: f32 = -0.3;
        /// Hunger cost per second while building.
        pub const HUNGER_PER_SEC: f32 = 1.5;
    }

    pub mod attack {
        pub const DURATION_TICKS: u32 = 30;
        pub const BASE_COST: f32 = 10.0;
        pub const ENERGY_PER_SEC: f32 = -2.0;
    }

    pub mod walk {
        pub const ENERGY_PER_SEC: f32 = -0.3;
        pub const HUNGER_PER_SEC: f32 = 0.5;
        pub const ALERTNESS_PER_SEC: f32 = 10.0;

        /// Estimated energy cost per tile at normal speed (for planner estimation).
        /// Derived: (TILE_SIZE / BASE_SPEED_PER_TICK) * |ENERGY_PER_SEC| * (DEFAULT_TICKS_PER_SEC / 3600)
        /// = (16 / 0.8) * 0.3 * (60 / 3600) = 20 * 0.005 = 0.1
        pub const ENERGY_PER_TILE_NORMAL: f32 = 0.1;

        /// Estimated energy cost per tile at tired speed (below TIRED_ENERGY_THRESHOLD).
        /// Doubles compared to normal because TIRED_SPEED_MULTIPLIER = 0.5.
        pub const ENERGY_PER_TILE_TIRED: f32 = 0.2;
    }

    pub mod explore {
        pub const BASE_COST: f32 = 3.0;
        pub const ENERGY_PER_SEC: f32 = -0.25;
        pub const HUNGER_PER_SEC: f32 = 2.5;
        pub const ALERTNESS_PER_SEC: f32 = 5.0;
    }

    pub mod wander {
        pub const BASE_COST: f32 = 5.0;
        pub const ENERGY_PER_SEC: f32 = -0.2;
        pub const HUNGER_PER_SEC: f32 = 2.0;
        pub const ALERTNESS_PER_SEC: f32 = 5.0;
    }

    pub mod flee {
        pub const BASE_COST: f32 = 1.0;
        pub const ENERGY_PER_SEC: f32 = -0.5;
        pub const HUNGER_PER_SEC: f32 = 3.0;
        pub const ALERTNESS_PER_SEC: f32 = 20.0;
    }

    pub mod sleep {
        pub const BASE_COST: f32 = 0.1;
        pub const ENERGY_PER_SEC: f32 = 20.0;
        pub const HUNGER_PER_SEC: f32 = 0.2;
        pub const ALERTNESS_PER_SEC: f32 = -50.0;
    }
}

/// Brain behavior thresholds and urgency scores
pub mod brains {
    pub mod survival {
        /// Energy level at which a sleeping agent wakes up fully rested.
        pub const WAKE_ENERGY_THRESHOLD: f32 = 90.0;
        /// Energy safety margin used by the planner: if a walk would leave the agent below
        /// this level, Sleep is prepended so the survival brain doesn't interrupt the trip.
        pub const EXHAUSTION_TRIGGER: f32 = 15.0;
    }

    /// Emotional brain urgency scores and emotion intensity thresholds
    pub mod emotional {
        pub const FEAR_ENTITY_THRESHOLD: f32 = 0.3;
        pub const FEAR_ENTITY_URGENCY_MULTIPLIER: f32 = 80.0;
        pub const JOY_ENTITY_THRESHOLD: f32 = 0.3;
        pub const JOY_ENTITY_URGENCY_MULTIPLIER: f32 = 50.0;
        pub const ANGER_ENTITY_THRESHOLD: f32 = 0.5;
        pub const ANGER_ENTITY_URGENCY_MULTIPLIER: f32 = 60.0;
        pub const FEAR_GENERAL_THRESHOLD: f32 = 0.7;
        pub const FEAR_GENERAL_URGENCY_MULTIPLIER: f32 = 90.0;
        /// Social drive above which the emotional brain proposes
        /// `InitiateConversation` toward a visible person.
        pub const SOCIAL_SEEK_THRESHOLD: f32 = 0.55;
        /// Multiplier applied to social drive to score the urgency of
        /// initiating a conversation.
        pub const SOCIAL_SEEK_URGENCY_MULTIPLIER: f32 = 40.0;
    }

    /// Rational brain planning urgency scores and thresholds
    pub mod rational {
        /// Minimum alertness required for conscious goal-directed planning
        pub const MIN_ALERTNESS_FOR_PLANNING: f32 = 0.3;
        pub const PLAN_CONTINUATION_URGENCY: f32 = 30.0;
        pub const GOAL_SATISFIED_WANDER_URGENCY: f32 = 5.0;
        /// Multiplier applied to goal priority when falling back to exploration
        pub const EXPLORE_FALLBACK_PRIORITY_MULTIPLIER: f32 = 0.3;
        pub const IDLE_WANDER_URGENCY: f32 = 10.0;
    }

    /// GOAP planner search parameters
    pub mod planner {
        pub const MAX_ITERATIONS: usize = 200;
        /// Cost per unmet goal condition in the A* heuristic
        pub const HEURISTIC_MULTIPLIER: f32 = 5.0;
    }
}
