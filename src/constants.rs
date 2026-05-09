//! Named constants for all magic numbers used throughout the simulation.
//!
//! Organized by domain. Import the relevant submodule where needed:
//! `use crate::constants::movement::BASE_SPEED_PER_TICK;`

/// World ambient thermodynamics and heat-field tuning (Celsius).
///
/// Units are real Celsius so values stay readable (`22.0` means 22°C).
/// The temperature grid stores deltas above ambient; `NIGHT_AMBIENT` and
/// `DAY_AMBIENT` swing the baseline as light level moves between its
/// night floor (0.3) and day ceiling (1.0).
pub mod thermal {
    /// Ambient temperature at full night (light level = 0.3). Deliberately
    /// below freezing so exposed agents feel real cold by bedtime and
    /// prioritize building/relighting fires. Temperate late-autumn
    /// feel, not tundra.
    pub const NIGHT_AMBIENT_C: f32 = -2.0;
    /// Ambient temperature at full day (light level = 1.0). Below the
    /// `COMFORT_MIN_C = 18°C` threshold on purpose — daytime is cool
    /// enough that agents lose warmth gently throughout the day, so
    /// by evening they're already looking for a fire instead of
    /// waiting for it to become an emergency.
    pub const DAY_AMBIENT_C: f32 = 12.0;

    /// Light-level floor (from `compute_light_level`) below which ambient
    /// stays pinned at `NIGHT_AMBIENT_C`. Matches the 0.3 floor used in
    /// `environment::compute_light_level`.
    pub const LIGHT_AT_NIGHT: f32 = 0.3;

    /// Start of the comfort band: above this cell temperature, a stationary
    /// agent recovers warmth passively.
    pub const COMFORT_MIN_C: f32 = 18.0;
    /// End of the comfort band: above this, an agent is thermoneutral
    /// (no warmth drain, no recovery beyond baseline). Overheat gameplay
    /// — drain above this — is a future concern.
    pub const COMFORT_MAX_C: f32 = 30.0;

    /// Heat injected per rate-second at an emitter's own tile for a
    /// full-intensity (intensity = 1.0) emitter. Scales linearly with
    /// `HeatSource::intensity` and falls off linearly to zero at the
    /// emitter's `radius`. Paired with `AMBIENT_RELAXATION_PER_SEC` so
    /// steady-state at source equals `RATE / RELAX` ≈ 57°C above
    /// ambient — putting the source tile above `FULL_RECOVERY_C` even
    /// at full-intensity 0.8 campfires on a sub-freezing night.
    pub const INJECTION_RATE_AT_SOURCE_C_PER_SEC: f32 = 20.0;

    /// Fraction of a cell's delta lost per rate-second to ambient. 0.35
    /// = 35%/sec, half-life ≈ 2 game-seconds. Fast enough that cells
    /// reach steady state inside the wall-clock timescale of
    /// "agent sits down next to a fire," slow enough that residual
    /// heat from an extinguished emitter is still visible for a
    /// several-second window.
    pub const AMBIENT_RELAXATION_PER_SEC: f32 = 0.35;

    /// How often (in ticks) the spatial-diffusion pass runs. Injection and
    /// relaxation run every tick; full neighbor-averaging is the expensive
    /// step and can run less often. 30 ticks = 0.5 game-seconds at 60Hz.
    pub const DIFFUSION_PERIOD_TICKS: u64 = 30;

    /// How much of a cell's delta mixes with its neighbors per diffusion
    /// pass. 0.25 means each pass pulls the cell 25% of the way toward
    /// the 4-neighbor average. Tuning trades sharpness (lower) vs. spread
    /// speed (higher).
    pub const DIFFUSION_BLEND: f32 = 0.25;

    /// Cell-delta magnitude below which a chunk may be pruned as
    /// equilibrated. Picked so sub-threshold numerical noise doesn't keep
    /// chunks pinned in memory.
    pub const EQUILIBRIUM_EPSILON_C: f32 = 0.05;

    /// How often (in ticks) equilibrated-chunk pruning runs. Infrequent
    /// because each pass is O(active cells). Game-minute-ish is fine.
    pub const PRUNE_PERIOD_TICKS: u64 = 3600;
}

/// World spawning configuration
pub mod world {
    pub const HUMAN_SPAWN_COUNT: usize = 12;
    /// Number of humans in the second cluster spawned across the river.
    pub const SECOND_GROUP_SPAWN_COUNT: usize = 8;
    pub const APPLE_TREE_SPAWN_COUNT: usize = 22;
    pub const BERRY_BUSH_SPAWN_COUNT: usize = 30;
    pub const DEER_SPAWN_COUNT: usize = 12;
    pub const WOLF_SPAWN_COUNT: usize = 8;
    pub const STONE_NODE_SPAWN_COUNT: usize = 14;
    pub const WOOD_LOG_SPAWN_COUNT: usize = 18;
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
    pub const SETTLEMENT_BERRY_BUSH_COUNT: usize = 10;

