//! Generic interpreter for [`ActionDefinition`].
//!
//! `GenericAction` is the single [`Action`] impl for every action in the
//! game — what used to be 24 per-action trait impls is now one interpreter
//! that walks the static [`ActionDefinition`] data and applies the common
//! rules for planning, gating, and completion. Irreducibly custom bits
//! (metabolism-gated inventory removal in Eat, skill-scaled transfer in
//! Harvest, staleness-weighted picker in Explore) live as named helper
//! functions referenced through [`Hooks`].

use super::action::drink::is_adjacent_to_water;
use super::channel::{ChannelUsage, Posture};
use super::definition::{
    ActionDefinition, CompletionPredicate, EffectTemplate, Gate, Pattern, PlanValidity, RuntimeOp,
    SatiationGate, TargetEffects,
};
use super::motor::Behavior;
use super::registry::{
    Action, ActionContext, ActionKind, CompletionContext, LegCompleteContext, LegResult,
    RuntimeEffects, SpawnRequest, TargetCandidate, TargetSource,
};
use super::types::ActionType;
use crate::agent::body::need::NeedKind;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Quantity, Triple, Value};
use crate::world::spatial_index::world_pos_to_tile;

/// Wraps a static [`ActionDefinition`] and implements the [`Action`] trait by
/// interpreting the definition's data. Registered with the [`ActionRegistry`]
/// by [`super::registry::ActionRegistry::register_def`].
pub struct GenericAction {
    pub def: &'static ActionDefinition,
}

impl GenericAction {
    pub const fn new(def: &'static ActionDefinition) -> Self {
        Self { def }
    }
}

// ============================================================================
// COMPILATION HELPERS — static data → runtime Triples / TriplePatterns
// ============================================================================

fn compile_pattern(pat: &Pattern) -> TriplePattern {
    match pat {
        Pattern::SelfContains { concept, quantity } => TriplePattern::new(
            Some(Node::Self_),
            Some(Predicate::Contains),
            Some(Value::Item(*concept, *quantity)),
        ),
        Pattern::SelfContainsFood => TriplePattern::self_contains_food(),
        Pattern::SelfContainsAny => TriplePattern::self_contains(),
        Pattern::SelfNearConcept(c) => TriplePattern::new(
            Some(Node::Self_),
            Some(Predicate::Near),
            Some(Value::Concept(*c)),
        ),
    }
}

fn compile_effect(eff: &EffectTemplate) -> Triple {
    match eff {
        EffectTemplate::SelfNeedExact { predicate, value } => Triple::new(
            Node::Self_,
            *predicate,
            Value::Quantity(Quantity::Exact(*value)),
        ),
        EffectTemplate::SelfNearConcept(c) => {
            Triple::new(Node::Self_, Predicate::Near, Value::Concept(*c))
        }
        EffectTemplate::SelfHasTrait(c) => {
            Triple::new(Node::Self_, Predicate::HasTrait, Value::Concept(*c))
        }
        EffectTemplate::SelfContains { concept, quantity } => Triple::new(
            Node::Self_,
            Predicate::Contains,
            Value::Item(*concept, *quantity),
        ),
    }
}

fn compile_effects(effects: &[EffectTemplate]) -> Vec<Triple> {
    effects.iter().map(compile_effect).collect()
}

// ============================================================================
// TARGET-AWARE EFFECT COMPILATION
// ============================================================================

fn effects_for_target(
    target_effects: TargetEffects,
    static_effects: &[EffectTemplate],
    target: &TargetCandidate,
    mind: &MindGraph,
) -> Vec<Triple> {
    match target_effects {
        TargetEffects::Static => compile_effects(static_effects),
        TargetEffects::FromTargetProduces => {
            from_target_produces(target, mind).unwrap_or_else(|| compile_effects(static_effects))
        }
        TargetEffects::FromTargetBecomes => from_target_becomes(target, mind),
        TargetEffects::FromTargetBecomesRequirements => {
            from_target_becomes_requirements(target, mind)
        }
        TargetEffects::FromTargetContains => {
            from_target_contains(target, mind).unwrap_or_else(|| compile_effects(static_effects))
        }
    }
}

