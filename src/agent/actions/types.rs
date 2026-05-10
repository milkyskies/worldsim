//! Action type enum: the verbs agents can perform, separated from events.
//!
//! Reads: nothing (leaf type)
//! Writes: ActionType (used as value across the agent stack)
//! Upstream: none
//! Downstream: actions::registry, brains, nervous_system, ui::character_sheet

use bevy::prelude::*;

/// Defines the objective "verbs" agents can perform.
/// This separates Intent (Action) from Occurrence (Event).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default, serde::Serialize)]
pub enum ActionType {
    // Survival / Biological
    Eat,
    Sleep,
    WakeUp, // NEW: Transition from sleep to awake
    Drink,
    /// Slow walk + continuous eating over a grass tile. Occupies
    /// Locomotion + Consumption simultaneously so the channel system
    /// expresses the "walk and eat at the same time" behaviour as one
    /// fused drift.
    Graze,

    // Resource
    Harvest,
    Pickup,
    Drop,
    Build,
    /// Build a lean-to shelter from wood at the agent's tile. Spawns a
    /// construction site that becomes a `LeanTo` once labor is contributed.
    BuildLeanTo,
    /// Build a wooden house from wood and stone at the agent's tile.
    /// Spawns a construction site that becomes a `House` once materials are
    /// deposited and labor is accumulated.
    BuildHouse,
    /// Build a wooden storage chest at the agent's tile. Spawns a
    /// construction site that becomes a public-access `StorageChest`.
    BuildStorageChest,
    /// Move items from the agent's own slots into a target entity's slots.
    /// Polymorphic across construction sites, chests, furnaces, etc. — the
    /// target's `ItemSlots` filters and access rules decide what's accepted.
    Deposit,
    /// Move items from a target entity's slots into the agent's own slots.
    /// Polymorphic across chests, dropped piles, furnace outputs, etc. —
    /// the target's `extract_access` decides what can leave.
    Take,
    /// Transform a raw food item in inventory into its cooked variant by
    /// standing near a `HeatEmitting` entity. Consumes one raw unit and
    /// produces one cooked unit with freshness stamped at completion.
    Cook,

    // Movement / Positioning
    Walk,    // "Travel"
    Wander,  // Random short-distance movement
    Explore, // Open-ended curiosity wandering toward stale chunks
    /// Goal-directed search for a specific concept when the agent has a
    /// drive but no known instance (e.g. hungry with no known food).
    /// Biases target selection toward chunks with MindGraph hints for
    /// the search concept. Rational brain proposes this as a fallback
    /// when `derive_search_concept` says the driving urgency's satisfier
    /// has an `isa_filter` / `trait_filter` precondition.
    LookFor,
    #[default]
    Idle,
    /// Sit and recover. Milder than Sleep — some stamina gain without
    /// dropping alertness. The natural fit for mild fatigue that
    /// doesn't justify losing consciousness.
    Rest,
    /// Stay beside a heat source to restore warmth. The direct satisfier
    /// of `NeedKind::Warmth` — mirrors Eat/Drink/Sleep: stays in place,
    /// passively absorbs the drive-restoring effect over time.
    WarmUp,
    /// Stay inside a shelter to recover rest-quality. The direct satisfier
    /// of `NeedKind::RestQuality` — mirrors WarmUp: stays in place near a
    /// `ShelterProvider`, passively absorbs the drive-restoring effect.
    RestInShelter,
    /// Stand near a storage chest to recover food-security. The direct
    /// satisfier of `NeedKind::FoodSecurity` — same shape as RestInShelter
    /// for shelter and WarmUp for warmth.
    StockChest,
    /// Stand still and attend to a visible target. Satisfies curiosity
    /// without moving. A cat watching a bird, a wolf watching a deer
    /// from a distance, a human studying a stranger.
    Observe,
    /// Work on a construction site that requires labor to complete.
    /// Targets a world entity with a `Becomes` component whose trigger tree
    /// contains a `LaborAccumulated` variant. Running this action each tick
    /// causes the `labor_accumulation_system` to increment the site's labor
    /// counter by one. The action runs indefinitely until the target is
    /// despawned (i.e. the site transforms into the finished entity).
    Construct,