    /// Number of deer per herd. Total deer split across `DEER_SPAWN_COUNT /
    /// DEER_HERD_SIZE` herds.
    pub const DEER_HERD_SIZE: usize = 6;

    /// Tile radius for deer-herd cluster spawning.
    pub const DEER_HERD_RADIUS_TILES: u32 = 5;

    /// Minimum tile distance between a deer herd anchor and the human
    /// settlement center.
    pub const DEER_MIN_DISTANCE_FROM_SETTLEMENT: u32 = 18;

    /// Number of wolves per pack.
    pub const WOLF_PACK_SIZE: usize = 6;

    /// Tile radius for wolf-pack cluster spawning.
    pub const WOLF_PACK_RADIUS_TILES: u32 = 6;

    /// Minimum tile distance between a wolf pack anchor and the human settlement.
    pub const WOLF_MIN_DISTANCE_FROM_SETTLEMENT: u32 = 30;
}

/// Agent movement parameters
pub mod movement {
    /// Pixels per tick at normal stamina. Visually-tuned for the 60x wallclock
    /// compression (literal real-life walking would look like sprinting).
    /// 1.5 px/tick = pure RimWorld visual walking pace, comfortably clickable.
    /// In-game m/s is 0.094, well below real walking (1.4 m/s) — that gap is
    /// intentional; see docs/spatial_scale.md for the visual-feel rationale.
    pub const BASE_SPEED_PER_TICK: f32 = 1.5;
    pub const TIRED_STAMINA_THRESHOLD: f32 = 20.0;
    pub const TIRED_SPEED_MULTIPLIER: f32 = 0.5;
    pub const EXHAUSTED_STAMINA_THRESHOLD: f32 = 5.0;
    pub const EXHAUSTED_SPEED_MULTIPLIER: f32 = 0.2;
    /// Floor on movement even with fully destroyed legs (can always crawl)
    pub const MIN_INJURY_MOBILITY: f32 = 0.1;
    /// Upper range of leg-function contribution to movement (maps 0..1 → MIN..MIN+RANGE)
    pub const INJURY_MOBILITY_RANGE: f32 = 0.9;
}

pub mod biology {
    /// HP fraction at or below which a leg `BodyNode` flips the agent to
    /// `Lame`. Predator target enumeration prefers Lame prey.
    pub const LAMENESS_HP_FRACTION: f32 = 0.5;
}

/// Display thresholds shared by overhead status icons and the
/// character-sheet condition row, so a single threshold change moves
/// both surfaces together.
pub mod ui_status {
    pub const COLD_WARMTH: f32 = 0.3;
    pub const TIRED_AEROBIC_FRACTION: f32 = 0.2;
}

/// Per-action constants (durations, costs, runtime effects)
pub mod actions {
    pub mod eat {
        /// One Eat = ingesting one food item (one berry, one apple). ~20
        /// game-sec per item matches real chewing + swallowing time. A
        /// "meal" emerges naturally as a chain of Eats until the stomach-
        /// full precondition (`Eat::can_start`) blocks further Eat starts.
        pub const DURATION_TICKS: u32 = 20;
        pub const STAMINA_GAIN: f32 = 10.0;
    }

    pub mod drink {
        pub const DURATION_TICKS: u32 = 15;
        /// How much hydration (0..1 Need satisfaction) one Drink grants.
        pub const THIRST_REDUCTION: f32 = 0.5;
        pub const STAMINA_GAIN: f32 = 5.0;
    }

    pub mod harvest {
        pub const DURATION_TICKS: u32 = 30;
    }

    pub mod devour {
        /// One Devour = one bite of meat from a corpse. Slightly longer than
        /// Eat — tearing flesh off a carcass is more work than swallowing a
        /// berry. A pack feeding emerges as multiple wolves running Devour
        /// in parallel against the same target.
        pub const DURATION_TICKS: u32 = 25;
        pub const STAMINA_GAIN: f32 = 8.0;
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
        /// Wood required to start a lean-to construction site (consumed at
        /// site placement). Site requires the same amount via deposit slots.
        pub const LEAN_TO_WOOD_REQUIRED: u32 = 5;
        /// Labor ticks needed to finish a lean-to construction site after
        /// the wood slots are filled. ~150 ticks ≈ 2.5 game-minutes per
        /// solo agent — visible commitment without dragging.
        pub const LEAN_TO_LABOR_TICKS: u32 = 150;
        /// Wood required to start a house construction site.
        pub const HOUSE_WOOD_REQUIRED: u32 = 12;
        /// Stone required to start a house construction site.
        pub const HOUSE_STONE_REQUIRED: u32 = 6;
        /// Labor ticks needed to finish a house construction site.
        pub const HOUSE_LABOR_TICKS: u32 = 400;
    }