fn from_target_produces(target: &TargetCandidate, mind: &MindGraph) -> Option<Vec<Triple>> {
    let entity = target.as_entity()?;
    let direct = mind.query(Some(&Node::Entity(entity)), Some(Predicate::Produces), None);
    if !direct.is_empty() {
        return Some(
            direct
                .into_iter()
                .map(|t| Triple::new(Node::Self_, Predicate::Contains, t.object.clone()))
                .collect(),
        );
    }
    let via_type: Vec<Triple> = mind
        .query(Some(&Node::Entity(entity)), Some(Predicate::IsA), None)
        .into_iter()
        .flat_map(|t| {
            if let Value::Concept(c) = t.object {
                mind.query(Some(&Node::Concept(c)), Some(Predicate::Produces), None)
            } else {
                vec![]
            }
        })
        .map(|t| Triple::new(Node::Self_, Predicate::Contains, t.object.clone()))
        .collect();
    if via_type.is_empty() {
        None
    } else {
        Some(via_type)
    }
}

fn from_target_becomes(target: &TargetCandidate, mind: &MindGraph) -> Vec<Triple> {
    let Some(entity) = target.as_entity() else {
        return vec![];
    };
    mind.query(Some(&Node::Entity(entity)), Some(Predicate::Becomes), None)
        .into_iter()
        .filter_map(|t| {
            if let Value::Concept(c) = t.object {
                Some(Triple::new(Node::Self_, Predicate::Near, Value::Concept(c)))
            } else {
                None
            }
        })
        .collect()
}

fn from_target_becomes_requirements(target: &TargetCandidate, mind: &MindGraph) -> Vec<Triple> {
    let Some(entity) = target.as_entity() else {
        return vec![];
    };
    let recipes: Vec<Concept> = mind
        .query(Some(&Node::Entity(entity)), Some(Predicate::Becomes), None)
        .into_iter()
        .filter_map(|t| match t.object {
            Value::Concept(c) => Some(c),
            _ => None,
        })
        .collect();

    let mut effects = Vec::new();
    for recipe in recipes {
        for req in mind.query(
            Some(&Node::Concept(recipe)),
            Some(Predicate::Requires),
            None,
        ) {
            if matches!(req.object, Value::Item(_, _)) {
                effects.push(Triple::new(
                    Node::Entity(entity),
                    Predicate::Contains,
                    req.object.clone(),
                ));
            }
        }
    }
    effects
}

fn from_target_contains(target: &TargetCandidate, mind: &MindGraph) -> Option<Vec<Triple>> {
    let entity = target.as_entity()?;
    let effects: Vec<Triple> = mind
        .query(Some(&Node::Entity(entity)), Some(Predicate::Contains), None)
        .into_iter()
        .filter_map(|t| {
            if let Value::Item(_, qty) = t.object
                && qty > 0
            {
                Some(Triple::new(
                    Node::Self_,
                    Predicate::Contains,
                    t.object.clone(),
                ))
            } else {
                None
            }
        })
        .collect();
    if effects.is_empty() {
        None
    } else {
        Some(effects)
    }
}

// ============================================================================
// GATES — runtime can_start checks
// ============================================================================

