//! Combat resolution: hit rolls, damage application, bleeding, severance, death.
//!
//! Reads: SimEvent::ActionCompleted, Body (read+write), PhysicalNeeds,
//!        Consciousness, Skills, Transform, Name, MindGraph, ItemSlots, SimRng
//! Writes: Body, PhysicalNeeds, SimEvent::CombatHit/Missed/PartSevered/Death,
//!         Liquid puddle entities, SeveredPart entities, Becomes (Corpse path)
//! Upstream: actions::action::attack / bite (emit ActionCompleted), SkillsPlugin
//! Downstream: event_log, brain (reads pain urgency), world rendering
//!
//! # Pipeline
//!
//! The resolver runs once per tick after the action execution systems:
//!
//! 1. Collect every `ActionCompleted` for Attack / Bite into a side vec.
//! 2. For each completion, fetch the attacker's snapshot (skill, mind,
//!    target). Everything needed from the attacker is captured as owned
//!    data here so the subsequent mutable defender borrow doesn't alias.
//! 3. Mutate the defender: roll dodge, pick a part, apply an Injury,
//!    optionally punch damage through to an internal organ.
//! 4. On a kill, queue a Death event, attach `Becomes::InPlace Corpse`,
//!    and remember to deposit the "first cut" meat into the attacker's
//!    inventory after the defender borrow drops.
//! 5. Spawn blood puddles proportional to the damage dealt.
//!
//! The separate `bleed_system` drains HP from open wounds every tick and
//! leaves a blood trail at the bleeder's current position. The separate
//! `severance_system` checks for non-vital parts at 0 HP and drops them
//! as `SeveredPart` world entities.

use bevy::prelude::*;
use rand::Rng;

use crate::agent::Agent;
use crate::agent::actions::ActionType;
use crate::agent::actions::channel::Channel;
use crate::agent::biology::body::{Body, BodyNodeKind, Injury, InjuryType};
use crate::agent::body::needs::Consciousness;
use crate::agent::events::SimEvent;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::agent::skills::{SkillKind, Skills};
use crate::core::GameLog;
use crate::core::sim_rng::SimRng;
use crate::core::tick::TickCount;
use crate::world::liquid::{Liquid, LiquidKind, spawn_or_merge_liquid};
use crate::world::severed_part::spawn_severed_part;

// ════════════════════════════════════════════════════════════════════════════
// TUNABLES
// ════════════════════════════════════════════════════════════════════════════

// Fist damage: a punch from a human should break ribs and crack
// skulls, not tickle. Real-world studies clock trained boxing strikes
// at >1000 N of force; a level-0 untrained human at the low end still
// ranges 10-20 HP per hit against a 50-80 HP body part. A four-hit
// kill on an unarmored target is about right for an unarmed fistfight.
const FIST_DAMAGE_MIN: f32 = 10.0;
const FIST_DAMAGE_MAX: f32 = 20.0;
// Bite damage: wolf jaws clamp at ~1500 PSI on prey. A single decent
// bite should significantly injure prey; two or three from a real
// pack kills a deer.
const BITE_DAMAGE_MIN: f32 = 18.0;
const BITE_DAMAGE_MAX: f32 = 32.0;

/// Attacker Combat skill multiplier: level 0.0 → 0.7x, level 1.0 → 1.3x.
const SKILL_MULT_BASE: f32 = 0.7;
const SKILL_MULT_SPAN: f32 = 0.6;

/// Dodge ceiling before skill shave: `alertness × locomotion × DODGE_COEFF`.
const DODGE_COEFF: f32 = 0.4;
/// A maxed-Combat attacker roughly halves the dodge ceiling.
const SKILL_DODGE_SHAVE: f32 = 0.5;

const PIERCE_ORGAN_CHANCE: f32 = 0.4;
const SLASH_ORGAN_CHANCE: f32 = 0.2;
const CRUSH_ORGAN_CHANCE: f32 = 0.05;
const ORGAN_PENETRATION_FRACTION: f32 = 0.35;

const CRIT_CHANCE: f32 = 0.1;
const GRAZE_CHANCE: f32 = 0.1;
const CRIT_MULT: f32 = 1.5;
const GRAZE_MULT: f32 = 0.5;

const PAIN_PER_SEVERITY: f32 = 6.0;

