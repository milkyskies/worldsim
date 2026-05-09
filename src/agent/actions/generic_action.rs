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
            if is_near_trait(ctx.mind, Concept::HeatEmitting) {
                Ok(())
            } else {
                Err(FailureReason::TargetGone)
            }
        }
        Gate::NearShelterProvider => {
            if is_near_trait(ctx.mind, Concept::ShelterProviding) {
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
    }
}

/// Runtime check mirroring the planner's `(Self, Near, $trait)` relation:
/// true when a known entity carrying `trait_concept` sits on self's tile.
fn is_near_trait(mind: &MindGraph, trait_concept: Concept) -> bool {
    let Some(Value::Tile(self_tile)) = mind.get(&Node::Self_, Predicate::LocatedAt).cloned() else {
        return false;
    };
    mind.query(
        None,
        Some(Predicate::LocatedAt),
        Some(&Value::Tile(self_tile)),
    )
    .iter()
    .any(|t| matches!(t.subject, Node::Entity(_)) && mind.has_trait(&t.subject, trait_concept))
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