fn check_gate(gate: &Gate, ctx: &ActionContext) -> Result<(), FailureReason> {
    match gate {
        Gate::InventoryHasQuantity { concept, quantity } => {
            if ctx.inventory.count(*concept) >= *quantity {
                Ok(())
            } else {
                Err(FailureReason::MissingMaterials)
            }
        }
        Gate::InventoryHasFood => {
            if ctx
                .inventory
                .all_items()
                .any(|item| ctx.mind.is_a(&Node::Concept(item.concept), Concept::Food))
            {
                Ok(())
            } else {
                Err(FailureReason::NoEdibleFood)
            }
        }
        Gate::InventoryNonEmpty => {
            if ctx.inventory.all_items().next().is_some() {
                Ok(())
            } else {
                Err(FailureReason::MissingMaterials)
            }
        }
        Gate::TargetEntity(reason) => {
            if ctx.target_entity.is_some() {
                Ok(())
            } else {
                Err(reason.clone())
            }
        }
        Gate::AdjacentToWater => {
            if is_adjacent_to_water(ctx.agent_position, ctx.world_map) {
                Ok(())
            } else {
                Err(FailureReason::NoWaterNearby)
            }
        }
        Gate::NearHeatEmitter => {
            if is_near_trait(ctx.mind, ctx.world_positions, Concept::HeatEmitting) {
                Ok(())
            } else {
                Err(FailureReason::TargetGone)
            }
        }
        Gate::NearShelterProvider => {
            if is_near_trait(ctx.mind, ctx.world_positions, Concept::ShelterProviding) {
                Ok(())
            } else {
                Err(FailureReason::TargetGone)
            }
        }
        Gate::OnGrassTile => {
            if matches!(
                ctx.world_map.tile_at(ctx.agent_position),
                Some(crate::world::map::TileType::Grass)
            ) {
                Ok(())
            } else {
                Err(FailureReason::NoEdibleFood)
            }
        }
        Gate::Nighttime {
            start_hour,
            end_hour,
        } => {
            if is_nighttime(ctx.current_tick, *start_hour, *end_hour) {
                Ok(())
            } else {
                Err(FailureReason::Interrupted)
            }
        }
        Gate::MoodAtLeast(threshold) => {
            let mood = ctx.emotional.map(|e| e.current_mood).unwrap_or(0.0);
            if mood >= *threshold {
                Ok(())
            } else {
                Err(FailureReason::Interrupted)
            }
        }
        Gate::CompanionshipAtLeast(threshold) => {
            let level = ctx.drives.map(|d| d.companionship.value).unwrap_or(0.0);
            if level >= *threshold {
                Ok(())
            } else {
                Err(FailureReason::Interrupted)
            }
        }
        Gate::TargetIsInjured => {
            let Some(target) = ctx.target_entity else {
                return Err(FailureReason::NoTarget);
            };
            if ctx.mind.has_trait(&Node::Entity(target), Concept::Lame) {
                Ok(())
            } else {
                Err(FailureReason::TargetGone)
            }
        }
        Gate::KnowsRecentDeath => {
            if knows_any_death(ctx.mind) {
                Ok(())
            } else {
                Err(FailureReason::Interrupted)
            }
        }
        Gate::TargetAffectionAtLeast(threshold) => {
            let Some(target) = ctx.target_entity else {
                return Err(FailureReason::NoTarget);
            };
            let affection = ctx
                .mind
                .get(&Node::Entity(target), Predicate::Affection)
                .and_then(|v| match v {
                    Value::Quantity(Quantity::Exact(f)) => Some(*f),
                    _ => None,
                })
                .unwrap_or(0.0);
            if affection >= *threshold {
                Ok(())
            } else {
                Err(FailureReason::Interrupted)
            }
        }
        Gate::TileReachable => {
            let Some(pos) = ctx.target_position else {
                return Ok(());
            };
            let v = world_pos_to_tile(pos);
            let tile = (v.x, v.y);
            if ctx.unreachable_tiles.contains(&tile) {
                Err(FailureReason::PathBlocked { target_tile: tile })
            } else {
                Ok(())
            }
        }
        Gate::TargetNotEngaged(reason) => {
            let Some(target) = ctx.target_entity else {
                return Ok(());
            };
            let busy = !ctx
                .mind
                .query(
                    Some(&Node::Entity(target)),
                    Some(Predicate::EngagedWith),
                    None,
                )
                .is_empty();
            if busy { Err(reason.clone()) } else { Ok(()) }
        }
    }
}

/// Hour-of-day predicate over the cyclic `[start, end)` window across midnight.
/// Inputs in 0..=23.
fn is_nighttime(tick: u64, start_hour: u32, end_hour: u32) -> bool {
    use crate::core::time::GameTime;
    let total_ticks = tick + GameTime::INITIAL_TICK_OFFSET;
    let total_hours = total_ticks / GameTime::TICKS_PER_HOUR;
    let hour = (total_hours % GameTime::HOURS_PER_DAY) as u32;
    if start_hour < end_hour {
        (start_hour..end_hour).contains(&hour)
    } else {
        hour >= start_hour || hour < end_hour
    }
}

/// True when the agent's MindGraph carries any
/// `(?event, Action, Death)` triple — a recent death the belief updater
/// has translated from a `SimEvent::Death` into the agent's episodic memory.
fn knows_any_death(mind: &MindGraph) -> bool {
    !mind
        .query(
            None,
            Some(Predicate::Action),
            Some(&Value::Concept(Concept::Death)),
        )
        .is_empty()
}

