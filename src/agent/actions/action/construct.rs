//! Construct action — work on a construction site that requires labor.
//!
//! This is the "hammer on it" phase that follows the "place the site" phase
//! from the Build action. While this action runs the `labor_accumulation_system`
//! increments the target site's `LaborAccumulated.current` by 1 per tick.
//! Multiple agents constructing the same site add up linearly.
//!
//! The action runs indefinitely (like Sleep) until the site's `Becomes`
//! trigger fires, at which point the site entity is despawned and the
//! execution system cancels Construct via the `target_gone` path.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{BodyChannel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects, TargetCandidate,
    TargetSource,
};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{MindGraph, Node, Predicate, Triple, Value};
use crate::constants::actions::construct::{BASE_COST, ENERGY_PER_SEC, HUNGER_PER_SEC};

pub struct ConstructAction;

impl Action for ConstructAction {
    fn action_type(&self) -> ActionType {
        ActionType::Construct
    }

    fn name(&self) -> &'static str {
        "Construct"
    }

    /// Indefinite — runs until the site transforms (target despawned) or the
    /// agent is interrupted. Uses `u32::MAX` so the tick machinery treats it
    /// as never-autocomplete, exactly like Sleep and Idle.
    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: u32::MAX,
        }
    }

    /// Construction sites are world entities with a `Construct` affordance.
    fn target_source(&self) -> TargetSource {
        TargetSource::EntityAffordance
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(BodyChannel::Hands, 0.8),
            ChannelUsage::new(BodyChannel::Legs, 0.1),
        ];
        CHANNELS
    }

    fn cost(&self) -> f32 {
        BASE_COST
    }

    /// Generic precondition: agent must have something to work toward.
    /// Per-target precondition (proximity) is injected by `to_template_for_target`.
    fn preconditions(&self) -> Vec<TriplePattern> {
        vec![]
    }

    /// Effect from the planner's perspective: constructing this site will
    /// eventually produce the finished entity, treated as if the agent now
    /// "has" that entity. This drives the backward chain:
    ///   Want warmth → campfire provides warmth → construct site → walk to site
    fn plan_effects_for_target(&self, target: &TargetCandidate, mind: &MindGraph) -> Vec<Triple> {
        let Some(entity) = target.as_entity() else {
            return vec![];
        };
        // Derive the target concept from the (entity, Becomes, concept) belief.
        mind.query(Some(&Node::Entity(entity)), Some(Predicate::Becomes), None)
            .into_iter()
            .filter_map(|t| {
                if let Value::Concept(c) = t.object {
                    Some(Triple::new(
                        Node::Self_,
                        Predicate::Contains,
                        Value::Item(c, 1),
                    ))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Only valid when the target entity is known to become something (i.e. the
    /// agent has perceived the site's `Becomes` component).
    fn is_plan_valid(&self, target: &TargetCandidate, mind: &MindGraph) -> bool {
        let Some(entity) = target.as_entity() else {
            return false;
        };
        !mind
            .query(Some(&Node::Entity(entity)), Some(Predicate::Becomes), None)
            .is_empty()
    }

    /// Runtime check: target must still exist.
    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if ctx.target_entity.is_none() {
            return Err(FailureReason::TargetGone);
        }
        Ok(())
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            hunger_per_sec: HUNGER_PER_SEC,
            ..Default::default()
        }
    }

    /// `on_complete` is intentionally empty: the site transforms (and despawns)
    /// when `becomes_system` fires after `LaborAccumulated.current >= required`.
    /// The execution system cancels this action via the `target_gone` path.
    fn on_complete(&self, _ctx: &mut CompletionContext) {}

    fn start_log(&self) -> Option<&'static str> {
        Some("started constructing")
    }
}
