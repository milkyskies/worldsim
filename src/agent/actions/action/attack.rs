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
        // Attack-as-hunt: enumerate every entity the agent believes is Prey.
        // The emotional brain bypasses enumeration entirely when proposing
        // Attack on a perceived threat, so this trait gate only governs the
        // rational planner's hunting chain.
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

    /// Plan-time view: attacking prey yields whatever that prey produces.
    /// Mirrors `HarvestAction::plan_effects_for_target`: walks the entity's
    /// `Produces` triples (direct first, then indirect via `IsA`) and turns
    /// each into a `Self_, Contains, Item(...)` projection so the regressive
    /// planner can chain "I want meat → Attack(deer)".
    fn plan_effects_for_target(&self, target: &TargetCandidate, mind: &MindGraph) -> Vec<Triple> {
        let Some(entity) = target.as_entity() else {
            return self.plan_effects();
        };
        prey_yield_effects(entity, mind)
    }

    /// Only valid when the target produces something IsA Food or Resource.
    /// The trait-based target enumeration finds every Prey entity, so this
    /// gate keeps the planner from forming hunts against prey it doesn't
    /// know how to butcher (no `Produces` knowledge).
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

/// Compute `(Self_, Contains, Item(X, n))` projections for any item the
/// `entity` is known to produce, walking the same direct/indirect path as
/// `HarvestAction::plan_effects_for_target`.
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

/// True when the entity produces at least one item that the agent's mind
/// classifies as Food or Resource. Walks the same direct/indirect path as
/// `prey_yield_effects` and asks the ontology to classify each yield.
pub(crate) fn prey_produces_useful_item(entity: bevy::prelude::Entity, mind: &MindGraph) -> bool {
    let mut produced = mind.query(Some(&Node::Entity(entity)), Some(Predicate::Produces), None);

    if produced.is_empty() {
        let type_triples = mind.query(Some(&Node::Entity(entity)), Some(Predicate::IsA), None);
        for type_triple in type_triples {
            if let Value::Concept(concept) = type_triple.object {
                produced.extend(mind.query(
                    Some(&Node::Concept(concept)),
                    Some(Predicate::Produces),
                    None,
                ));
            }
        }
    }

    for triple in produced {
        if let Value::Item(concept, _) = triple.object
            && (mind.is_a(&Node::Concept(concept), Concept::Food)
                || mind.is_a(&Node::Concept(concept), Concept::Resource))
        {
            return true;
        }
    }
    false
}

/// Apply the hunting kill: if the agent's beliefs say the target is Prey
/// that produces edible items, drop one of each yield into the hunter's
/// inventory and queue a `Becomes` transformation so the substrate replaces
/// the slain prey with a meat-drop entity on the next tick.
///
/// Skipped entirely for non-prey targets so the emotional brain's "I'm
/// angry, punch the wolf" attacks don't accidentally generate meat.
///
/// Shared between `AttackAction` (humans) and `BiteAction` (wolves) so a
/// rational hunter and a hungry wolf converge on identical post-kill state.
pub(crate) fn apply_hunt_kill(ctx: &mut CompletionContext) {
    let Some(target) = ctx.target_entity else {
        return;
    };

    if !ctx.mind.has_trait(&Node::Entity(target), Concept::Prey) {
        return;
    }

    // Yield each item the agent's mind knows the target produces. The same
    // direct/indirect query the planner used to project plan effects.
    let yields = prey_yield_effects(target, ctx.mind);
    if yields.is_empty() {
        return;
    }

    let mut yield_concept = None;
    for triple in &yields {
        if let Value::Item(concept, qty) = &triple.object {
            ctx.inventory.add(*concept, *qty);
            yield_concept.get_or_insert(*concept);
        }
    }

    let drop_concept = yield_concept.unwrap_or(Concept::Meat);
    ctx.spawn_requests.push(SpawnRequest::BecomesAttach {
        entity: target,
        target: drop_concept,
    });
}