/// Runtime check mirroring the planner's `(Self, Near, $trait)` relation:
/// true when an entity carrying `trait_concept` sits on self's tile.
/// Mobile entities are checked against the agent's MindGraph; static
/// entities are checked against the world snapshot (#756).
fn is_near_trait(
    mind: &MindGraph,
    world_positions: &crate::world::entity_positions::WorldEntityPositions,
    trait_concept: Concept,
) -> bool {
    let Some(Value::Tile(self_tile)) = mind.get(&Node::Self_, Predicate::LocatedAt).cloned() else {
        return false;
    };
    let mobile_match = mind
        .query(
            None,
            Some(Predicate::LocatedAt),
            Some(&Value::Tile(self_tile)),
        )
        .iter()
        .any(|t| matches!(t.subject, Node::Entity(_)) && mind.has_trait(&t.subject, trait_concept));
    if mobile_match {
        return true;
    }
    world_positions.entities_at_tile(self_tile).any(|entity| {
        world_positions
            .entry(entity)
            .is_some_and(|loc| mind.ontology.has_trait(loc.concept, trait_concept))
    })
}

// ============================================================================
// SATIATION
// ============================================================================

fn evaluate_satiation(
    gate: SatiationGate,
    physical: Option<&PhysicalNeeds>,
    inventory: Option<&ItemSlots>,
) -> Option<(NeedKind, f32)> {
    let physical = physical?;
    let need = gate.need_kind();
    match gate {
        SatiationGate::EatStomach => {
            let metabolism = &physical.metabolism;
            if let Some(inv) = inventory
                && let Some(macros) = inv
                    .all_items()
                    .find_map(|t| crate::agent::body::metabolism::food_macros(t.concept))
                && !metabolism.would_fit(macros)
            {
                return Some((need, 1.0));
            }
            Some((need, metabolism.stomach_fraction()))
        }
        SatiationGate::HungerStomach => Some((need, physical.metabolism.stomach_fraction())),
        SatiationGate::HydrationValue => Some((need, physical.hydration.value)),
        SatiationGate::WarmthValue => Some((need, physical.warmth.value)),
        SatiationGate::RestQualityValue => Some((need, physical.rest_quality.value)),
        SatiationGate::WakefulnessValue => Some((need, physical.wakefulness.value)),
        SatiationGate::StaminaAerobic => Some((need, physical.stamina.aerobic_fraction())),
    }
}

// ============================================================================
// PLAN VALIDITY
// ============================================================================

fn evaluate_plan_validity(
    validity: PlanValidity,
    target: &TargetCandidate,
    mind: &MindGraph,
) -> bool {
    match validity {
        PlanValidity::Always => true,
        PlanValidity::TargetHasBecomes => {
            let Some(entity) = target.as_entity() else {
                return false;
            };
            !mind
                .query(Some(&Node::Entity(entity)), Some(Predicate::Becomes), None)
                .is_empty()
        }
        PlanValidity::TargetProducesFoodOrResource => target_produces_useful(target, mind),
        PlanValidity::TargetContainsAny => {
            let Some(entity) = target.as_entity() else {
                return false;
            };
            mind.query(Some(&Node::Entity(entity)), Some(Predicate::Contains), None)
                .iter()
                .any(|t| matches!(t.object, Value::Item(_, qty) if qty > 0))
        }
        PlanValidity::TargetContainsEdible => {
            let Some(entity) = target.as_entity() else {
                return false;
            };
            mind.query(Some(&Node::Entity(entity)), Some(Predicate::Contains), None)
                .iter()
                .any(|t| match &t.object {
                    Value::Item(concept, qty) => {
                        *qty > 0 && mind.is_a(&Node::Concept(*concept), Concept::Food)
                    }
                    _ => false,
                })
        }
        PlanValidity::RecipeKnown(concept) => !mind
            .query(
                Some(&Node::Concept(concept)),
                Some(Predicate::Requires),
                None,
            )
            .is_empty(),
    }
}

fn target_produces_useful(target: &TargetCandidate, mind: &MindGraph) -> bool {
    let Some(entity) = target.as_entity() else {
        return false;
    };
    if mind.is_known_empty(entity) {
        return false;
    }

    let produced: Vec<Value> = collect_produced(entity, mind);
    if produced.is_empty() {
        return false;
    }
    produced.iter().any(|value| {
        if let Value::Item(concept, _) = value {
            mind.is_a(&Node::Concept(*concept), Concept::Food)
                || mind.is_a(&Node::Concept(*concept), Concept::Resource)
        } else {
            false
        }
    })
}

