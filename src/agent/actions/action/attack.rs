use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::motor::{ActionPrimitive, Behavior, IntensityPolicy, TargetSelector};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, TargetCandidate, TargetSource,
};
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Triple, Value};
use crate::constants::actions::attack::{BASE_COST, DURATION_TICKS};

pub struct AttackAction;

impl Action for AttackAction {
    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            ActionPrimitive::Manipulate,
            TargetSelector::InPlace,
            IntensityPolicy::Fixed(0.0),
            crate::agent::actions::motor::Intent::Safety,
        )
    }

    fn name(&self) -> &'static str {
        "Attack"
    }

    fn action_type(&self) -> ActionType {
        ActionType::Attack
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: DURATION_TICKS,
        }
    }

    fn cost(&self) -> f32 {
        BASE_COST
    }

    fn target_source(&self) -> TargetSource {
        TargetSource::EntityWithTrait(Concept::Prey)
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Manual melee — requires hands/arms to strike. Wolves can't use
        // this (no Manipulation); they get `BiteAction` instead. Humans use
        // it for punches, grapples, and held weapons. No Locomotion claim
        // — posture-agnostic means the agent can charge, grapple in motion,
        // or strike from a standstill, and the posture gate doesn't care.
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Manipulation, 0.9),
            ChannelUsage::new(Channel::FullBody, 0.7),
            ChannelUsage::new(Channel::Focus, 0.3),
            ChannelUsage::new(Channel::Awareness, 0.5),
        ];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        // Posture-agnostic: a human can punch while walking, grapple while
        // charging, or strike from a standstill. Attack claims full body
        // via FullBody 0.7 but doesn't pick a posture.
        None
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), crate::agent::events::FailureReason> {
        if ctx.target_entity.is_none() {
            return Err(crate::agent::events::FailureReason::NoTarget);
        }
        Ok(())
    }

    fn plan_effects_for_target(&self, target: &TargetCandidate, mind: &MindGraph) -> Vec<Triple> {
        let Some(entity) = target.as_entity() else {
            return self.plan_effects();
        };
        prey_yield_effects(entity, mind)
    }

    fn is_plan_valid(&self, target: &TargetCandidate, mind: &MindGraph) -> bool {
        let Some(entity) = target.as_entity() else {
            return false;
        };
        prey_produces_useful_item(entity, mind)
    }

    // Damage, dodge, death, and meat deposit all live in
    // `biology::combat::resolve_combat_hits`, which consumes the
    // `SimEvent::ActionCompleted` this action emits. Keeping on_complete
    // empty here means the planner / action registry only knows
    // "Attack is a timed action that needs Manipulation" — every bit of
    // combat semantics stays in the combat module.
    fn on_complete(&self, _ctx: &mut CompletionContext) {}
}

/// `(Self_, Contains, Item(X, n))` projections for everything the entity
/// produces — direct `Produces` first, then indirect via `IsA` type chain.
pub(crate) fn prey_yield_effects(entity: bevy::prelude::Entity, mind: &MindGraph) -> Vec<Triple> {
    let direct = mind.query(Some(&Node::Entity(entity)), Some(Predicate::Produces), None);
    if !direct.is_empty() {
        return direct
            .into_iter()
            .map(|t| Triple::new(Node::Self_, Predicate::Contains, t.object.clone()))
            .collect();
    }

    let type_triples = mind.query(Some(&Node::Entity(entity)), Some(Predicate::IsA), None);
    type_triples
        .iter()
        .flat_map(|type_triple| {
            if let Value::Concept(concept) = type_triple.object {
                mind.query(
                    Some(&Node::Concept(concept)),
                    Some(Predicate::Produces),
                    None,
                )
            } else {
                vec![]
            }
        })
        .map(|t| Triple::new(Node::Self_, Predicate::Contains, t.object.clone()))
        .collect()
}

/// True when the entity produces at least one item classified as Food or Resource.
pub(crate) fn prey_produces_useful_item(entity: bevy::prelude::Entity, mind: &MindGraph) -> bool {
    prey_yield_effects(entity, mind).iter().any(|triple| {
        if let Value::Item(concept, _) = &triple.object {
            mind.is_a(&Node::Concept(*concept), Concept::Food)
                || mind.is_a(&Node::Concept(*concept), Concept::Resource)
        } else {
            false
        }
    })
}