const PIERCE_BLEED_COEFF: f32 = 1.5;
const SLASH_BLEED_COEFF: f32 = 2.0;
const CRUSH_BLEED_COEFF: f32 = 0.0;

// ════════════════════════════════════════════════════════════════════════════
// DAMAGE HELPERS
// ════════════════════════════════════════════════════════════════════════════

fn default_damage_type(action: ActionType) -> InjuryType {
    match action {
        ActionType::Attack => InjuryType::Crush,
        ActionType::Bite => InjuryType::Pierce,
        _ => InjuryType::Bruise,
    }
}

fn base_damage_range(action: ActionType) -> Option<(f32, f32)> {
    match action {
        ActionType::Attack => Some((FIST_DAMAGE_MIN, FIST_DAMAGE_MAX)),
        ActionType::Bite => Some((BITE_DAMAGE_MIN, BITE_DAMAGE_MAX)),
        _ => None,
    }
}

fn organ_chance(kind: InjuryType) -> f32 {
    match kind {
        InjuryType::Pierce => PIERCE_ORGAN_CHANCE,
        InjuryType::Slash => SLASH_ORGAN_CHANCE,
        InjuryType::Crush => CRUSH_ORGAN_CHANCE,
        _ => 0.0,
    }
}

fn bleed_coefficient(kind: InjuryType) -> f32 {
    match kind {
        InjuryType::Pierce => PIERCE_BLEED_COEFF,
        InjuryType::Slash => SLASH_BLEED_COEFF,
        InjuryType::Crush => CRUSH_BLEED_COEFF,
        _ => 0.0,
    }
}

/// Weighted pick of a target body part. Bigger parts catch more hits.
fn pick_hit_location(rng: &mut impl Rng, body: &Body) -> Option<BodyNodeKind> {
    let total: f32 = body.parts.iter().map(|p| p.max_hp).sum();
    if total <= 0.0 {
        return None;
    }
    let mut roll = rng.random_range(0.0..total);
    for part in &body.parts {
        roll -= part.max_hp;
        if roll <= 0.0 {
            return Some(part.kind);
        }
    }
    body.parts.last().map(|p| p.kind)
}

/// Outcome of a single strike: the parts the rest of the pipeline needs
/// to emit events, deposit meat, and spawn blood.
#[derive(Debug, Clone, Copy)]
enum Resolution {
    Missed,
    Landed(HitOutcome),
}

#[derive(Debug, Clone, Copy)]
struct HitOutcome {
    part_kind: BodyNodeKind,
    damage: f32,
    injury_type: InjuryType,
    vital_organ_destroyed: Option<BodyNodeKind>,
    defender_died: bool,
}

/// Apply a strike to the defender and return what happened. Mutates the
/// defender's body in-place.
fn apply_strike(
    rng: &mut impl Rng,
    action: ActionType,
    attacker_combat_skill: f32,
    defender_body: &mut Body,
    defender_alertness: f32,
    defender_locomotion: f32,
) -> Resolution {
    let dodge = (defender_alertness * defender_locomotion * DODGE_COEFF)
        * (1.0 - attacker_combat_skill * SKILL_DODGE_SHAVE);
    if rng.random::<f32>() < dodge {
        return Resolution::Missed;
    }

    let Some((min, max)) = base_damage_range(action) else {
        return Resolution::Missed;
    };
    let mut damage = rng.random_range(min..=max);
    let skill_mult = SKILL_MULT_BASE + SKILL_MULT_SPAN * attacker_combat_skill.clamp(0.0, 1.0);
    damage *= skill_mult;

    let roll = rng.random::<f32>();
    if roll < CRIT_CHANCE {
        damage *= CRIT_MULT;
    } else if roll > 1.0 - GRAZE_CHANCE {
        damage *= GRAZE_MULT;
    }

    let Some(part_kind) = pick_hit_location(rng, defender_body) else {
        return Resolution::Missed;
    };
    let injury_type = default_damage_type(action);

    let mut vital_organ_destroyed: Option<BodyNodeKind> = None;
    if let Some(part) = defender_body.part_mut(part_kind) {
        part.current_hp = (part.current_hp - damage).max(0.0);
        let severity = (damage / part.max_hp.max(1.0)).clamp(0.0, 1.0);
        let pain = severity * PAIN_PER_SEVERITY;
        let bleed_rate = severity * bleed_coefficient(injury_type);
        part.injuries.push(Injury {
            injury_type,
            severity,
            pain,
            bleed_rate,
            healed_amount: 0.0,
        });
        part.recalculate_function();

        if !part.children.is_empty() && rng.random::<f32>() < organ_chance(injury_type) {
            let idx = rng.random_range(0..part.children.len());
            let child = &mut part.children[idx];
            let penetration = damage * ORGAN_PENETRATION_FRACTION;
            child.current_hp = (child.current_hp - penetration).max(0.0);
            child.recalculate_function();
            if child.is_destroyed() && child.vital {
                vital_organ_destroyed = Some(child.kind);
            }
        }
    }

    let defender_died =
        defender_body.any_vital_organ_destroyed() || defender_body.is_incapacitated();

    Resolution::Landed(HitOutcome {
        part_kind,
        damage,
        injury_type,
        vital_organ_destroyed,
        defender_died,
    })
}