fn collect_produced(entity: bevy::prelude::Entity, mind: &MindGraph) -> Vec<Value> {
    let mut produced: Vec<Value> = mind
        .query(Some(&Node::Entity(entity)), Some(Predicate::Produces), None)
        .into_iter()
        .map(|t| t.object.clone())
        .collect();
    if !produced.is_empty() {
        return produced;
    }
    for t in mind.query(Some(&Node::Entity(entity)), Some(Predicate::IsA), None) {
        if let Value::Concept(concept) = t.object {
            produced.extend(
                mind.query(
                    Some(&Node::Concept(concept)),
                    Some(Predicate::Produces),
                    None,
                )
                .into_iter()
                .map(|t| t.object.clone()),
            );
        }
    }
    produced
}

// ============================================================================
// RUNTIME OPS — on_complete execution
// ============================================================================

fn apply_op(op: &RuntimeOp, ctx: &mut CompletionContext) {
    match op {
        RuntimeOp::RemoveFromInventory { concept, quantity } => {
            ctx.inventory.remove(*concept, *quantity);
        }
        RuntimeOp::TopUpHydration(amount) => {
            ctx.physical.hydration.top_up(*amount);
        }
        RuntimeOp::TopUpWarmth(amount) => {
            ctx.physical.warmth.top_up(*amount);
        }
        RuntimeOp::AdjustAerobic(amount) => {
            ctx.physical.stamina.adjust_aerobic(*amount);
        }
        RuntimeOp::SpawnSite {
            target,
            requirements,
            initial_items,
            labor_required,
        } => {
            ctx.spawn_requests.push(SpawnRequest::Site {
                target: *target,
                position: ctx.agent_position,
                requirements: requirements.to_vec(),
                initial_items: initial_items.to_vec(),
                labor_required: *labor_required,
            });
        }
    }
}

// ============================================================================
// ACTION IMPL — the single interpreter
// ============================================================================

impl Action for GenericAction {
    fn action_type(&self) -> ActionType {
        self.def.action_type
    }

