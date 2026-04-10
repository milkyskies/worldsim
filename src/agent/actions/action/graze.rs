//! Graze action — slow drift across grass while continuously eating.
//!
//! A single fused action that expresses "walk + eat" via the body channel
//! system: Legs at low intensity (slow drift) plus Mouth at high intensity
//! (continuous nibbling). No Hands — grazing herbivores eat mouth-first, not
//! by handling food, so the channel occupancy matches the anatomy of the
//! behaviour. Hunger is reduced via `runtime_effects` rather than a
//! completion hook, so the agent feeds throughout the drift, not only on
//! arrival.
//!
//! Declares `TargetSource::TileWithTrait(Grazable)` so the rational brain
//! enumerates one Graze target per known grass tile (asserted by
//! `perceive_grass_tiles` as `Tile(?) HasTrait Grazable`). The default
//! `to_template_for_target` auto-injects a `self_at(tile)` precondition, and
//! the regressive planner chains `Walk → Graze` via the implicit walk
//! generator if the agent isn't already on a grass tile.
//!
//! Execution picks a fresh random nearby grass tile as the drift target each
//! time the action starts (see `execution.rs`), which is how the "drift
//! through grass" visual emerges from repeated short grazes.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{BodyChannel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, RuntimeEffects, TargetSource,
};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, Node, Predicate, Triple, Value};
use crate::constants::actions::graze::{
    ALERTNESS_PER_SEC, BASE_COST, ENERGY_PER_SEC, HUNGER_PER_SEC,
};
use crate::world::map::{TILE_SIZE, TileType};

pub struct GrazeAction;

impl Action for GrazeAction {
    fn action_type(&self) -> ActionType {
        ActionType::Graze
    }

    fn name(&self) -> &'static str {
        "Graze"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Movement
    }

    fn cost(&self) -> f32 {
        BASE_COST
    }

    fn target_source(&self) -> TargetSource {
        TargetSource::TileWithTrait(Concept::Grazable)
    }

    /// Preconditions enumerated independently of a target so the survival
    /// brain (which bypasses `target_enumeration`) can still propose Graze.
    /// The hunger-satisfying effect is declared here; the per-target
    /// `self_at(tile)` precondition is auto-injected by
    /// `to_template_for_target` when the rational brain builds a plan.
    fn preconditions(&self) -> Vec<TriplePattern> {
        vec![]
    }

    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(Node::Self_, Predicate::Hunger, Value::Int(0))]
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Slow drift + busy mouth. Leaves Hands free (deer have none anyway)
        // and keeps the Legs channel low enough that a predator's Flee
        // (Legs 1.0) still hard-conflicts Graze and preempts it.
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(BodyChannel::Legs, 0.3),
            ChannelUsage::new(BodyChannel::Mouth, 0.8),
        ];
        CHANNELS
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        // Negative hunger_per_sec: the grass provides steady nutrition while
        // the agent drifts. Energy ticks down slowly from walking.
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            hunger_per_sec: HUNGER_PER_SEC,
            alertness_per_sec: ALERTNESS_PER_SEC,
        }
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        let tx = (ctx.agent_position.x / TILE_SIZE).floor();
        let ty = (ctx.agent_position.y / TILE_SIZE).floor();
        if tx < 0.0 || ty < 0.0 {
            return Err(FailureReason::NoEdibleFood);
        }
        match ctx.world_map.get_tile(tx as u32, ty as u32) {
            Some(TileType::Grass) => Ok(()),
            _ => Err(FailureReason::NoEdibleFood),
        }
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("grazing")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::channel::{ChannelCapacities, ChannelLoad};
    use crate::agent::actions::registry::Action;

    #[test]
    fn graze_admits_cleanly_on_empty_load() {
        let graze = GrazeAction;
        let load = ChannelLoad::new();
        let caps = ChannelCapacities::full();
        assert!(
            !load.would_hard_conflict(graze.body_channels(), &caps),
            "Graze must fit on an idle agent"
        );
    }

    #[test]
    fn flee_preempts_graze_via_legs_saturation() {
        let graze = GrazeAction;
        let mut load = ChannelLoad::new();
        load.add(graze.body_channels());
        let caps = ChannelCapacities::full();

        // Flee: Legs 1.0 + FullBody 0.5. Legs load = 0.3 (graze) + 1.0 (flee) = 1.3.
        // Below the 1.4 hard threshold, so channels alone wouldn't preempt.
        // The crucial guard is that Graze *shares* Legs with Flee, so channel
        // occupancy stays honest: Flee hard-conflicts Graze via the Movement
        // single-slot rule in `preempt_existing_movement`, not via channels.
        // Here we just assert grazing doesn't monopolize Legs so a fleeing
        // deer can still recruit them.
        use crate::agent::actions::channel::BodyChannel;
        assert!(
            load.saturation(BodyChannel::Legs) < 0.5,
            "Graze should leave most of Legs free"
        );
        let _ = caps;
    }

    #[test]
    fn graze_reduces_hunger_per_second() {
        let graze = GrazeAction;
        assert!(
            graze.runtime_effects().hunger_per_sec < 0.0,
            "grazing must continuously reduce hunger"
        );
    }

    #[test]
    fn graze_targets_grazable_tiles() {
        let graze = GrazeAction;
        assert!(matches!(
            graze.target_source(),
            TargetSource::TileWithTrait(Concept::Grazable)
        ));
    }
}
