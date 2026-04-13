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
    /// Pixels per tick at normal stamina
    pub const BASE_SPEED_PER_TICK: f32 = 0.8;
    pub const TIRED_STAMINA_THRESHOLD: f32 = 20.0;
    pub const TIRED_SPEED_MULTIPLIER: f32 = 0.5;
    pub const EXHAUSTED_STAMINA_THRESHOLD: f32 = 5.0;
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
        pub const STAMINA_GAIN: f32 = 10.0;
    }

    pub mod drink {
        pub const DURATION_TICKS: u32 = 15;
        pub const THIRST_REDUCTION: f32 = 50.0;
        pub const STAMINA_GAIN: f32 = 5.0;
    }

    pub mod harvest {
        pub const DURATION_TICKS: u32 = 30;
    }

    pub mod deposit {
        pub const DURATION_TICKS: u32 = 15;
    }

    pub mod take {
        pub const DURATION_TICKS: u32 = 15;
    }

    pub mod construct {
        /// Interaction distance (pixels) to start constructing a site.
        pub const INTERACTION_DISTANCE: f32 = 32.0;
        /// Labor ticks required to complete a campfire. Each tick one active
        /// constructor contributes 1 labor unit; multiple agents add up linearly.
        pub const CAMPFIRE_LABOR_TICKS: u32 = 120;
        /// Base planner cost for the Construct action.
        pub const BASE_COST: f32 = 3.0;
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
    }

    pub mod attack {
        pub const DURATION_TICKS: u32 = 30;
        pub const BASE_COST: f32 = 10.0;
    }

    pub mod walk {
        /// Estimated stamina cost per tile at normal speed (for planner estimation).
        pub const STAMINA_PER_TILE_NORMAL: f32 = 0.1;

        /// Estimated stamina cost per tile at tired speed (below TIRED_STAMINA_THRESHOLD).
        pub const STAMINA_PER_TILE_TIRED: f32 = 0.2;
    }

    pub mod graze {
        /// Drifting range (pixels) for a single graze session before the
        /// movement completes and the brain proposes another drift.
        pub const DRIFT_RANGE_MIN: f32 = 8.0;
        pub const DRIFT_RANGE_MAX: f32 = 20.0;
        /// Plant carbs ingested per second while grazing. Grass is low-calorie,
        /// so a full graze-loop trickles into the stomach rather than replacing
        /// a berry-sized meal.
        pub const STOMACH_CARBS_PER_SEC: f32 = 4.0;
    }
}

/// Brain behavior thresholds and urgency scores
pub mod brains {
    pub mod survival {
        /// Stamina level at which a sleeping agent wakes up fully rested.
        /// Legacy absolute threshold — still used by the planner's
        /// exhaustion gate. Wake-up checks now use
        /// `WAKE_STAMINA_FRACTION` so agents with genetic
        /// `aerobic_capacity < 1.0` (and therefore `aerobic_max < 100`)
        /// can still satisfy the rested-wake condition.
        pub const WAKE_STAMINA_THRESHOLD: f32 = 90.0;
        /// Fraction of `aerobic_max` at which a sleeping agent wakes up
        /// fully rested. Used instead of the absolute threshold so
        /// genetically below-average individuals (whose max is below
        /// the legacy 100) can actually reach a "well-rested" state.
        pub const WAKE_STAMINA_FRACTION: f32 = 0.9;
        /// Stamina safety margin used by the planner: if a walk would leave the agent below
        /// this level, Sleep is prepended so the survival brain doesn't interrupt the trip.
        pub const EXHAUSTION_TRIGGER: f32 = 15.0;
        /// Wakefulness level at which a sleeping agent wakes up naturally.
        /// Raised from 0.9 to 0.95 so a full night's sleep completes a
        /// proper 6–8 game hour cycle from wake ≈ 0.15 → 0.95 instead of
        /// waking half-rested every ~2 game hours.
        pub const WAKE_WAKEFULNESS_THRESHOLD: f32 = 0.95;
    }