    fn name(&self) -> &'static str {
        self.def.action_type.name()
    }

    fn default_behavior(&self) -> Behavior {
        Behavior::new(
            self.def.primitive,
            self.def.target_selector.clone(),
            self.def.intensity.clone(),
            self.def.intent,
        )
    }

    fn kind(&self) -> ActionKind {
        self.def.kind.clone()
    }

    fn cost(&self) -> f32 {
        self.def.base_cost
    }

    fn target_source(&self) -> TargetSource {
        self.def.target_source.clone()
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        self.def.body_channels
    }

    fn posture(&self) -> Option<Posture> {
        self.def.posture
    }

    fn interruptible(&self) -> bool {
        self.def.interruptible
    }

    fn start_log(&self) -> Option<&'static str> {
        self.def.start_log
    }

    fn complete_log(&self) -> Option<&'static str> {
        self.def.complete_log
    }

    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            stomach_carbs_per_sec: self.def.stomach_carbs_per_sec,
            joy_per_sec: self.def.joy_per_sec,
        }
    }

    fn preconditions(&self) -> Vec<TriplePattern> {
        self.def.preconditions.iter().map(compile_pattern).collect()
    }

    fn plan_effects(&self) -> Vec<Triple> {
        compile_effects(self.def.plan_effects)
    }

    fn plan_consumes(&self) -> Vec<TriplePattern> {
        self.def.plan_consumes.iter().map(compile_pattern).collect()
    }

    fn plan_effects_for_target(&self, target: &TargetCandidate, mind: &MindGraph) -> Vec<Triple> {
        if let Some(custom) = self.def.hooks.plan_effects_for_target {
            return custom(target, mind);
        }
        effects_for_target(self.def.target_effects, self.def.plan_effects, target, mind)
    }

    fn target_preconditions(
        &self,
        target: &TargetCandidate,
        mind: &MindGraph,
    ) -> Vec<TriplePattern> {
        self.def
            .hooks
            .target_preconditions
            .map(|f| f(target, mind))
            .unwrap_or_default()
    }

    fn target_consumes(&self, target: &TargetCandidate, mind: &MindGraph) -> Vec<TriplePattern> {
        self.def
            .hooks
            .target_consumes
            .map(|f| f(target, mind))
            .unwrap_or_default()
    }

    fn is_plan_valid(&self, target: &TargetCandidate, mind: &MindGraph) -> bool {
        evaluate_plan_validity(self.def.plan_validity, target, mind)
    }

    fn satiation(
        &self,
        physical: Option<&PhysicalNeeds>,
        inventory: Option<&ItemSlots>,
    ) -> Option<(NeedKind, f32)> {
        self.def
            .satiation
            .and_then(|g| evaluate_satiation(g, physical, inventory))
    }

    fn eligible_diets(&self) -> &'static [crate::agent::body::species::Diet] {
        use crate::agent::body::species::Diet;
        // Graze is the only action with a diet restriction today: previously
        // enforced implicitly by gating perception of grass tiles to
        // herbivores. Now that perception is gone, declare the restriction
        // here so the rational brain skips Graze for omnivores/carnivores.
        match self.def.action_type {
            ActionType::Graze => &[Diet::Herbivore],
            _ => &[],
        }
    }

    fn should_complete(&self, physical: &PhysicalNeeds) -> bool {
        match self.def.completion {
            CompletionPredicate::Never => false,
            CompletionPredicate::AerobicAtLeast(threshold) => {
                physical.stamina.aerobic_fraction() >= threshold
            }
            CompletionPredicate::WarmthAtLeast(threshold) => physical.warmth.value >= threshold,
            CompletionPredicate::RestQualityAtLeast(threshold) => {
                physical.rest_quality.value >= threshold
            }
        }
    }

    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if let Some(custom) = self.def.hooks.can_start {
            return custom(ctx);
        }
        for gate in self.def.gates {
            check_gate(gate, ctx)?;
        }
        Ok(())
    }

    fn on_complete(&self, ctx: &mut CompletionContext) {
        if let Some(custom) = self.def.hooks.on_complete {
            custom(ctx);
            return;
        }
        for op in self.def.on_complete_ops {
            apply_op(op, ctx);
        }
    }

    fn on_leg_complete(&self, ctx: &mut LegCompleteContext) -> LegResult {
        if let Some(custom) = self.def.hooks.on_leg_complete {
            custom(ctx)
        } else {
            LegResult::Complete
        }
    }

    fn location_preference(
        &self,
    ) -> Option<
        fn(&crate::agent::actions::definition::PreferenceContext, &[bevy::math::IVec2]) -> Vec<f32>,
    > {
        self.def.hooks.location_preference
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::action::{EAT_DEF, INITIATE_CONVERSATION_DEF, WALK_DEF};
    use crate::agent::actions::registry::Action;
    use crate::agent::body::needs::PhysicalNeeds;
    use crate::agent::item_slots::ItemSlots;
    use crate::agent::mind::knowledge::{
        Concept, Metadata, MindGraph, Node, Predicate, Triple, Value, setup_ontology,
    };
    use crate::world::map::{TILE_SIZE, WorldMap};
    use bevy::math::Vec2;
    use bevy::prelude::Entity;

    fn mind() -> MindGraph {
        MindGraph::new(setup_ontology())
    }

    fn world_map() -> WorldMap {
        WorldMap::new(32, 32)
    }

    #[allow(clippy::too_many_arguments)]
    fn ctx<'a>(
        inventory: &'a ItemSlots,
        mind: &'a MindGraph,
        world_map: &'a WorldMap,
        world_positions: &'a crate::world::entity_positions::WorldEntityPositions,
        physical: &'a PhysicalNeeds,
        target_entity: Option<Entity>,
        target_position: Option<Vec2>,
        unreachable_tiles: &'a [(i32, i32)],
    ) -> ActionContext<'a> {
        ActionContext {
            inventory,
            mind,
            world_map,
            world_positions,
            target_entity,
            target_position,
            agent_position: Vec2::ZERO,
            physical: Some(physical),
            drives: None,
            emotional: None,
            current_tick: 0,
            unreachable_tiles,
        }
    }

    #[test]
    fn eat_is_infeasible_without_food_in_inventory() {
        let inventory = ItemSlots::agent_carry();
        let mind = mind();
        let map = world_map();
        let physical = PhysicalNeeds::default();
        let positions = crate::world::entity_positions::WorldEntityPositions::default();
        let ctx = ctx(
            &inventory,
            &mind,
            &map,
            &positions,
            &physical,
            None,
            None,
            &[],
        );
        let eat = GenericAction::new(&EAT_DEF);
        assert!(
            !eat.is_feasible(&ctx),
            "Eat must not propose when ItemSlots has no food"
        );
    }

    #[test]
    fn eat_is_feasible_with_food_in_inventory() {
        let mut inventory = ItemSlots::agent_carry();
        inventory.add(Concept::Apple, 1);
        let mind = mind();
        let map = world_map();
        let physical = PhysicalNeeds::default();
        let positions = crate::world::entity_positions::WorldEntityPositions::default();
        let ctx = ctx(
            &inventory,
            &mind,
            &map,
            &positions,
            &physical,
            None,
            None,
            &[],
        );
        let eat = GenericAction::new(&EAT_DEF);
        assert!(
            eat.is_feasible(&ctx),
            "Eat must propose when ItemSlots holds an Apple"
        );
    }

    #[test]
    fn walk_is_infeasible_to_unreachable_tile() {
        let inventory = ItemSlots::agent_carry();
        let mind = mind();
        let map = world_map();
        let physical = PhysicalNeeds::default();
        let target_pos = Some(Vec2::new(5.0 * TILE_SIZE, 5.0 * TILE_SIZE));
        let unreachable = [(5, 5)];
        let positions = crate::world::entity_positions::WorldEntityPositions::default();
        let ctx = ctx(
            &inventory,
            &mind,
            &map,
            &positions,
            &physical,
            None,
            target_pos,
            &unreachable,
        );
        let walk = GenericAction::new(&WALK_DEF);
        assert!(
            !walk.is_feasible(&ctx),
            "Walk must not propose toward a tile in the Unreachable belief"
        );
    }

    #[test]
    fn walk_is_feasible_to_reachable_tile() {
        let inventory = ItemSlots::agent_carry();
        let mind = mind();
        let map = world_map();
        let physical = PhysicalNeeds::default();
        let target_pos = Some(Vec2::new(5.0 * TILE_SIZE, 5.0 * TILE_SIZE));
        let positions = crate::world::entity_positions::WorldEntityPositions::default();
        let ctx = ctx(
            &inventory,
            &mind,
            &map,
            &positions,
            &physical,
            None,
            target_pos,
            &[],
        );
        let walk = GenericAction::new(&WALK_DEF);
        assert!(
            walk.is_feasible(&ctx),
            "Walk must propose toward a tile with no Unreachable belief"
        );
    }

    #[test]
    fn initiate_conversation_is_infeasible_when_target_engaged() {
        let inventory = ItemSlots::agent_carry();
        let mut mind = mind();
        let target = Entity::from_bits(11);
        let other = Entity::from_bits(12);
        mind.assert(Triple::with_meta(
            Node::Entity(target),
            Predicate::EngagedWith,
            Value::Entity(other),
            Metadata::perception(0),
        ));
        let map = world_map();
        let physical = PhysicalNeeds::default();
        let positions = crate::world::entity_positions::WorldEntityPositions::default();
        let ctx = ctx(
            &inventory,
            &mind,
            &map,
            &positions,
            &physical,
            Some(target),
            None,
            &[],
        );
        let initiate = GenericAction::new(&INITIATE_CONVERSATION_DEF);
        assert!(
            !initiate.is_feasible(&ctx),
            "InitiateConversation must not propose toward a target the agent perceives as engaged"
        );
    }

    #[test]
    fn initiate_conversation_is_feasible_when_target_free() {
        let inventory = ItemSlots::agent_carry();
        let mind = mind();
        let map = world_map();
        let physical = PhysicalNeeds::default();
        let target = Entity::from_bits(11);
        let positions = crate::world::entity_positions::WorldEntityPositions::default();
        let ctx = ctx(
            &inventory,
            &mind,
            &map,
            &positions,
            &physical,
            Some(target),
            None,
            &[],
        );
        let initiate = GenericAction::new(&INITIATE_CONVERSATION_DEF);
        assert!(
            initiate.is_feasible(&ctx),
            "InitiateConversation must propose toward a target with no EngagedWith belief"
        );
    }
}
