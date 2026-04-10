use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects, SpawnRequest,
    TargetCandidate, TargetSource,
};
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Triple, Value};
use crate::constants::actions::attack::{BASE_COST, DURATION_TICKS, ENERGY_PER_SEC};

pub struct AttackAction;

impl Action for AttackAction {
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
        // it for punches, grapples, and held weapons.
        const CHANNELS: &[ChannelUsage] = &[
            ChannelUsage::new(Channel::Manipulation, 0.9),
            ChannelUsage::new(Channel::Locomotion, 0.6),
            ChannelUsage::new(Channel::FullBody, 0.7),
        ];
        CHANNELS
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

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            energy_per_sec: ENERGY_PER_SEC,
            ..Default::default()
        }
    }

    fn on_complete(&self, ctx: &mut CompletionContext) {
        apply_hunt_kill(ctx);
    }
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

/// Hunting kill: deposit the killer's "first cut" into their inventory and
/// queue an in-place Becomes transformation that morphs the prey into a
/// Corpse holding extra meat for scavengers. The killer's direct deposit
/// keeps the planner's `Walk → Attack → Eat` chain working as a single plan;
/// the corpse exists as the world artifact future agents can Harvest.
///
/// Skipped for non-prey targets so the emotional brain's reactive
/// "I'm angry, hit the wolf" attacks don't generate meat or corpses.
/// Shared between Attack and Bite.
pub(crate) fn apply_hunt_kill(ctx: &mut CompletionContext) {
    let Some(target) = ctx.target_entity else {
        return;
    };

    if !ctx.mind.has_trait(&Node::Entity(target), Concept::Prey) {
        return;
    }

    let yields = prey_yield_effects(target, ctx.mind);
    if yields.is_empty() {
        return;
    }

    for triple in &yields {
        if let Value::Item(concept, qty) = &triple.object {
            ctx.inventory.add(*concept, *qty);
        }
    }

    ctx.spawn_requests.push(SpawnRequest::BecomesAttach {
        entity: target,
        target: Concept::Corpse,
        mode: crate::world::becomes::BecomesMode::InPlace,
    });
}
