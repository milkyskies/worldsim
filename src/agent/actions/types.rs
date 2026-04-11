//! Action type enum: the verbs agents can perform, separated from events.
//!
//! Reads: nothing (leaf type)
//! Writes: ActionType (used as value across the agent stack)
//! Upstream: none
//! Downstream: actions::registry, brains, nervous_system, ui::character_sheet

use bevy::prelude::*;

/// Defines the objective "verbs" agents can perform.
/// This separates Intent (Action) from Occurrence (Event).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, Default)]
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

    // Movement / Positioning
    Walk,    // "Travel"
    Wander,  // Random short-distance movement
    Explore, // Directed long-distance exploration to find resources
    #[default]
    Idle,
    /// Sit and recover. Milder than Sleep — some stamina gain without
    /// dropping alertness. The natural fit for mild fatigue that
    /// doesn't justify losing consciousness.
    Rest,
    /// Stand still and attend to a visible target. Satisfies curiosity
    /// without moving. A cat watching a bird, a wolf watching a deer
    /// from a distance, a human studying a stranger.
    Observe,
    /// Self-grooming and low-level body tending. The natural default
    /// when an agent has no drive pressing them toward anything.
    /// Animals at rest groom themselves; humans fidget, preen, tidy.
    Groom,

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
    Flee,
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl ActionType {
    /// Default desired locomotion intensity in [0, 1] for Movement-class
    /// actions (#339). `0.0` means this action isn't locomotion and the
    /// intensity field is unused. Brains may override the default via
    /// `pick_locomotion_intensity` to reflect urgency.
    pub fn default_locomotion_intensity(self) -> f32 {
        match self {
            // 0.25 keeps slow-drift wandering under Stamina::drain's
            // sprint gate (intensity > 0.3 → burns anaerobic). A
            // high-urgency territorial wander gets boosted by
            // `pick_locomotion_intensity` up to ~0.55 which does burn
            // anaerobic, so urgent patrol still feels the cost.
            ActionType::Wander => 0.25, // slow drift
            ActionType::Walk => 0.5,    // purposeful walk
            ActionType::Explore => 0.5,
            ActionType::Graze => 0.25, // walk-and-eat shuffle
            ActionType::Flee => 1.0,   // sprint
            ActionType::InitiateConversation => 0.5,
            _ => 0.0,
        }
    }

    /// Blend an action's default intensity with the brain's urgency so
    /// desperate agents push harder on the same action. Urgency is expected
    /// in the [0, 1] "normalized drive" scale (the brain produces 0-100
    /// scores; callers divide before passing). The boost caps at 0.3 so an
    /// ambient Walk can accelerate toward sprint without skipping straight
    /// to 1.0 on the first urgency bump.
    pub fn pick_locomotion_intensity(self, urgency_unit: f32) -> f32 {
        let default = self.default_locomotion_intensity();
        if default == 0.0 {
            return 0.0;
        }
        let boost = urgency_unit.clamp(0.0, 1.0) * 0.3;
        (default + boost).clamp(0.0, 1.0)
    }

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
            ActionType::Walk => "Walking to",
            ActionType::Wander => "Wandering",
            ActionType::Explore => "Exploring",
            ActionType::Idle => "Idle",
            ActionType::Rest => "Resting",
            ActionType::Observe => "Watching",
            ActionType::Groom => "Grooming",
            ActionType::Wave => "Waving at",
            ActionType::InitiateConversation => "Approaching",
            ActionType::Converse => "Talking to",
            ActionType::Attack => "Attacking",
            ActionType::Bite => "Biting",
            ActionType::Flee => "Fleeing from",
        }
    }
}