    // Social / Combat
    Wave,
    /// Walk-to-target marker proposed by brains to start a conversation.
    /// Owned by the ConversePlugin — on arrival within CONVERSATION_RANGE
    /// the plugin swaps this for `Converse` and registers a Conversation.
    InitiateConversation,
    /// Body-channel marker for being in a conversation. Inserted and removed
    /// by the ConversePlugin — never proposed by brains directly.
    Converse,
    /// Walk-to-prey marker proposed by brains to start a hunt engagement.
    /// Owned by the HuntPlugin — on arrival within strike range the plugin
    /// installs `EngagedHunt` and starts the strike loop.
    InitiateHunt,
    /// Walk-to-corpse marker proposed by brains to start a devour engagement.
    /// Owned by the DevourPlugin — on arrival the plugin installs
    /// `EngagedDevour` and starts the bite loop.
    InitiateDevour,
    /// Walk-to-bush marker proposed by brains to start a harvest engagement.
    /// Owned by the HarvestPlugin — on arrival the plugin installs
    /// `EngagedHarvest` and starts the per-yield loop.
    InitiateHarvest,
    /// Marker proposed by emotional/survival brain when fear urgency
    /// crosses threshold. Owned by the FleePlugin — installs `EngagedFlee`
    /// and tracks the threat target across ticks.
    InitiateFlee,
    /// Marker proposed by survival brain when sleepiness crosses threshold.
    /// Owned by the SleepPlugin — picks a sleep spot, walks there, then
    /// installs `EngagedSleep` for the duration.
    InitiateSleep,
    Attack,
    /// Jaws-as-weapon strike. Engagement-internal beat owned by the
    /// HuntPlugin — never proposed directly by brains. The 30-tick
    /// duration is the strike cooldown within a hunt, not a stationary
    /// action duration.
    Bite,
    /// Predator/scavenger feeding from a corpse. Engagement-internal beat
    /// owned by the DevourPlugin — never proposed directly. Multiple
    /// agents can devour the same corpse on the same tick (pack feeding).
    Devour,
    /// Per-tick flight beat owned by the FleePlugin. Tracks the threat
    /// target's current position, not a snapshot. Never proposed by brains.
    Flee,
    /// Counterattack against a `Dangerous` target — the non-prey
    /// counterpart of `Attack`. Proposed by the emotional brain when
    /// accumulated Anger crosses a threshold.
    DefendSelf,

    // Settled-life expressions (#269)
    /// Idle stance with a seated visual — the "settlements look lived-in"
    /// alternative to standing Idle. Light Locomotion gate so it mutexes
    /// against Walk/Wander.
    Sit,
    /// Wait by water for a catch. Adjacent-to-water gated; produces a
    /// `Fish` item on completion.
    Fish,
    /// Hand a food item to a nearby agent. The prosocial counterpart of
    /// Deposit, gated on positive affection toward the recipient.
    ShareFood,
    /// First-aid stance: heal a nearby injured agent's wounds.
    TendWounds,
    /// Sentinel posture at night near a campfire. Replaces Sleep for one
    /// agent so the rest of the camp can sleep safely.
    StandWatch,
    /// Group celebration when mood is high and social drive is satisfied —
    /// emotional contagion radiates joy to nearby agents.
    Dance,
    /// Stationary grief processing after the agent's MindGraph records
    /// the death of a known agent.
    Mourn,
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl ActionType {
    /// Human-readable present-participle verb ("Eating", "Fleeing from", ...)
    /// for the character sheet. Verbs ending in a preposition imply a target
    /// follows (e.g. "Walking to <place>", "Attacking <target>").
    pub fn verb(self) -> &'static str {
        match self {
            ActionType::Eat => "Eating",
            ActionType::Sleep => "Sleeping",
            ActionType::WakeUp => "Waking up",
            ActionType::Drink => "Drinking",
            ActionType::Graze => "Grazing",
            ActionType::Construct => "Constructing",
            ActionType::Harvest => "Harvesting",
            ActionType::Pickup => "Picking up",
            ActionType::Drop => "Dropping",
            ActionType::Build => "Building",
            ActionType::BuildLeanTo => "Building lean-to",
            ActionType::BuildHouse => "Building house",
            ActionType::BuildStorageChest => "Building storage chest",
            ActionType::Deposit => "Depositing into",
            ActionType::Take => "Taking from",
            ActionType::Cook => "Cooking",
            ActionType::Walk => "Walking to",
            ActionType::Wander => "Wandering",
            ActionType::Explore => "Exploring",
            ActionType::LookFor => "Looking for",
            ActionType::Idle => "Idle",
            ActionType::Rest => "Resting",
            ActionType::WarmUp => "Warming up by",
            ActionType::RestInShelter => "Resting inside",
            ActionType::StockChest => "Stocking the chest",
            ActionType::Observe => "Watching",
            ActionType::Wave => "Waving at",
            ActionType::InitiateConversation => "Approaching",
            ActionType::InitiateHunt => "Closing on",
            ActionType::InitiateDevour => "Approaching corpse",
            ActionType::InitiateHarvest => "Approaching",
            ActionType::InitiateFlee => "Bolting from",
            ActionType::InitiateSleep => "Settling down",
            ActionType::Converse => "Talking to",
            ActionType::Attack => "Attacking",
            ActionType::Bite => "Biting",
            ActionType::Devour => "Devouring",
            ActionType::Flee => "Fleeing from",
            ActionType::DefendSelf => "Defending against",
            ActionType::Sit => "Sitting",
            ActionType::Fish => "Fishing",
            ActionType::ShareFood => "Sharing food with",
            ActionType::TendWounds => "Tending wounds of",
            ActionType::StandWatch => "Standing watch",
            ActionType::Dance => "Dancing",
            ActionType::Mourn => "Mourning",
        }
    }