    pub mod wakefulness {
        /// Base adenosine-like decay rate per rate-second while awake.
        /// Tuned so wakefulness goes from 1.0 to ~0.15 across ~16 game
        /// hours of awake time (960 rate-seconds at 60 tps), letting
        /// circadian boost nudge the crossover to land in the late-night
        /// window (22:00–02:00) on a noon-start day.
        pub const ADENOSINE_RATE: f32 = 0.00089;
        /// Sleep restore rate per rate-second while Sleep action is active.
        /// Tuned so a full sleep bout from ~0.15 → 0.95 takes ~8 game hours
        /// (480 rate-seconds), matching real-life human sleep duration.
        pub const SLEEP_RESTORE_RATE: f32 = 0.00167;
        /// How much the circadian cycle amplifies wakefulness decay at night.
        /// At full darkness (light = 0.3): effective multiplier = 1.0 + 1.0 * 0.7 = 1.7x.
        /// At full daylight (light = 1.0): multiplier = 1.0 (no change).
        pub const CIRCADIAN_NIGHT_BOOST: f32 = 1.0;
        /// Light level ceiling for the circadian boost formula. The boost
        /// is `(ceiling - current_light).max(0)`, so at full day (1.0) it's
        /// zero and at full night (0.3) it's 0.7.
        pub const CIRCADIAN_LIGHT_CEILING: f32 = 1.0;
        /// How much each 0.1 wakefulness deficit passively drags alertness.
        /// At wakefulness 0.5, alertness is capped at ~0.85. At 0.2, capped at ~0.6.
        pub const ALERTNESS_DRAG_PER_DEFICIT: f32 = 0.3;
        /// Daylight dampening factor for the Sleepiness urgency score.
        /// At full day (light=1.0) the Sleepiness score is multiplied by
        /// `1 - SLEEPINESS_DAYLIGHT_DAMPEN * 1.0 = 0.5`; at full night
        /// (light=0.3) the score is unchanged. Prevents agents from
        /// napping every two game hours regardless of the sun — sleep
        /// has to concentrate at night.
        pub const SLEEPINESS_DAYLIGHT_DAMPEN: f32 = 0.5;
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
        /// `InitiateConversation` toward a visible person. Lowered from
        /// 0.55 so agents initiate conversations more readily — in real
        /// life, seeing someone you know is usually enough to say hi.
        pub const SOCIAL_SEEK_THRESHOLD: f32 = 0.35;
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
        /// Alertness drained per brain decision cycle (arbitration tick).
        /// Thinking is mentally taxing; each decision burns a little mental fuel.
        pub const COGNITIVE_TICK_ALERTNESS_DRAIN: f32 = 0.003;
        /// Alertness drained per goal-directed plan generation.
        /// GOAP search is more expensive than simple arbitration.
        pub const PLAN_GENERATION_ALERTNESS_DRAIN: f32 = 0.02;
    }

    /// Cognitive load personality modulation
    pub mod cognition {
        /// How much conscientiousness reduces per-tick brain drain (0.0 = no effect,
        /// 1.0 = fully conscientious agents pay zero brain tick cost).
        pub const CONSCIENTIOUSNESS_TICK_RELIEF: f32 = 0.5;
        /// How much openness reduces plan generation drain. Curious agents
        /// enjoy thinking so they tire less from it.
        pub const OPENNESS_PLANNING_RELIEF: f32 = 0.6;
        /// Alertness drained from the speaker per conversation turn.
        /// Talking is mildly taxing — you're composing language and tracking
        /// the partner's state.
        pub const CONVERSATION_SPEAKER_ALERTNESS_DRAIN: f32 = 0.012;
        /// Alertness drained from each listener per conversation turn.
        /// Listening is more taxing than speaking because you're parsing
        /// unfamiliar content and updating your theory-of-mind.
        pub const CONVERSATION_LISTENER_ALERTNESS_DRAIN: f32 = 0.02;
        /// How much extraversion relieves conversation alertness drain.
        /// Introverts pay full price (social fatigue); extraverts pay very
        /// little (social stimulation is energising).
        pub const EXTRAVERSION_CONVERSATION_RELIEF: f32 = 0.85;
    }

    /// GOAP planner search parameters
    pub mod planner {
        pub const MAX_ITERATIONS: usize = 200;
        /// Cost per unmet goal condition in the A* heuristic
        pub const HEURISTIC_MULTIPLIER: f32 = 5.0;
    }
}
