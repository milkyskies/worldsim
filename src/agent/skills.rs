//! Skills: per-agent learned-proficiency system.
//!
//! Reads: SimEvent::ActionCompleted, Personality, Transform, ActiveActions
//! Writes: Skills (per-agent levels), SimEvent::SkillChanged
//! Upstream: nervous_system::execution (ActionCompleted), psyche::personality (learning-rate modulation)
//! Downstream: actions::action::harvest (yield scaling), event_log (SkillChanged), future combat/build actions
//!
//! # Model
//!
//! Each agent carries a [`Skills`] component — a sparse map from [`SkillKind`]
//! to [`SkillState`] (level plus last-practiced tick). Completing a relevant
//! action drives [`skill_progression_system`], which increments the matching
//! skill. Disuse is handled by [`decay_skills_system`], a canonical half-life
//! decay mirroring `psyche::relationships::decay_relationships`.
//!
//! The learning curve is logarithmic: each practice event contributes
//! `base * (1 - level)^2`, so a novice gains quickly and a near-master only
//! inches upward. Skills are capped at 1.0 and decay toward a floor of 0.05
//! (you never fully forget what you once learned).
//!
//! Personality modulates learning rate — high conscientiousness learns
//! faster. Mentorship adds a proximity bonus: practicing near a more-skilled
//! agent performing the same action speeds up the gain, proportional to the
//! skill gap.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::agent::Agent;
use crate::agent::actions::ActionType;
use crate::agent::actions::registry::ActiveActions;
use crate::agent::events::{SimEvent, SimEventKind};
use crate::agent::psyche::personality::Personality;
use crate::core::tick::TickCount;
use crate::core::time::GameTime;
use crate::world::map::TILE_SIZE;

// ════════════════════════════════════════════════════════════════════════════
// SKILL KINDS
// ════════════════════════════════════════════════════════════════════════════

/// Coarse learned-proficiency categories. A single kind covers every action
/// that uses the same broad competence, so practicing Attack and Bite both
/// train `Combat`. Split further when a domain grows specialised enough to
/// justify it (e.g. split `Combat` into `Melee`/`Ranged` once ranged weapons
/// exist).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect, serde::Serialize)]
pub enum SkillKind {
    /// Trained by Attack and Bite. Scales hit rolls, damage output, and
    /// (future) ability to disarm or grapple.
    Combat,
    /// Trained by Harvest. Scales items gathered per action and reduces
    /// wastage on the target's stock.
    Harvesting,
    /// Trained by Build and Construct. Scales construction progress per
    /// labor tick and (future) finished-structure quality.
    Building,
    /// Trained by Converse, Wave, and InitiateConversation. Scales
    /// persuasion strength, rumor propagation, and (future) teaching.
    Socializing,
}

impl SkillKind {
    pub const ALL: [SkillKind; 4] = [
        SkillKind::Combat,
        SkillKind::Harvesting,
        SkillKind::Building,
        SkillKind::Socializing,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            SkillKind::Combat => "Combat",
            SkillKind::Harvesting => "Harvesting",
            SkillKind::Building => "Building",
            SkillKind::Socializing => "Socializing",
        }
    }
}

/// Maps an action to the skill it trains. Actions with no skill (Walk,
/// Idle, Sleep, Eat, Drink, ...) return `None` and are ignored by the
/// progression system.
pub fn skill_for_action(action: ActionType) -> Option<SkillKind> {
    match action {
        ActionType::Attack | ActionType::Bite | ActionType::DefendSelf => Some(SkillKind::Combat),
        ActionType::Harvest => Some(SkillKind::Harvesting),
        ActionType::Build | ActionType::Construct => Some(SkillKind::Building),
        ActionType::Converse | ActionType::Wave | ActionType::InitiateConversation => {
            Some(SkillKind::Socializing)
        }
        _ => None,
    }
}

// ════════════════════════════════════════════════════════════════════════════
// COMPONENT
// ════════════════════════════════════════════════════════════════════════════