// ════════════════════════════════════════════════════════════════════════════
// PREY YIELD
// ════════════════════════════════════════════════════════════════════════════

/// Compute the items a killed prey should drop into the killer's
/// inventory ("first cut" deposit). Mirrors the old `apply_hunt_kill`
/// shape so the existing Walk → Attack → Eat hunter plan chain keeps
/// working without teaching the planner about corpse-harvesting.
fn compute_prey_yield(mind: &MindGraph, defender: Entity) -> Vec<(Concept, u32)> {
    let mut result: Vec<(Concept, u32)> = Vec::new();
    if !mind.has_trait(&Node::Entity(defender), Concept::Prey) {
        return result;
    }
    // Direct (entity, Produces, item).
    let direct = mind.query(
        Some(&Node::Entity(defender)),
        Some(Predicate::Produces),
        None,
    );
    if !direct.is_empty() {
        for triple in direct {
            if let Value::Item(concept, qty) = triple.object {
                result.push((concept, qty));
            }
        }
        return result;
    }
    // Indirect via IsA type chain.
    let type_triples = mind.query(Some(&Node::Entity(defender)), Some(Predicate::IsA), None);
    for triple in type_triples {
        if let Value::Concept(concept) = triple.object {
            let produced = mind.query(
                Some(&Node::Concept(concept)),
                Some(Predicate::Produces),
                None,
            );
            for p in produced {
                if let Value::Item(c, qty) = p.object {
                    result.push((c, qty));
                }
            }
        }
    }
    result
}

// ════════════════════════════════════════════════════════════════════════════
// RESOLUTION SYSTEM
// ════════════════════════════════════════════════════════════════════════════

/// Compact record of everything the resolver needs to follow up on once
/// the defender borrow has been released. The hit outcome (if any) is
/// stored as owned data so phase 3 can build the `SimEvent::CombatHit`
/// lazily without cloning.
struct CombatEffect {
    attacker: Entity,
    defender: Entity,
    defender_pos: Vec2,
    outcome: CombatOutcome,
    /// Items to deposit into attacker inventory on a killing blow.
    prey_drops: Vec<(Concept, u32)>,
}

enum CombatOutcome {
    Dodged,
    Hit {
        part_kind: BodyNodeKind,
        damage: f32,
        injury_type: InjuryType,
        defender_died: bool,
        vital_organ_destroyed: Option<BodyNodeKind>,
    },
}

