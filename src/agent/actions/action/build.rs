//! Build action - construct entities from materials in inventory.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects, SpawnRequest,
    TargetCandidate,
};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Triple, Value};
use crate::constants::actions::build::{
    CAMPFIRE_DURATION_TICKS, CAMPFIRE_WOOD_REQUIRED, HUNGER_PER_SEC, STAMINA_PER_SEC,
};

pub struct BuildAction;

impl Action for BuildAction {
    fn action_type(&self) -> ActionType {
        ActionType::Build
    }

    fn name(&self) -> &'static str {
        "Build"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: CAMPFIRE_DURATION_TICKS,
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Manipulation, 0.9),
            ChannelUsage::new(Channel::Locomotion, 0.2),
        ];
        CHANNELS
    }

    /// Planning: precondition is having the required wood in inventory.
    fn preconditions(&self) -> Vec<TriplePattern> {
        vec![TriplePattern::new(
            Some(Node::Self_),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Wood, 1)),
        )]
    }

    /// Planning: effect is a conceptual "agent has built a campfire".
    /// The planner uses this to chain goals (want campfire → plan build).
    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(
            Node::Self_,
            Predicate::Contains,
            Value::Item(Concept::Campfire, 1),
        )]
    }

    /// Planning: consuming wood from self prevents double-planning against the same stock.
    fn plan_consumes(&self) -> Vec<TriplePattern> {
        vec![TriplePattern::new(
            Some(Node::Self_),
            Some(Predicate::Contains),
            Some(Value::Item(Concept::Wood, 1)),
        )]
    }

    fn cost(&self) -> f32 {
        5.0
    }

    /// Build is uninterruptible: a half-built campfire is not something we want
    /// to drop because a smaller urgency edged in. The channel-level preempt
    /// pass in `nervous_system::execution::preempt_to_make_room` skips this
    /// action when looking for a victim, so any action whose channel
    /// requirements would otherwise displace Build is itself rejected.
    ///
    /// **Exit transition.** Build is `ActionKind::Timed { duration_ticks:
    /// CAMPFIRE_DURATION_TICKS }`. The standard tick countdown in
    /// `tick_actions` decrements `ticks_remaining` each tick and fires
    /// `on_complete` when it reaches zero. There is no special force-clear
    /// path — the action's natural completion is its only exit. This is safe
    /// because Build's body channels (Hands 0.9, Legs 0.2) leave room for
    /// the timed-action machinery to keep ticking it down without conflict
    /// with itself.
    ///
    /// Trade-off: an agent being chased by a wolf will keep building rather
    /// than flee. We accept this for now; tier-aware preemption (where Flee
    /// can override an uninterruptible Build because the urgency is high
    /// enough) is a future extension of this hook.
    fn interruptible(&self) -> bool {
        false
    }

    /// Runtime check: agent must have enough wood.
    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        let wood_count = ctx.inventory.count(Concept::Wood);
        if wood_count >= CAMPFIRE_WOOD_REQUIRED {
            Ok(())
        } else {
            Err(FailureReason::MissingMaterials)
        }
    }

    fn is_plan_valid(&self, _target: &TargetCandidate, mind: &MindGraph) -> bool {
        // Valid if the agent knows at least one recipe (campfire requires something)
        !mind
            .query(
                Some(&Node::Concept(Concept::Campfire)),
                Some(Predicate::Requires),
                None,
            )
            .is_empty()
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            stamina_per_sec: STAMINA_PER_SEC,
            hunger_per_sec: HUNGER_PER_SEC,
            ..Default::default()
        }
    }

    fn on_complete(&self, ctx: &mut CompletionContext) {
        // Consume materials from the agent's inventory.
        ctx.inventory.remove(Concept::Wood, CAMPFIRE_WOOD_REQUIRED);

        // Spawn a construction site rather than the finished campfire.
        // The site immediately receives the materials the agent just consumed,
        // so for a single-agent build the next `becomes_system` pass transforms
        // it into the finished entity. For collaborative builds (#62 Deposit
        // action), other agents top up partial slots over time.
        ctx.spawn_requests.push(SpawnRequest::Site {
            target: Concept::Campfire,
            position: ctx.agent_position,
            requirements: vec![(Concept::Wood, CAMPFIRE_WOOD_REQUIRED)],
            initial_items: vec![(Concept::Wood, CAMPFIRE_WOOD_REQUIRED)],
            labor_required: None,
        });
    }

    fn start_log(&self) -> Option<&'static str> {
        Some("started building")
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("built campfire")
    }
}
