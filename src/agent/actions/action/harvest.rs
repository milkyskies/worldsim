//! Harvest action - gather resources from targets.

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::{Channel, ChannelUsage, Posture};
use crate::agent::actions::registry::{
    Action, ActionContext, ActionKind, CompletionContext, RuntimeEffects, TargetCandidate,
    TargetSource,
};
use crate::agent::brains::thinking::TriplePattern;
use crate::agent::events::FailureReason;
use crate::agent::item_slots::{Thing, perishable_decay_rate};
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Triple, Value};
use crate::agent::skills::SkillKind;
use crate::constants::actions::harvest::{DURATION_TICKS, GLUCOSE_DRAIN_PER_SEC, STAMINA_PER_SEC};

pub struct HarvestAction;

impl Action for HarvestAction {
    fn action_type(&self) -> ActionType {
        ActionType::Harvest
    }

    fn name(&self) -> &'static str {
        "Harvest"
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Timed {
            duration_ticks: DURATION_TICKS,
        }
    }

    // Planning: After harvesting, we have a generic placeholder item.
    // The real per-target effect (apple, wood, stone, ...) comes from
    // `plan_effects_for_target` which queries the target's `Produces` triples.
    fn plan_effects(&self) -> Vec<Triple> {
        vec![Triple::new(
            Node::Self_,
            Predicate::Contains,
            Value::Item(Concept::Apple, 1),
        )]
    }

    fn cost(&self) -> f32 {
        2.0
    }

    fn target_source(&self) -> TargetSource {
        TargetSource::EntityAffordance
    }

    fn target_preconditions(
        &self,
        target: &TargetCandidate,
        _mind: &MindGraph,
    ) -> Vec<TriplePattern> {
        match target.as_entity() {
            Some(entity) => vec![TriplePattern::entity_contains(entity)],
            None => vec![],
        }
    }

    /// Per-target consumed pattern: harvesting removes items from the target
    /// entity's stock so two plan steps can't double-count the same stack.
    fn target_consumes(&self, target: &TargetCandidate, _mind: &MindGraph) -> Vec<TriplePattern> {
        match target.as_entity() {
            Some(entity) => vec![TriplePattern::entity_contains(entity)],
            None => vec![],
        }
    }

    fn body_channels(&self) -> &'static [ChannelUsage] {
        // Hands only — the legs are planted and the posture gate handles
        // the "you can't harvest while walking" mutex.
        const CHANNELS: &[ChannelUsage] = &[ChannelUsage::new(Channel::Manipulation, 0.9)];
        CHANNELS
    }

    fn posture(&self) -> Option<Posture> {
        Some(Posture::Stationary)
    }

    // Execution: Must have a target entity
    fn can_start(&self, ctx: &ActionContext) -> Result<(), FailureReason> {
        if ctx.target_entity.is_some() {
            Ok(())
        } else {
            Err(FailureReason::TargetGone)
        }
    }

    fn is_plan_valid(&self, target: &TargetCandidate, mind: &MindGraph) -> bool {
        let Some(target_entity) = target.as_entity() else {
            return false;
        };

        let mut produced: Vec<Value> = mind
            .query(
                Some(&Node::Entity(target_entity)),
                Some(Predicate::Produces),
                None,
            )
            .into_iter()
            .map(|t| t.object.clone())
            .collect();

        if produced.is_empty() {
            let type_triples = mind.query(
                Some(&Node::Entity(target_entity)),
                Some(Predicate::IsA),
                None,
            );
            for triple in type_triples {
                if let Value::Concept(concept) = triple.object {
                    let type_produced = mind.query(
                        Some(&Node::Concept(concept)),
                        Some(Predicate::Produces),
                        None,
                    );
                    produced.extend(type_produced.into_iter().map(|t| t.object.clone()));
                }
            }
        }

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

    // Per-tick effects while harvesting
    fn runtime_effects(&self) -> RuntimeEffects {
        RuntimeEffects {
            stamina_per_sec: STAMINA_PER_SEC,
            glucose_drain_per_sec: GLUCOSE_DRAIN_PER_SEC,
            ..Default::default()
        }
    }

    // Execution: What happens when harvest completes
    fn on_complete(&self, ctx: &mut CompletionContext) {
        // Transfer items from target's inventory to agent's inventory.
        // Perishable items get freshness = 1.0 and created_at stamped at
        // harvest time. A skilled harvester pulls more per action — novices
        // extract one item, masters extract up to three — bounded by what
        // the target actually has in stock.
        let Some(target_inv) = &mut ctx.target_inventory else {
            return;
        };
        let Some(concept) = target_inv.all_items().next().map(|t| t.concept) else {
            return;
        };

        let skill_level = ctx
            .skills
            .map(|s| s.level(SkillKind::Harvesting))
            .unwrap_or(0.0);
        // 1 at skill 0.0, 2 by ~0.5, 3 at 1.0. Floor so the progression
        // is predictable and replay-friendly.
        let desired = 1 + (skill_level * 2.0).floor() as u32;
        let available = target_inv.count(concept);
        let actual = desired.min(available);

        for _ in 0..actual {
            if !target_inv.remove(concept, 1) {
                break;
            }
            let thing = if perishable_decay_rate(concept).is_some() {
                Thing::fresh(concept, ctx.tick)
            } else {
                Thing::new(concept)
            };
            ctx.inventory.add_thing(thing);
        }
    }

    fn complete_log(&self) -> Option<&'static str> {
        Some("harvested")
    }

    /// Harvest yields whatever the target entity actually produces, not a hardcoded Apple.
    ///
    /// Checks `(Entity, Produces, ?)` first (directly observed entity), then falls back
    /// to `(ConceptType, Produces, ?)` via `IsA` (type-level knowledge from culture).
    fn plan_effects_for_target(&self, target: &TargetCandidate, mind: &MindGraph) -> Vec<Triple> {
        let Some(entity) = target.as_entity() else {
            return self.plan_effects();
        };

        // Direct: (entity, Produces, ?item)
        let produced = mind.query(Some(&Node::Entity(entity)), Some(Predicate::Produces), None);
        if !produced.is_empty() {
            return produced
                .into_iter()
                .map(|t| Triple::new(Node::Self_, Predicate::Contains, t.object.clone()))
                .collect();
        }

        // Indirect: entity IsA concept → (concept, Produces, ?item)
        let type_triples = mind.query(Some(&Node::Entity(entity)), Some(Predicate::IsA), None);
        let concept_effects: Vec<Triple> = type_triples
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
            .collect();

        if concept_effects.is_empty() {
            self.plan_effects()
        } else {
            concept_effects
        }
    }
}