/// System: read `ActionCompleted` for Attack/Bite and resolve each strike.
#[allow(clippy::too_many_arguments)]
pub fn resolve_combat_hits(
    mut commands: Commands,
    mut sim_events: ParamSet<(MessageReader<SimEvent>, MessageWriter<SimEvent>)>,
    mut rng: ResMut<SimRng>,
    mut game_log: ResMut<GameLog>,
    tick: Res<TickCount>,
    names: Query<&Name>,
    // Single big query covering attacker + defender. Using one unified
    // query sidesteps the Bevy borrow checker's two-mut-queries-on-
    // overlapping-archetypes rejection: we reach both sides via
    // `get_many_mut` on disjoint entities.
    mut agents: Query<
        (
            &mut Body,
            &mut ItemSlots,
            Option<&Consciousness>,
            &Transform,
            Option<&Skills>,
            &MindGraph,
        ),
        With<Agent>,
    >,
    mut liquids: Query<(Entity, &Transform, &mut Liquid)>,
) {
    // Phase 1: collect combat-relevant completions. The target travels
    // on the event itself so no ActiveActions lookup is needed — by the
    // time this system runs, the completed action is already gone from
    // the running set.
    let combat_completions: Vec<(Entity, ActionType, Entity)> = sim_events
        .p0()
        .read()
        .filter_map(|event| match event {
            SimEvent::ActionCompleted {
                agent,
                action,
                target: Some(target),
                ..
            } if matches!(*action, ActionType::Attack | ActionType::Bite) => {
                Some((*agent, *action, *target))
            }
            _ => None,
        })
        .collect();

    if combat_completions.is_empty() {
        return;
    }

    // Phase 2: resolve each hit and buffer the side effects.
    let mut effects: Vec<CombatEffect> = Vec::new();

    for (attacker, action, defender) in combat_completions {
        if defender == attacker {
            continue;
        }

        // Snapshot attacker data before we borrow the defender mutably.
        // Using `.get()` on the mutable query returns immutable refs, so
        // no borrow checker conflict with the later `.get_mut()`.
        let (attacker_skill, prey_drops) = {
            let Ok((_, _, _, _, skills, mind)) = agents.get(attacker) else {
                continue;
            };
            let skill = skills.map(|s| s.level(SkillKind::Combat)).unwrap_or(0.0);
            let drops = compute_prey_yield(mind, defender);
            (skill, drops)
        };

        // Now mutate the defender.
        let (resolution, blood_pos) = {
            let Ok((mut defender_body, _, consc, def_transform, _, _)) = agents.get_mut(defender)
            else {
                continue;
            };
            let alertness = consc.map(|c| c.alertness).unwrap_or(1.0);
            let locomotion = defender_body.channel_capacity(Channel::Locomotion).min(1.5);
            let pos = def_transform.translation.truncate();
            let res = apply_strike(
                rng.inner_mut(),
                action,
                attacker_skill,
                &mut defender_body,
                alertness,
                locomotion,
            );
            (res, pos)
        };

        let outcome = match resolution {
            Resolution::Missed => CombatOutcome::Dodged,
            Resolution::Landed(hit) => CombatOutcome::Hit {
                part_kind: hit.part_kind,
                damage: hit.damage,
                injury_type: hit.injury_type,
                defender_died: hit.defender_died,
                vital_organ_destroyed: hit.vital_organ_destroyed,
            },
        };

        let drops = matches!(
            outcome,
            CombatOutcome::Hit {
                defender_died: true,
                ..
            }
        )
        .then_some(prey_drops)
        .unwrap_or_default();

        effects.push(CombatEffect {
            attacker,
            defender,
            defender_pos: blood_pos,
            outcome,
            prey_drops: drops,
        });
    }

    // Phase 3: apply buffered effects.
    let mut emitted: Vec<SimEvent> = Vec::new();

    for effect in effects {
        let defender_name = names
            .get(effect.defender)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|_| format!("{:?}", effect.defender));
        let attacker_name = names
            .get(effect.attacker)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|_| format!("{:?}", effect.attacker));

        match effect.outcome {
            CombatOutcome::Dodged => {
                game_log.event(&format!("{attacker_name} missed {defender_name}"));
                emitted.push(SimEvent::CombatMissed {
                    attacker: effect.attacker,
                    defender: effect.defender,
                    tick: tick.current,
                });
            }
            CombatOutcome::Hit {
                part_kind,
                damage,
                injury_type,
                defender_died,
                vital_organ_destroyed,
            } => {
                game_log.event(&format!(
                    "{attacker_name} struck {defender_name} ({}) for {damage:.0} {injury_type:?}",
                    part_kind.display_name()
                ));
                emitted.push(SimEvent::CombatHit {
                    attacker: effect.attacker,
                    defender: effect.defender,
                    tick: tick.current,
                    part_kind,
                    damage,
                    injury_type,
                });

                // Blood splash proportional to bleed coefficient.
                let amount = damage * bleed_coefficient(injury_type) * 0.5;
                if amount > 0.0 {
                    spawn_or_merge_liquid(
                        &mut commands,
                        &mut liquids,
                        LiquidKind::Blood,
                        effect.defender_pos,
                        amount,
                        tick.current,
                    );
                }

                if defender_died {
                    // Deposit "first cut" meat into the attacker's
                    // inventory via the combined query.
                    if !effect.prey_drops.is_empty()
                        && let Ok((_, mut inv, _, _, _, _)) = agents.get_mut(effect.attacker)
                    {
                        for (concept, qty) in effect.prey_drops {
                            inv.add(concept, qty);
                        }
                    }

                    // Attach Corpse transformation.
                    commands
                        .entity(effect.defender)
                        .insert(crate::world::becomes::Becomes {
                            target: Concept::Corpse,
                            trigger: crate::world::becomes::BecomesTrigger::AfterTicks(0),
                            started_tick: tick.current,
                            mode: crate::world::becomes::BecomesMode::InPlace,
                        });
                    // Kills spew extra blood.
                    spawn_or_merge_liquid(
                        &mut commands,
                        &mut liquids,
                        LiquidKind::Blood,
                        effect.defender_pos,
                        20.0,
                        tick.current,
                    );

                    let cause = match vital_organ_destroyed {
                        Some(organ) => format!("combat (vital organ: {organ:?})"),
                        None => "combat (bleed/incapacitation)".to_string(),
                    };
                    game_log.event(&format!("{defender_name} died of {cause}"));
                    emitted.push(SimEvent::Death {
                        agent: effect.defender,
                        tick: tick.current,
                        cause,
                    });
                }
            }
        }
    }

    if !emitted.is_empty() {
        let mut writer = sim_events.p1();
        for event in emitted {
            writer.write(event);
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// BLEEDING SYSTEM
// ════════════════════════════════════════════════════════════════════════════

fn bleed_node(node: &mut crate::agent::biology::body::BodyNode, dt: f32) -> f32 {
    let mut node_bleed = 0.0_f32;
    for injury in node.injuries.iter_mut() {
        node_bleed += injury.effective_bleed();
        injury.bleed_rate =
            (injury.bleed_rate - crate::agent::biology::body::CLOT_DECAY_PER_SEC * dt).max(0.0);
    }
    if node_bleed > 0.0 {
        let drain = node_bleed * dt;
        node.current_hp = (node.current_hp - drain).max(0.0);
        node.recalculate_function();
        drain
    } else {
        node.recalculate_function();
        0.0
    }
}

pub fn bleed_system(
    mut commands: Commands,
    tick: Res<TickCount>,
    mut agents: Query<(&mut Body, &Transform), With<Agent>>,
    mut liquids: Query<(Entity, &Transform, &mut Liquid)>,
) {
    let dt = tick.dt();
    if dt <= 0.0 {
        return;
    }

    let mut drips: Vec<(Vec2, f32)> = Vec::new();

    for (mut body, transform) in agents.iter_mut() {
        let mut total_drain = 0.0_f32;
        for part in body.parts.iter_mut() {
            total_drain += bleed_node(part, dt);
            for child in part.children.iter_mut() {
                total_drain += bleed_node(child, dt);
            }
        }
        if total_drain > 0.0 {
            drips.push((transform.translation.truncate(), total_drain));
        }
    }

    for (pos, amount) in drips {
        spawn_or_merge_liquid(
            &mut commands,
            &mut liquids,
            LiquidKind::Blood,
            pos,
            amount,
            tick.current,
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════
// SEVERANCE SYSTEM
// ════════════════════════════════════════════════════════════════════════════

/// Scan for non-vital body parts at 0 HP and drop them as world entities.
pub fn severance_system(
    mut commands: Commands,
    tick: Res<TickCount>,
    mut agents: Query<(Entity, &mut Body, &Transform), With<Agent>>,
    mut sim_events: MessageWriter<SimEvent>,
    mut liquids: Query<(Entity, &Transform, &mut Liquid)>,
) {
    let mut drops: Vec<(Entity, BodyNodeKind, Vec2)> = Vec::new();

    for (entity, mut body, transform) in agents.iter_mut() {
        let pos = transform.translation.truncate();
        let mut i = body.parts.len();
        while i > 0 {
            i -= 1;
            let part = &body.parts[i];
            if part.current_hp <= 0.0 && !part.vital {
                let kind = part.kind;
                body.parts.remove(i);
                drops.push((entity, kind, pos));
            }
        }
    }

    for (owner, kind, pos) in drops {
        spawn_severed_part(&mut commands, owner, kind, pos, tick.current);
        spawn_or_merge_liquid(
            &mut commands,
            &mut liquids,
            LiquidKind::Blood,
            pos,
            15.0,
            tick.current,
        );
        sim_events.write(SimEvent::PartSevered {
            entity: owner,
            tick: tick.current,
            part_kind: kind,
        });
    }
}

// ════════════════════════════════════════════════════════════════════════════
// TESTS
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    #[test]
    fn bleed_coefficients_are_ordered() {
        const { assert!(SLASH_BLEED_COEFF > PIERCE_BLEED_COEFF) };
        assert_eq!(CRUSH_BLEED_COEFF, 0.0);
    }

    #[test]
    fn default_damage_types_match_action() {
        assert_eq!(default_damage_type(ActionType::Attack), InjuryType::Crush);
        assert_eq!(default_damage_type(ActionType::Bite), InjuryType::Pierce);
    }

    #[test]
    fn pick_hit_location_returns_some_part_for_a_fresh_body() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let body = Body::human();
        for _ in 0..50 {
            assert!(pick_hit_location(&mut rng, &body).is_some());
        }
    }

    #[test]
    fn bigger_parts_get_hit_more_often() {
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let body = Body::human();
        let mut torso_count = 0;
        let mut mouth_count = 0;
        let iterations = 2000;
        for _ in 0..iterations {
            match pick_hit_location(&mut rng, &body).unwrap() {
                BodyNodeKind::Torso => torso_count += 1,
                BodyNodeKind::Mouth => mouth_count += 1,
                _ => {}
            }
        }
        // Torso (max_hp 100) should be hit a lot more than mouth (max_hp 30).
        assert!(
            torso_count > mouth_count * 2,
            "torso {torso_count} should be hit >> mouth {mouth_count}"
        );
    }

    #[test]
    fn fist_strike_deals_crush_damage_to_a_target() {
        let mut rng = ChaCha8Rng::seed_from_u64(99);
        let mut body = Body::human();
        let total_hp_before: f32 = body.parts.iter().map(|p| p.current_hp).sum();

        // Fire a bunch of strikes with max skill so dodge misses are rare.
        for _ in 0..10 {
            apply_strike(&mut rng, ActionType::Attack, 1.0, &mut body, 0.0, 0.0);
        }
        let total_hp_after: f32 = body.parts.iter().map(|p| p.current_hp).sum();
        assert!(
            total_hp_after < total_hp_before,
            "body HP should drop after multiple hits ({total_hp_before} -> {total_hp_after})"
        );

        // At least one crush injury should be present somewhere.
        let has_crush = body.parts.iter().any(|p| {
            p.injuries
                .iter()
                .any(|i| i.injury_type == InjuryType::Crush)
        });
        assert!(has_crush, "at least one crush injury expected");
    }

    #[test]
    fn bite_pierce_can_destroy_a_vital_organ_eventually() {
        // Hit a body until a vital organ dies or we give up.
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let mut body = Body::deer();
        for _ in 0..200 {
            apply_strike(&mut rng, ActionType::Bite, 1.0, &mut body, 0.0, 0.0);
            if body.any_vital_organ_destroyed() || body.is_incapacitated() {
                return;
            }
        }
        panic!("200 max-skill bites failed to kill a deer — curve is too gentle");
    }

    #[test]
    fn dodge_prevents_damage_when_alert_and_fast() {
        // Rig deterministically: zero attacker skill, max alertness +
        // locomotion. Dodge ceiling = 1.0 * 1.0 * 0.4 = 0.4. Not
        // guaranteed but over many rolls we should see *some* misses.
        let mut rng = ChaCha8Rng::seed_from_u64(123);
        let mut body = Body::human();
        let mut miss_count = 0;
        for _ in 0..100 {
            if matches!(
                apply_strike(&mut rng, ActionType::Attack, 0.0, &mut body, 1.0, 1.0),
                Resolution::Missed
            ) {
                miss_count += 1;
            }
        }
        assert!(
            miss_count > 10,
            "dodge should fire noticeably often at max alertness (got {miss_count}/100)"
        );
    }
}