/// Per-skill state: current level and the tick it was last practiced.
#[derive(Debug, Clone, Copy, Reflect)]
pub struct SkillState {
    /// Proficiency in `[0.0, 1.0]`. 0.0 is untrained; 1.0 is mastery.
    pub level: f32,
    /// Simulation tick of the most recent practice event. Used by the
    /// decay grace window.
    pub last_practiced: u64,
}

impl SkillState {
    fn new(tick: u64) -> Self {
        Self {
            level: 0.0,
            last_practiced: tick,
        }
    }
}

/// Learned skills carried by every thinking agent. Starts empty; kinds are
/// inserted lazily the first time they're practiced.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct Skills {
    #[reflect(ignore)]
    levels: HashMap<SkillKind, SkillState>,
}

impl Skills {
    /// Returns the current level of a skill, or 0.0 if never practiced.
    pub fn level(&self, kind: SkillKind) -> f32 {
        self.levels.get(&kind).map(|s| s.level).unwrap_or(0.0)
    }

    /// Iterate every trained skill as `(kind, level)` pairs. Untouched
    /// skills are omitted — callers that want "every `SkillKind::ALL`
    /// including zeros" should iterate `SkillKind::ALL` and call
    /// [`Skills::level`] themselves.
    pub fn iter(&self) -> impl Iterator<Item = (SkillKind, f32)> + '_ {
        self.levels.iter().map(|(k, s)| (*k, s.level))
    }

    /// Apply one practice event with a pre-modulated `base_delta`.
    ///
    /// Uses a diminishing-returns shape: `effective = base * (1 - level)^2`.
    /// Slope is steep near 0 (novices learn fast) and flat near 1 (masters
    /// plateau). Returns `Some((old, new))` when the level changed, `None`
    /// if the update was a no-op (e.g. already at 1.0).
    pub fn practice(&mut self, kind: SkillKind, base_delta: f32, tick: u64) -> Option<(f32, f32)> {
        let state = self
            .levels
            .entry(kind)
            .or_insert_with(|| SkillState::new(tick));
        let old = state.level;
        let headroom = (1.0 - old).clamp(0.0, 1.0);
        let effective = base_delta * headroom * headroom;
        let new = (old + effective).clamp(0.0, 1.0);
        state.level = new;
        state.last_practiced = tick;
        if (new - old).abs() > f32::EPSILON {
            Some((old, new))
        } else {
            None
        }
    }

    /// Directly set a skill level. Used by culture-seeded spawners (future)
    /// and tests. Does not stamp `last_practiced`, so the decay grace window
    /// has to be primed separately.
    pub fn set_level(&mut self, kind: SkillKind, level: f32, tick: u64) {
        self.levels.insert(
            kind,
            SkillState {
                level: level.clamp(0.0, 1.0),
                last_practiced: tick,
            },
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════
// CONFIG
// ════════════════════════════════════════════════════════════════════════════

/// Tunables for the skills system. Defaults chosen so that a dedicated
/// practitioner reaches meaningful proficiency within a handful of game
/// days and plateaus well before mastery, while a disused skill drifts
/// back to the floor over a season.
#[derive(Resource, Reflect, Debug, Clone)]
#[reflect(Resource)]
pub struct SkillsConfig {
    /// Base learning delta per completed action, before personality and
    /// mentorship modulation and before the diminishing-returns curve.
    pub base_learning_rate: f32,
    /// How much Big Five conscientiousness scales learning. A value of
    /// `0.6` means the top-conscientiousness agent learns 60% faster than
    /// an average agent, and the bottom-conscientiousness agent learns
    /// 60% slower.
    pub conscientiousness_boost: f32,
    /// Maximum learning-rate multiplier bonus when practicing next to a
    /// more-skilled agent. Scales with the skill gap: a `1.0` master
    /// teaching a `0.0` novice grants the full bonus.
    pub mentorship_max_bonus: f32,
    /// World-space radius within which another agent counts as a mentor.
    pub mentorship_radius: f32,
    /// Half-life (game days) for disuse decay. Thirty days = a skill
    /// halves in a game month of neglect.
    pub decay_half_life_days: f32,
    /// Floor level below which skills don't decay further.
    pub decay_floor: f32,
    /// Grace period (ticks) after the last practice during which decay
    /// is skipped. One game day by default.
    pub decay_grace_ticks: u64,
    /// Interval (ticks) between decay fires.
    pub decay_interval_ticks: u64,
    /// Game days per decay fire — production uses `1.0`; tests override.
    pub decay_step_days: f32,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            base_learning_rate: 0.05,
            conscientiousness_boost: 0.6,
            mentorship_max_bonus: 1.0,
            mentorship_radius: TILE_SIZE * 3.0,
            decay_half_life_days: 30.0,
            decay_floor: 0.05,
            decay_grace_ticks: GameTime::TICKS_PER_DAY,
            decay_interval_ticks: GameTime::TICKS_PER_DAY,
            decay_step_days: 1.0,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// LEARNING-RATE MODULATION
// ════════════════════════════════════════════════════════════════════════════

/// Returns the personality multiplier on `base_learning_rate`. Conscientious
/// agents (high trait) learn faster; spontaneous agents (low trait) learn
/// slower. Centred on `1.0` at trait value `0.5`.
pub fn personality_learning_multiplier(consc: f32, config: &SkillsConfig) -> f32 {
    let centred = (consc - 0.5) * 2.0;
    (1.0 + config.conscientiousness_boost * centred).max(0.0)
}

/// Returns the mentorship multiplier on `base_learning_rate` given the
/// learner's and the best nearby mentor's levels. `1.0` when there's no
/// mentor or the learner already surpasses them.
pub fn mentorship_multiplier(learner_level: f32, mentor_level: f32, config: &SkillsConfig) -> f32 {
    if mentor_level <= learner_level {
        return 1.0;
    }
    let gap = (mentor_level - learner_level).clamp(0.0, 1.0);
    1.0 + config.mentorship_max_bonus * gap
}

// ════════════════════════════════════════════════════════════════════════════
// PROGRESSION SYSTEM
// ════════════════════════════════════════════════════════════════════════════

/// Snapshot of one agent's position, personality trait, skill levels, and
/// currently-running actions. Captured up-front so the mutation pass can
/// read other agents' state without aliasing the mutable query.
struct AgentSnapshot {
    entity: Entity,
    pos: Vec2,
    conscientiousness: f32,
    skills: HashMap<SkillKind, f32>,
    active: Vec<ActionType>,
}

/// System: process ActionCompleted events and award practice XP.
///
/// For each completed action:
/// 1. Look up the skill it trains (`skill_for_action`). No-op if none.
/// 2. Find the best-skilled *other* agent inside `mentorship_radius` who
///    is currently running the same action, if any.
/// 3. Compute the learning delta from base rate, personality multiplier,
///    and mentorship multiplier.
/// 4. Apply `Skills::practice` and emit `SimEvent::SkillChanged` on change.
///
/// Runs after `tick_actions` so ActionCompleted messages from this tick
/// are visible. Early-exits when there are no events this tick so the
/// mentorship snapshot allocation only happens on frames with work.
///
/// `SimEvent` reading and writing share one `ParamSet` because Bevy's
/// system-param checker rejects a plain `MessageReader` + `MessageWriter`
/// pair against the same message type (disjoint-access violation).
pub fn skill_progression_system(
    mut sim_events: ParamSet<(MessageReader<SimEvent>, MessageWriter<SimEvent>)>,
    mut agents: Query<
        (
            Entity,
            &mut Skills,
            Option<&Personality>,
            &Transform,
            &ActiveActions,
        ),
        With<Agent>,
    >,
    config: Res<SkillsConfig>,
    tick: Res<TickCount>,
) {
    // Drain the events first so the expensive agent snapshot only runs
    // on ticks that actually have action completions. Most ticks have
    // none — this keeps the hot path allocation-free.
    let completions: Vec<(Entity, ActionType)> = sim_events
        .p0()
        .read()
        .filter_map(|event| match event {
            SimEvent {
                kind: SimEventKind::ActionCompleted { agent, action, .. },
                ..
            } => Some((*agent, *action)),
            _ => None,
        })
        .filter(|(_, action)| skill_for_action(*action).is_some())
        .collect();

    if completions.is_empty() {
        return;
    }

    let snapshots: Vec<AgentSnapshot> = agents
        .iter()
        .map(
            |(entity, skills, personality, transform, active)| AgentSnapshot {
                entity,
                pos: transform.translation.truncate(),
                conscientiousness: personality
                    .map(|p| p.traits.conscientiousness)
                    .unwrap_or(0.5),
                skills: skills.levels.iter().map(|(k, s)| (*k, s.level)).collect(),
                active: active.iter().map(|s| s.action_type).collect(),
            },
        )
        .collect();

    let current_tick = tick.current;
    // Collected here so ParamSet can switch from reader to writer only
    // once, after every mutation is done.
    let mut emitted: Vec<(Entity, SkillKind, f32, f32)> = Vec::new();

    for (learner_entity, action) in completions {
        let Some(skill_kind) = skill_for_action(action) else {
            continue;
        };

        let Some(learner) = snapshots.iter().find(|s| s.entity == learner_entity) else {
            continue;
        };

        let learner_level = learner.skills.get(&skill_kind).copied().unwrap_or(0.0);

        let mentor_level = snapshots
            .iter()
            .filter(|peer| peer.entity != learner.entity)
            .filter(|peer| learner.pos.distance(peer.pos) <= config.mentorship_radius)
            .filter(|peer| peer.active.contains(&action))
            .map(|peer| peer.skills.get(&skill_kind).copied().unwrap_or(0.0))
            .fold(0.0_f32, f32::max);

        let personality_mult = personality_learning_multiplier(learner.conscientiousness, &config);
        let mentorship_mult = mentorship_multiplier(learner_level, mentor_level, &config);
        let delta = config.base_learning_rate * personality_mult * mentorship_mult;

        let Ok((_, mut skills, _, _, _)) = agents.get_mut(learner_entity) else {
            continue;
        };

        if let Some((old, new)) = skills.practice(skill_kind, delta, current_tick) {
            emitted.push((learner_entity, skill_kind, old, new));
        }
    }

    if !emitted.is_empty() {
        let mut writer = sim_events.p1();
        for (agent, skill, old, new) in emitted {
            writer.write(SimEvent::single(
                current_tick,
                agent,
                SimEventKind::SkillChanged {
                    agent,
                    skill,
                    old_value: old,
                    new_value: new,
                },
            ));
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// DECAY SYSTEM
// ════════════════════════════════════════════════════════════════════════════

/// Fraction of the distance to the floor that a skill decays over one step.
/// Exponential half-life: `frac = 1 - 0.5^(step/half_life)`.
fn decay_fraction(step_days: f32, half_life_days: f32) -> f32 {
    if half_life_days <= 0.0 {
        return 0.0;
    }
    1.0 - 0.5_f32.powf(step_days / half_life_days)
}

/// System: exponentially decay disused skills toward the floor.
///
/// Mirrors `psyche::relationships::decay_relationships` in shape: fires on
/// a tick interval, skips recently-practiced skills via a grace window,
/// and pulls toward a neutral value (the floor, not neutral — skills only
/// decay downward).
pub fn decay_skills_system(
    mut agents: Query<&mut Skills, With<Agent>>,
    tick: Res<TickCount>,
    config: Res<SkillsConfig>,
) {
    if config.decay_interval_ticks == 0 || !tick.current.is_multiple_of(config.decay_interval_ticks)
    {
        return;
    }

    let current_tick = tick.current;
    let fraction = decay_fraction(config.decay_step_days, config.decay_half_life_days);

    for mut skills in agents.iter_mut() {
        for state in skills.levels.values_mut() {
            if current_tick.saturating_sub(state.last_practiced) < config.decay_grace_ticks {
                continue;
            }
            if state.level <= config.decay_floor {
                continue;
            }
            let new_level = state.level - (state.level - config.decay_floor) * fraction;
            state.level = new_level.max(config.decay_floor);
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// TESTS
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SkillsConfig {
        SkillsConfig {
            base_learning_rate: 0.1,
            ..SkillsConfig::default()
        }
    }

    #[test]
    fn first_practice_from_zero_makes_visible_progress() {
        let mut skills = Skills::default();
        let changed = skills.practice(SkillKind::Harvesting, 0.1, 100);
        assert!(changed.is_some());
        let level = skills.level(SkillKind::Harvesting);
        assert!(level > 0.0, "first practice should leave level > 0");
        assert!(level <= 0.1, "first practice can't exceed base delta");
    }

    #[test]
    fn diminishing_returns_shrink_gains_near_mastery() {
        let mut novice = Skills::default();
        let mut master = Skills::default();
        master.set_level(SkillKind::Harvesting, 0.9, 0);

        let (_, novice_new) = novice.practice(SkillKind::Harvesting, 0.1, 0).unwrap();
        let (_, master_new) = master.practice(SkillKind::Harvesting, 0.1, 0).unwrap();

        let novice_gain = novice_new;
        let master_gain = master_new - 0.9;

        assert!(
            novice_gain > master_gain * 10.0,
            "novice should gain much more than master (novice={novice_gain}, master={master_gain})"
        );
    }

    #[test]
    fn level_caps_at_one() {
        let mut skills = Skills::default();
        skills.set_level(SkillKind::Combat, 0.9, 0);
        // Delta large enough that the raw addition would overshoot 1.0 —
        // headroom² × 100 = 0.01 × 100 = 1.0, added to 0.9 gives 1.9, which
        // the clamp must pin to 1.0.
        skills.practice(SkillKind::Combat, 100.0, 0);
        assert_eq!(skills.level(SkillKind::Combat), 1.0);
    }

    #[test]
    fn high_conscientiousness_learns_faster_than_low() {
        let config = test_config();
        let high = personality_learning_multiplier(1.0, &config);
        let low = personality_learning_multiplier(0.0, &config);
        let mid = personality_learning_multiplier(0.5, &config);

        assert!(
            (mid - 1.0).abs() < f32::EPSILON,
            "mid conscientiousness should be neutral (got {mid})"
        );
        assert!(
            high > low,
            "high consc ({high}) should learn faster than low ({low})"
        );
        assert!(high > 1.0, "high consc should be above baseline");
        assert!(low < 1.0, "low consc should be below baseline");
    }

    #[test]
    fn mentorship_adds_bonus_when_mentor_is_ahead() {
        let config = test_config();
        let no_mentor = mentorship_multiplier(0.2, 0.0, &config);
        let with_mentor = mentorship_multiplier(0.2, 0.9, &config);
        let self_master = mentorship_multiplier(0.9, 0.2, &config);

        assert!(
            (no_mentor - 1.0).abs() < f32::EPSILON,
            "no mentor should be neutral (got {no_mentor})"
        );
        assert!(
            with_mentor > 1.0,
            "mentor ahead should boost learning (got {with_mentor})"
        );
        assert!(
            (self_master - 1.0).abs() < f32::EPSILON,
            "learner ahead of peer should get no bonus (got {self_master})"
        );
    }

    #[test]
    fn decay_fraction_half_life_math() {
        // One full half-life step should remove exactly half the distance.
        let frac = decay_fraction(30.0, 30.0);
        assert!(
            (frac - 0.5).abs() < 1e-6,
            "one half-life step should decay 50%, got {frac}"
        );
        // A tiny step should decay very little.
        let tiny = decay_fraction(1.0, 30.0);
        assert!(
            tiny > 0.0 && tiny < 0.05,
            "one-day step should be small, got {tiny}"
        );
    }

    #[test]
    fn decay_does_not_push_below_floor() {
        let config = SkillsConfig {
            decay_floor: 0.1,
            decay_half_life_days: 1.0,
            ..SkillsConfig::default()
        };
        let frac = decay_fraction(config.decay_step_days, config.decay_half_life_days);
        // Simulate what the system does inline.
        let mut level: f32 = 0.2;
        for _ in 0..100 {
            let new_level = level - (level - config.decay_floor) * frac;
            level = new_level.max(config.decay_floor);
        }
        assert!(
            (level - config.decay_floor).abs() < 1e-4,
            "level should asymptote to floor, got {level}"
        );
    }
}