    pub mod cook {
        /// Ticks to cook one raw food item over a heat source. ~80 ticks =
        /// ~1.3 game-minutes — visible cooking time without making it
        /// dominate a hunger window.
        pub const DURATION_TICKS: u32 = 80;
        /// Raw food units consumed per cook.
        pub const RAW_REQUIRED: u32 = 1;
    }

    pub mod fish {
        /// Wait time at the water's edge before producing a Fish item.
        /// Fishing is mostly waiting — long enough that an agent visibly
        /// stays put, short enough that it's not boring.
        pub const DURATION_TICKS: u32 = 240;
    }

    pub mod share_food {
        pub const DURATION_TICKS: u32 = 15;
        /// Lower bound on `(Self, Affection, target)` for Share Food to
        /// fire — agents don't share with strangers or rivals.
        pub const MIN_AFFECTION: f32 = 0.4;
    }

    pub mod tend_wounds {
        pub const DURATION_TICKS: u32 = 60;
    }

    pub mod stand_watch {
        /// Hours of the in-game day during which Stand Watch is valid:
        /// `[NIGHT_START, 24) ∪ [0, NIGHT_END)`. 20:00–06:00 mirrors
        /// rural human sentinel patterns.
        pub const NIGHT_START_HOUR: u32 = 20;
        pub const NIGHT_END_HOUR: u32 = 6;
    }

    pub mod dance {
        pub const DURATION_TICKS: u32 = 90;
        /// Mood threshold above which Dance becomes available. `current_mood`
        /// is in `[-1.0, 1.0]`; > 0.7 is genuinely happy.
        pub const MIN_MOOD: f32 = 0.7;
        /// Companionship satisfaction threshold above which Dance fires —
        /// you don't dance when you're lonely.
        pub const MIN_COMPANIONSHIP: f32 = 0.6;
    }

    pub mod mourn {
        pub const DURATION_TICKS: u32 = 240;
        /// Window after a known agent's death within which Mourn applies.
        /// Beyond this, grief shifts from acute mourning to background memory.
        pub const RECENT_DEATH_WINDOW_TICKS: u64 = 86_400;
    }

    pub mod pickup {
        /// Quick pickup of a ground item — short enough to feel snappy in
        /// supply-chain hauling.
        pub const DURATION_TICKS: u32 = 10;
    }

    pub mod wave {
        /// Brief gestural greeting / pointing motion.
        pub const DURATION_TICKS: u32 = 10;
    }

    pub mod attack {
        pub const DURATION_TICKS: u32 = 30;
        pub const BASE_COST: f32 = 10.0;
    }

    pub mod defend_self {
        pub const DURATION_TICKS: u32 = 30;
        pub const BASE_COST: f32 = 10.0;
        /// Anger added to defender per CombatHit (before damage scaling).
        /// Anger accumulates until the general-anger threshold flips
        /// the agent from flee to retaliation.
        pub const ANGER_PER_HIT: f32 = 0.3;
        /// Fear added to defender per CombatHit (before damage scaling).
        /// Smaller than anger — repeated injury makes you angrier than
        /// scared once you're already committed to combat.
        pub const FEAR_PER_HIT: f32 = 0.15;
        /// Damage that scales the per-hit emotion increment to 1.0.
        /// A graze under this scales down; a heavy hit scales up.
        pub const DAMAGE_REFERENCE_HP: f32 = 30.0;
        pub const HIT_SCALE_MIN: f32 = 0.2;
        pub const HIT_SCALE_MAX: f32 = 1.5;
        /// Witnesses gain emotion at this fraction of the defender's
        /// per-hit increment — alarmed but not personally injured.
        pub const WITNESS_INTENSITY_FRACTION: f32 = 0.5;
    }

    pub mod walk {
        /// Estimated stamina cost per tile at normal speed (for planner estimation).
        /// Scales inversely with `BASE_SPEED_PER_TICK` so per-real-time fatigue
        /// stays consistent regardless of movement tuning. At 1.5 px/tick,
        /// 0.054/tile drains ~0.3 stamina per game-minute of walking, so a
        /// 100-stamina pool sustains ~5 game-hours of continuous travel.
        pub const STAMINA_PER_TILE_NORMAL: f32 = 0.054;

