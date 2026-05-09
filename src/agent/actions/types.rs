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
    /// Owned by the CommunicationPlugin — on arrival within CONVERSATION_RANGE
    /// the plugin swaps this for `Converse` and registers a Conversation.
    InitiateConversation,
    /// Body-channel marker for being in a conversation. Inserted and removed
    /// by the CommunicationPlugin — never proposed by brains directly.
    Converse,
    Attack,
    /// Jaws-as-weapon attack. Requires `Channel::Bite`, so only species
    /// whose anatomy provides it (wolves, future crocodiles, snakes) can
    /// perform it. Distinct from `Attack`, which needs `Manipulation` and
    /// covers hands / weapons / grapples.
    Bite,
    /// Predator/scavenger feeding from a corpse. Tears meat off the
    /// target's ItemSlots in place — no Harvest hop into personal
    /// inventory. Multiple agents can devour the same corpse on the
    /// same tick (pack feeding). Distinct from `Eat` (eat-from-own-
    /// inventory) so the species split stays clean: humans Eat, wolves
    /// Devour.
    Devour,
    Flee,
    /// Counterattack against a `Dangerous` target — the non-prey
    /// counterpart of `Attack`. Proposed by the emotional brain when
    /// accumulated Anger crosses a threshold.
    DefendSelf,
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
            ActionType::Observe => "Watching",
            ActionType::Wave => "Waving at",
            ActionType::InitiateConversation => "Approaching",
            ActionType::Converse => "Talking to",
            ActionType::Attack => "Attacking",
            ActionType::Bite => "Biting",
            ActionType::Devour => "Devouring",
            ActionType::Flee => "Fleeing from",
            ActionType::DefendSelf => "Defending against",
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
            ActionType::Observe => "Observe",
            ActionType::Wave => "Wave",
            ActionType::InitiateConversation => "InitiateConversation",
            ActionType::Converse => "Converse",
            ActionType::Attack => "Attack",
            ActionType::Bite => "Bite",
            ActionType::Devour => "Devour",
            ActionType::Flee => "Flee",
            ActionType::DefendSelf => "DefendSelf",
        }
    }
}
