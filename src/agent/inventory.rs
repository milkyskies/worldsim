//! inventory: EntityType marker component. Item storage is handled by item_slots::ItemSlots.
//!
//! Reads: Concept (item type vocabulary)
//! Writes: EntityType (marks what kind of entity something is)
//! Upstream: world entity spawning
//! Downstream: brain_system (type influences action choices), belief_updater (syncs type beliefs)

use crate::agent::mind::knowledge::Concept;
use bevy::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// ENTITY TYPE — Universal type marker using Concept
// ═══════════════════════════════════════════════════════════════════════════

/// Marks what type of thing an entity IS. Uses Concept as the shared vocabulary
/// between ECS (reality) and MindGraph (beliefs).
#[derive(Component, Reflect, Default, Clone, Copy, Debug)]
#[reflect(Component)]
pub struct EntityType(pub Concept);