        /// Estimated stamina cost per tile at tired speed (below TIRED_STAMINA_THRESHOLD).
        pub const STAMINA_PER_TILE_TIRED: f32 = 0.108;
    }

    pub mod warm_up {
        /// Warmth at which the stance auto-completes. Just below the 0.95
        /// satiation gate so a small dip doesn't reject an immediate
        /// re-entry.
        pub const COMPLETE_WARMTH_FRACTION: f32 = 0.9;
        /// Small stamina gain from sitting warm — rest-like by-product.
        pub const STAMINA_GAIN: f32 = 5.0;
    }

    pub mod rest_in_shelter {
        /// Rest-quality value at which the stance auto-completes. Mirrors
        /// `warm_up::COMPLETE_WARMTH_FRACTION` so re-entry hysteresis matches.
        pub const COMPLETE_REST_QUALITY_FRACTION: f32 = 0.9;
    }

    pub mod lean_to {
        /// Initial durability of a freshly-built lean-to, in arbitrary HP units.
        pub const INITIAL_DURABILITY: f32 = 50.0;
        /// Per-tick decay applied by `durability_system`. Tuned so a lean-to
        /// despawns after roughly one game-week of neglect (~604,800 ticks).
        pub const DURABILITY_DECAY_PER_TICK: f32 = 0.0001;
        /// How many sleepers a lean-to can shelter at once.
        pub const CAPACITY: u32 = 2;
        /// Shelter-quality multiplier — feeds the rest-quality recovery rate
        /// and the existing `shelter_system` aerobic bonus.
        pub const PROTECTION: f32 = 1.5;
        /// Burn time once ignited, in seconds.
        pub const FLAMMABLE_BURN_TIME: f32 = 200.0;
    }

    pub mod house {
        /// Initial durability of a freshly-built house. Far higher than
        /// the lean-to so a real investment outlasts a season.
        pub const INITIAL_DURABILITY: f32 = 200.0;
        /// Per-tick decay applied by `durability_system`. Lower than the
        /// lean-to's so a house lasts roughly five times as long.
        pub const DURABILITY_DECAY_PER_TICK: f32 = 0.00002;
        /// How many sleepers a house can shelter at once.
        pub const CAPACITY: u32 = 4;
        /// Shelter-quality multiplier.
        pub const PROTECTION: f32 = 2.5;
        /// Burn time once ignited, in seconds.
        pub const FLAMMABLE_BURN_TIME: f32 = 500.0;
    }

    pub mod rest {
        /// Aerobic fraction at which Rest self-completes. Matches the
        /// `WAKE_STAMINA_FRACTION` (0.9) spirit but slightly higher
        /// because Rest is lighter recovery — the agent should sit until
        /// nearly topped off before getting back up.
        pub const COMPLETE_AEROBIC_FRACTION: f32 = 0.95;
    }

    pub mod graze {
        /// Drifting range (pixels) for a single graze session before the
        /// movement completes and the brain proposes another drift. Tuned so
        /// each drift segment is 1-2.5 tiles — enough to read on screen,
        /// short enough not to wander out of grazing range.
        pub const DRIFT_RANGE_MIN: f32 = 15.0;
        pub const DRIFT_RANGE_MAX: f32 = 40.0;
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

    /// Rest-quality drive: sleep-quality drain and recovery.
    pub mod rest_quality {
        /// Baseline satisfaction drain per rate-second. Slow trickle so an
        /// agent who never sleeps in shelter notices it over a couple of
        /// game-days.
        pub const BASELINE_DRAIN_PER_SEC: f32 = 0.0003;
        /// Recovery per rate-second per unit of `ShelterProvider.protection`
        /// while near one. With LeanTo `PROTECTION = 1.5` this brings a
        /// depleted agent back to comfort over a single sleep bout.
        pub const SHELTER_RECOVERY_PER_SEC: f32 = 0.018;
        /// Rest-quality above this value produces near-zero urgency.
        pub const COMFORT_THRESHOLD: f32 = 0.6;
        /// Rest-quality at or below this value produces urgent demand.
        pub const URGENT_THRESHOLD: f32 = 0.3;
        /// Rest-quality at or below this value is a long-term debility.
        pub const CRITICAL_THRESHOLD: f32 = 0.1;
        /// Minimum urgency below which the drive is suppressed.
        pub const MIN_URGENCY_THRESHOLD: f32 = 0.05;
    }

