use crate::agent::body::needs::PhysicalNeeds;
use crate::core::GameLog;
use bevy::prelude::*;

#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct Body {
    pub head: BodyPart,
    pub torso: BodyPart,
    pub left_arm: BodyPart,
    pub right_arm: BodyPart,
    pub left_leg: BodyPart,
    pub right_leg: BodyPart,
}

impl Default for Body {
    fn default() -> Self {
        Self {
            head: BodyPart::new(50.0),
            torso: BodyPart::new(100.0),
            left_arm: BodyPart::new(60.0),
            right_arm: BodyPart::new(60.0),
            left_leg: BodyPart::new(70.0),
            right_leg: BodyPart::new(70.0),
        }
    }
}

impl Body {
    /// Returns an iterator over all body parts
    pub fn parts(&self) -> impl Iterator<Item = &BodyPart> {
        [
            &self.head,
            &self.torso,
            &self.left_arm,
            &self.right_arm,
            &self.left_leg,
            &self.right_leg,
        ]
        .into_iter()
    }

    /// Calculate total pain from all body parts
    /// Used by Survival Brain to detect pain urgency
    pub fn total_pain(&self) -> f32 {
        self.head.total_pain()
            + self.torso.total_pain()
            + self.left_arm.total_pain()
            + self.right_arm.total_pain()
            + self.left_leg.total_pain()
            + self.right_leg.total_pain()
    }

    /// Calculate mobility from leg function
    /// 0.0 = cannot walk, 1.0 = full mobility
    pub fn mobility(&self) -> f32 {
        (self.left_leg.function_rate + self.right_leg.function_rate) / 2.0
    }

    /// Calculate manipulation capability from arm function
    /// 0.0 = cannot use hands, 1.0 = full dexterity
    pub fn manipulation(&self) -> f32 {
        self.left_arm
            .function_rate
            .max(self.right_arm.function_rate)
    }

    /// Check if agent is incapacitated (head or torso critical)
    pub fn is_incapacitated(&self) -> bool {
        self.head.function_rate < 0.2 || self.torso.function_rate < 0.2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub enum InjuryType {
    Cut,
    Bruise,
    Fracture,
    Burn,
    Infection,
}

#[derive(Debug, Clone, Reflect)]
pub struct Injury {
    pub injury_type: InjuryType,
    pub severity: f32,      // 0.0 to 1.0 (1.0 = Max severity)
    pub pain: f32,          // 0.0 to 10.0
    pub healed_amount: f32, // 0.0 to 1.0 (1.0 = Fully healed)
}

#[derive(Debug, Clone, Reflect)]
pub struct BodyPart {
    pub max_hp: f32,
    pub current_hp: f32,
    pub function_rate: f32, // 0.0 to 1.0 calculated from hp + injuries
    pub injuries: Vec<Injury>,
}

impl BodyPart {
    pub fn new(max_hp: f32) -> Self {
        Self {
            max_hp,
            current_hp: max_hp,
            function_rate: 1.0,
            injuries: Vec::new(),
        }
    }

    pub fn add_injury(&mut self, injury: Injury) {
        self.current_hp = (self.current_hp - (injury.severity * 20.0)).max(0.0);
        self.injuries.push(injury);
        self.recalculate_function();
    }

    pub fn recalculate_function(&mut self) {
        // Base function from Health (0 HP = 0 function)
        let hp_factor = self.current_hp / self.max_hp;

        // Penalty from injuries
        let mut injury_penalty = 0.0;
        for injury in &self.injuries {
            let heal_factor = 1.0 - injury.healed_amount;
            injury_penalty += injury.severity * heal_factor;
        }

        self.function_rate = (hp_factor - injury_penalty).clamp(0.0, 1.0);
    }

    /// Calculate total pain from all injuries in this body part
    pub fn total_pain(&self) -> f32 {
        self.injuries
            .iter()
            .map(|injury| injury.pain * (1.0 - injury.healed_amount))
            .sum()
    }
}

pub fn process_healing(mut query: Query<(&mut Body, Option<&PhysicalNeeds>)>, time: Res<Time>) {
    let dt = time.delta_secs();
    let base_healing_speed = 0.05; // Amount of severity healed per second

    for (mut body, needs) in query.iter_mut() {
        let mut healing_speed = base_healing_speed;

        // Healing Bonus for being well rested
        if let Some(physical) = needs
            && physical.energy > 80.0 {
                healing_speed *= 2.0; // Double healing speed when rested
            }

        // Destructure to convince borrow checker of disjointness
        let Body {
            head,
            torso,
            left_arm,
            right_arm,
            left_leg,
            right_leg,
        } = &mut *body;

        let parts = vec![head, torso, left_arm, right_arm, left_leg, right_leg];

        for part in parts {
            let mut fully_healed_indices = Vec::new();

            // 1. Heal Injuries
            for (i, injury) in part.injuries.iter_mut().enumerate() {
                if injury.healed_amount < 1.0 {
                    injury.healed_amount += healing_speed * dt;
                    if injury.healed_amount >= 1.0 {
                        injury.healed_amount = 1.0;
                        fully_healed_indices.push(i);
                    }
                }
            }

            // 2. Process Scarring (Reduce Max HP) and Remove Healed
            // Reverse iteration to remove correctly
            for index in fully_healed_indices.iter().rev() {
                let severity = part.injuries[*index].severity;
                // Scarring: Max HP reduced by 10% of severity * 20.0 (Damage)
                // e.g. Severity 1.0 -> 2.0 Max HP reduction
                let scar_damage = severity * 2.0;
                part.max_hp = (part.max_hp - scar_damage).max(1.0);

                part.injuries.remove(*index);
            }

            // 3. Natural HP Regeneration (up to Max HP)
            if part.current_hp < part.max_hp {
                part.current_hp += 1.0 * dt;
                part.current_hp = part.current_hp.min(part.max_hp);
            }

            part.recalculate_function();
        }
    }
}

/// Starvation system - applies damage if hunger is critical
pub fn process_starvation(time: Res<Time>, mut query: Query<&mut PhysicalNeeds>) {
    let dt = time.delta_secs();

    for mut physical in query.iter_mut() {
        // Health damage if starving (hunger >= 90)
        if physical.hunger >= 90.0 {
            let health_damage = dt * 0.2; // 0.2 damage per second (~500s to die from full health?)
            physical.health = (physical.health - health_damage).clamp(0.0, 100.0);
        }
    }
}

// System to check for death
pub fn check_death(
    mut commands: Commands,
    query: Query<(Entity, &PhysicalNeeds, Option<&Name>)>,
    mut game_log: ResMut<GameLog>,
) {
    for (entity, physical, name) in query.iter() {
        if physical.health <= 0.0 {
            let name_str = name.map(|n| n.as_str()).unwrap_or("Unknown Entity");
            game_log.event(&format!("{} died of starvation/injury!", name_str));
            commands.entity(entity).despawn();
        }
    }
}
