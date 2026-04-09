//! Named constants for all magic numbers used throughout the simulation.
//!
//! Organized by domain. Import the relevant submodule where needed:
//! `use crate::constants::movement::BASE_SPEED_PER_TICK;`

/// World spawning configuration
pub mod world {
    pub const HUMAN_SPAWN_COUNT: usize = 6;
    pub const APPLE_TREE_SPAWN_COUNT: usize = 24;
    pub const BERRY_BUSH_SPAWN_COUNT: usize = 32;
    pub const DEER_SPAWN_COUNT: usize = 8;
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

    pub mod attack {
        pub const DURATION_TICKS: u32 = 30;
        pub const BASE_COST: f32 = 10.0;
        pub const ENERGY_PER_SEC: f32 = -2.0;
    }

    pub mod walk {
        pub const ENERGY_PER_SEC: f32 = -0.3;
        pub const HUNGER_PER_SEC: f32 = 0.5;
        pub const ALERTNESS_PER_SEC: f32 = 10.0;
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
    /// Survival brain reflexive response thresholds.
    ///
    /// Most thresholds use hysteresis: the HIGH value starts a response,
    /// the LOW value stops it (preventing rapid oscillation).
    pub mod survival {
        pub const STRESS_SNAP_HIGH: f32 = 90.0;
        pub const STRESS_SNAP_LOW: f32 = 70.0;
        pub const SNAP_HUNGER_THRESHOLD: f32 = 30.0;
        pub const SNAP_SEARCH_HUNGER_THRESHOLD: f32 = 50.0;
        pub const SNAP_EXHAUSTION_ENERGY_THRESHOLD: f32 = 50.0;
        pub const PAIN_HIGH: f32 = 70.0;
        pub const PAIN_LOW: f32 = 50.0;
        pub const HUNGER_HIGH: f32 = 80.0;
        pub const HUNGER_LOW: f32 = 60.0;
        pub const THIRST_HIGH: f32 = 80.0;
        pub const THIRST_LOW: f32 = 60.0;
        /// Energy below this triggers sleep (or keeps agent asleep)
        pub const EXHAUSTION_TRIGGER: f32 = 15.0;
        /// Energy above this allows the agent to stop sleeping from exhaustion
        pub const EXHAUSTION_RELEASE: f32 = 30.0;
        pub const FEAR_HIGH: f32 = 0.8;
        pub const FEAR_LOW: f32 = 0.5;
        /// Energy level at which a sleeping agent wakes up fully rested
        pub const WAKE_ENERGY_THRESHOLD: f32 = 90.0;
    }

    /// Emotional brain urgency scores and emotion intensity thresholds
    pub mod emotional {
        pub const CONVERSATION_RESPONSE_URGENCY: f32 = 90.0;
        pub const CONVERSATION_SOCIAL_THRESHOLD: f32 = 0.2;
        pub const CONVERSATION_CONTINUE_URGENCY: f32 = 70.0;
        pub const FEAR_ENTITY_THRESHOLD: f32 = 0.3;
        pub const FEAR_ENTITY_URGENCY_MULTIPLIER: f32 = 80.0;
        pub const JOY_ENTITY_THRESHOLD: f32 = 0.3;
        pub const JOY_ENTITY_URGENCY_MULTIPLIER: f32 = 50.0;
        pub const ANGER_ENTITY_THRESHOLD: f32 = 0.5;
        pub const ANGER_ENTITY_URGENCY_MULTIPLIER: f32 = 60.0;
        pub const FEAR_GENERAL_THRESHOLD: f32 = 0.7;
        pub const FEAR_GENERAL_URGENCY_MULTIPLIER: f32 = 90.0;
        pub const SOCIAL_SEEK_THRESHOLD: f32 = 0.3;
        pub const TALK_SOCIAL_URGENCY_MULTIPLIER: f32 = 40.0;
        pub const TALK_TRUST_URGENCY_BONUS: f32 = 10.0;
        pub const INTRODUCE_SOCIAL_URGENCY_MULTIPLIER: f32 = 35.0;
    }

    /// Rational brain planning urgency scores and thresholds
    pub mod rational {
        /// Minimum alertness required for conscious goal-directed planning
        pub const MIN_ALERTNESS_FOR_PLANNING: f32 = 0.3;
        pub const PLAN_CONTINUATION_URGENCY: f32 = 30.0;
        pub const GOAL_SATISFIED_WANDER_URGENCY: f32 = 5.0;
        /// Multiplier applied to goal priority when asking another agent for help
        pub const ASK_FOR_HELP_PRIORITY_MULTIPLIER: f32 = 0.5;
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