    /// Short action name for logs and planning templates. One source of
    /// truth so `Action::name()` can't drift from the enum variant.
    pub fn name(self) -> &'static str {
        match self {
            ActionType::Eat => "Eat",
            ActionType::Sleep => "Sleep",
            ActionType::WakeUp => "Wake Up",
            ActionType::Drink => "Drink",
            ActionType::Graze => "Graze",
            ActionType::Construct => "Construct",
            ActionType::Harvest => "Harvest",
            ActionType::Pickup => "Pickup",
            ActionType::Drop => "Drop",
            ActionType::Build => "Build",
            ActionType::BuildLeanTo => "BuildLeanTo",
            ActionType::BuildHouse => "BuildHouse",
            ActionType::BuildStorageChest => "BuildStorageChest",
            ActionType::Deposit => "Deposit",
            ActionType::Take => "Take",
            ActionType::Cook => "Cook",
            ActionType::Walk => "Walk",
            ActionType::Wander => "Wander",
            ActionType::Explore => "Explore",
            ActionType::LookFor => "LookFor",
            ActionType::Idle => "Idle",
            ActionType::Rest => "Rest",
            ActionType::WarmUp => "WarmUp",
            ActionType::RestInShelter => "RestInShelter",
            ActionType::StockChest => "StockChest",
            ActionType::Observe => "Observe",
            ActionType::Wave => "Wave",
            ActionType::InitiateConversation => "InitiateConversation",
            ActionType::InitiateHunt => "InitiateHunt",
            ActionType::InitiateDevour => "InitiateDevour",
            ActionType::InitiateHarvest => "InitiateHarvest",
            ActionType::InitiateFlee => "InitiateFlee",
            ActionType::InitiateSleep => "InitiateSleep",
            ActionType::Converse => "Converse",
            ActionType::Attack => "Attack",
            ActionType::Bite => "Bite",
            ActionType::Devour => "Devour",
            ActionType::Flee => "Flee",
            ActionType::DefendSelf => "DefendSelf",
            ActionType::Sit => "Sit",
            ActionType::Fish => "Fish",
            ActionType::ShareFood => "ShareFood",
            ActionType::TendWounds => "TendWounds",
            ActionType::StandWatch => "StandWatch",
            ActionType::Dance => "Dance",
            ActionType::Mourn => "Mourn",
        }
    }

    /// True for engagement-internal beats that may only be inserted into
    /// `ActiveActions` by an engagement plugin — never proposed by a
    /// brain. Brain-side validation rejects any proposal of a beat.
    pub fn is_beat(self) -> bool {
        matches!(
            self,
            ActionType::Converse
                | ActionType::Bite
                | ActionType::Devour
                | ActionType::Flee
                | ActionType::Sleep
                | ActionType::Harvest
        )
    }

    /// True for the "InitiateX" entry-point actions that brains propose
    /// to start an engagement. The matching engagement plugin consumes
    /// the initiate action and installs its kind-specific marker.
    pub fn is_engagement_initiator(self) -> bool {
        matches!(
            self,
            ActionType::InitiateConversation
                | ActionType::InitiateHunt
                | ActionType::InitiateDevour
                | ActionType::InitiateHarvest
                | ActionType::InitiateFlee
                | ActionType::InitiateSleep
        )
    }
}
