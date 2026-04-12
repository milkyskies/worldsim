//! Graze action — slow drift across grass while continuously eating.
//!
//! A single fused action that expresses "walk + eat" via the capability
//! channel system: `Locomotion` at low intensity (slow drift) plus
//! `Consumption` at high intensity (continuous nibbling). Herbivores eat
//! mouth-first, not by handling food, so no `Manipulation` channel is
//! declared — the occupancy matches the anatomy of the behaviour. Plant
//! carbs flow into the stomach continuously via `stomach_carbs_per_sec` on
//! `runtime_effects`, not a completion hook — the animal feeds throughout
//! the drift, not only on arrival.
//!
//! Declares `TargetSource::TileWithTrait(Grazable)` so the rational brain
//! enumerates one Graze target per known grass tile (asserted by
//! `perceive_grass_tiles` as `Tile(?) HasTrait Grazable`). The default
//! `to_template_for_target` auto-injects a `self_at(tile)` precondition,
//! and the regressive planner chains `Walk → Graze` via the implicit walk
//! generator if the agent isn't already on a grass tile.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, RuntimeEffects, TargetSource,
};
use crate::agent::body::effort::EffortProfile;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, Node, Predicate, Triple, Value};
use crate::constants::actions::graze::STOMACH_CARBS_PER_SEC;
use crate::world::map::TileType;

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
        2.0
    }

    fn target_source(&self) -> TargetSource {
        TargetSource::TileWithTrait(Concept::Grazable)
    }

    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(Node::Self_, Predicate::Hunger, Value::Int(0))]
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Locomotion, 0.3),
            ChannelUsage::new(Channel::Consumption, 0.8),
        ];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Graze is the fused walk-and-eat action — the animal is
        // continuously drifting, not committed in place.
        Some(Posture::Moving)
    }

    fn effort_profile(&self) -> EffortProfile {
        EffortProfile {
            locomotion: 0.15,
            cognition: 0.05,
            ..Default::default()
        }
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            stomach_carbs_per_sec: STOMACH_CARBS_PER_SEC,
            alertness_per_sec: 2.0,
            ..Default::default()
        }
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        match ctx.world_map.tile_at(ctx.agent_position) {
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

    #[test]
    fn graze_admits_cleanly_on_empty_load() {
        let graze = GrazeAction;
        let load = ChannelLoad::new();
        let caps = ChannelCapacities::full();
        assert!(!load.would_hard_conflict(graze.body_channels(), &caps));
    }

    #[test]
    fn graze_leaves_most_locomotion_free_for_flee() {
        // Graze needs to coexist with the *threat* of being preempted: a
        // deer hearing a wolf should still have Locomotion headroom for
        // Flee. With Graze at Locomotion 0.3, there's 0.7 free on the
        // channel — enough for Flee (1.0) to saturate and preempt via the
        // Movement single-slot rule.
        let graze = GrazeAction;
        let mut load = ChannelLoad::new();
        load.add(graze.body_channels());
        assert!(load.saturation(Channel::Locomotion) < 0.5);
    }

    #[test]
    fn graze_fills_stomach_per_second() {
        let graze = GrazeAction;
        assert!(
            graze.runtime_effects().stomach_carbs_per_sec > 0.0,
            "grazing continuously loads carbs into the stomach"
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