    /// Warmth drive: thermal comfort drain and recovery.
    pub mod warmth {
        /// Baseline satisfaction drain per rate-second in neutral conditions
        /// (no exposure, not near heat). Slow trickle — an unattended agent
        /// noticeably cools over ~15 game minutes.
        pub const BASELINE_DRAIN_PER_SEC: f32 = 0.00055;
        /// Extra satisfaction drain per rate-second when the agent is exposed
        /// (not within a HeatSource radius and not inside a ShelterProvider).
        /// Additive on top of the baseline. Tuned so an exposed agent drops
        /// into the urgent band (warmth < 0.3) in roughly 5 game minutes.
        pub const EXPOSURE_DRAIN_PER_SEC: f32 = 0.0028;
        /// Satisfaction gain per rate-second when within a lit HeatSource
        /// radius. Passive: applies regardless of action. Target ~60
        /// game-seconds cold-to-warm.
        pub const HEAT_RECOVERY_PER_SEC: f32 = 0.017;
        /// Warmth above this value produces near-zero urgency.
        pub const COMFORT_THRESHOLD: f32 = 0.6;
        /// Warmth at or below this value produces urgency rivalling Hunger.
        pub const URGENT_THRESHOLD: f32 = 0.3;
        /// Warmth at or below this value produces life-threatening urgency.
        pub const CRITICAL_THRESHOLD: f32 = 0.1;
        /// Minimum urgency value below which the Warmth drive is suppressed —
        /// matches the `min_threshold` convention used by other drives so a
        /// barely-cool agent doesn't clutter the urgency list.
        pub const MIN_URGENCY_THRESHOLD: f32 = 0.05;
    }

    pub mod wakefulness {
        /// Base adenosine-like decay rate per rate-second while awake.
        /// Tuned against a 06:00 start so wakefulness drops to the
        /// sleepiness-winning threshold around 22:00 — ~16 game-hours
        /// awake — and 8h of sleep lands wake-up near 06:00 the next
        /// morning. The circadian boost nudges evening drain up so
        /// bedtime doesn't drift too far past 22:00.
        pub const ADENOSINE_RATE: f32 = 0.00066;
        /// Sleep restore rate per rate-second while Sleep action is active.
        /// Tuned so a full sleep bout from bedtime wakefulness (~0.1)
        /// up to the 0.95 wake threshold takes ~8 game hours — agents
        /// sleep from roughly 00:00 to 08:00, matching real-life human
        /// sleep duration.
        pub const SLEEP_RESTORE_RATE: f32 = 0.00148;
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
        /// Multiplier applied to a Flee `ThreatResponse::urgency` to
        /// produce the proposal urgency. Set so saturated outmatching
        /// (urgency 1.5) rivals `FEAR_GENERAL_URGENCY_MULTIPLIER`.
        pub const FLEE_RESPONSE_URGENCY_MULTIPLIER: f32 = 70.0;
        /// Floor for a StandGround proposal — beats drift/idle but loses
        /// to anything pressing.
        pub const STAND_GROUND_BASE_URGENCY: f32 = 25.0;
        /// Base urgency for a Fight proposal; commitment scales it up.
        pub const FIGHT_RESPONSE_BASE_URGENCY: f32 = 60.0;
        pub const FIGHT_RESPONSE_COMMITMENT_MULTIPLIER: f32 = 60.0;
        /// Social drive above which the emotional brain proposes
        /// `InitiateConversation` toward a visible person. Lowered from
        /// 0.55 so agents initiate conversations more readily — in real
        /// life, seeing someone you know is usually enough to say hi.
        pub const SOCIAL_SEEK_THRESHOLD: f32 = 0.35;
        /// Multiplier applied to social drive to score the urgency of
        /// initiating a conversation.
        pub const SOCIAL_SEEK_URGENCY_MULTIPLIER: f32 = 40.0;

        /// Warmth deficit above which the emotional brain proposes a Walk
        /// toward a visible heat source. Set at the same point as
        /// `SOCIAL_SEEK_THRESHOLD` so "drift toward comfort" fires at
        /// parallel thresholds across drives — cold enough to care, not
        /// yet cold enough to be a survival emergency.
        pub const WARMTH_SEEK_THRESHOLD: f32 = 0.35;
        /// Multiplier applied to warmth deficit to score the urgency of
        /// walking toward a heat source. Matches the social-seek
        /// multiplier so the arbitrator weighs the two comfort-drifts
        /// on the same scale.
        pub const WARMTH_SEEK_URGENCY_MULTIPLIER: f32 = 40.0;

        /// Sleepiness deficit above which Sleep fires in place and skips
        /// the `location_preference` prep pass. Exhausted agents sleep
        /// wherever — life beats quality.
        pub const EMERGENCY_SLEEPINESS: f32 = 0.9;
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
