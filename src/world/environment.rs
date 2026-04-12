use crate::core::GameTime;
use crate::world::property::LightSource;
use bevy::prelude::*;

pub struct EnvironmentPlugin;

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<LightLevel>()
            .register_type::<ColorTint>()
            .register_type::<BaseColor>()
            .register_type::<AgentBodySprite>()
            .register_type::<CampfireGlowSprite>()
            .init_resource::<LightLevel>()
            .init_resource::<ColorTint>()
            .add_systems(
                Update,
                (
                    update_light_level,
                    apply_visual_lighting,
                    // sprite and glow write disjoint queries — run in parallel
                    (apply_sprite_lighting, apply_campfire_glow_lighting),
                )
                    .chain(),
            );
    }
}

/// Global light level (0.0 = Pitch Black, 1.0 = Bright Day)
#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub struct LightLevel(pub f32);

impl Default for LightLevel {
    fn default() -> Self {
        Self(1.0)
    }
}

/// Global color temperature tint applied on top of the light level.
/// Components are RGB multipliers where `Vec3::ONE` means neutral (no tint).
#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub struct ColorTint(pub Vec3);

impl Default for ColorTint {
    fn default() -> Self {
        Self(Vec3::ONE)
    }
}

/// Stores the original base color of a sprite before lighting is applied.
/// Attach to sprite entities that should respond to the day-night cycle.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct BaseColor(pub Color);

/// Marker for agent body/head sprites. These dim slightly less than terrain
/// so agents remain trackable at night.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct AgentBodySprite;

/// Marker for the campfire night-glow overlay sprite.
/// Alpha is driven by the inverse of the light level — fully visible at night.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct CampfireGlowSprite;

/// Pure function mapping game hour (0–24) to a light level (0.3–1.0).
/// Night: 0.3, Dawn (5–7): 0.3→1.0, Day (7–18): 1.0, Dusk (18–20): 1.0→0.3.
pub fn compute_light_level(hour: f32) -> f32 {
    if hour < 5.0 {
        0.3
    } else if hour < 7.0 {
        hour_lerp(0.3, 1.0, hour, 5.0, 7.0)
    } else if hour < 18.0 {
        1.0
    } else if hour < 20.0 {
        hour_lerp(1.0, 0.3, hour, 18.0, 20.0)
    } else {
        0.3
    }
}

/// Interpolates `a`→`b` as `hour` moves from `start` to `end`.
fn hour_lerp(a: f32, b: f32, hour: f32, start: f32, end: f32) -> f32 {
    let t = (hour - start) / (end - start);
    a + (b - a) * t
}

/// Multiplies a base color's RGB channels by `light * tint`, preserving alpha.
fn apply_light_tint(base: &Color, light: f32, tint: Vec3) -> Color {
    let s = base.to_srgba();
    Color::srgba(
        (s.red * light * tint.x).clamp(0.0, 1.0),
        (s.green * light * tint.y).clamp(0.0, 1.0),
        (s.blue * light * tint.z).clamp(0.0, 1.0),
        s.alpha,
    )
}

pub fn update_light_level(
    time: Res<GameTime>,
    mut light: ResMut<LightLevel>,
    mut tint: ResMut<ColorTint>,
) {
    let hour = time.hours as f32 + (time.minutes as f32 / 60.0);

    light.0 = compute_light_level(hour);

    let neutral = Vec3::ONE;
    let warm_dawn = Vec3::new(1.15, 0.88, 0.68);
    let warm_dusk = Vec3::new(1.1, 0.72, 0.52);
    let cool_night = Vec3::new(0.75, 0.85, 1.1);

    tint.0 = if hour < 5.0 {
        cool_night
    } else if hour < 6.0 {
        neutral.lerp(warm_dawn, hour - 5.0)
    } else if hour < 7.0 {
        warm_dawn.lerp(neutral, hour - 6.0)
    } else if hour < 18.0 {
        neutral
    } else if hour < 19.0 {
        neutral.lerp(warm_dusk, hour - 18.0)
    } else if hour < 20.0 {
        warm_dusk.lerp(cool_night, hour - 19.0)
    } else {
        cool_night
    };
}

fn apply_visual_lighting(light: Res<LightLevel>, mut clear_color: ResMut<ClearColor>) {
    let day_color = Vec3::new(0.4, 0.6, 0.9);
    let night_color = Vec3::new(0.05, 0.05, 0.2);

    // Map 0.3 (night) → 1.0 (day) to 0.0..1.0 for lerp
    let t = ((light.0 - 0.3) / 0.7).clamp(0.0, 1.0);
    let current = night_color.lerp(day_color, t);
    clear_color.0 = Color::srgb(current.x, current.y, current.z);
}

fn apply_sprite_lighting(
    light: Res<LightLevel>,
    tint: Res<ColorTint>,
    campfire_lights: Query<(&LightSource, &Transform)>,
    mut tiles: Query<
        (&mut Sprite, &BaseColor, &Transform),
        (Without<AgentBodySprite>, Without<CampfireGlowSprite>),
    >,
    mut agents: Query<(&mut Sprite, &BaseColor), With<AgentBodySprite>>,
) {
    if !light.is_changed() && !tint.is_changed() {
        return;
    }

    let light_level = light.0;
    let tint = tint.0;

    let campfire_sources: Vec<(Vec2, f32, f32)> = campfire_lights
        .iter()
        .map(|(src, t)| (t.translation.truncate(), src.radius, src.intensity))
        .collect();

    for (mut sprite, base, transform) in &mut tiles {
        let tile_pos = transform.translation.truncate();
        let campfire_bonus = campfire_sources
            .iter()
            .map(|(pos, radius, intensity)| {
                let dist = tile_pos.distance(*pos);
                if dist < *radius {
                    let falloff = 1.0 - (dist / radius);
                    falloff * intensity * (1.0 - light_level)
                } else {
                    0.0
                }
            })
            .fold(0.0_f32, f32::max);

        let effective = (light_level + campfire_bonus).clamp(0.0, 1.0);
        sprite.color = apply_light_tint(&base.0, effective, tint);
    }

    // Agents retain 30% of the brightness gap to remain trackable at night.
    let agent_light = light_level + (1.0 - light_level) * 0.3;
    for (mut sprite, base) in &mut agents {
        sprite.color = apply_light_tint(&base.0, agent_light, tint);
    }
}

fn apply_campfire_glow_lighting(
    light: Res<LightLevel>,
    mut glows: Query<&mut Sprite, With<CampfireGlowSprite>>,
) {
    if !light.is_changed() {
        return;
    }

    let glow_alpha = (1.0 - light.0).clamp(0.0, 1.0) * 0.8;
    for mut sprite in &mut glows {
        sprite.color.set_alpha(glow_alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_is_full_brightness() {
        assert_eq!(compute_light_level(12.0), 1.0);
    }

    #[test]
    fn midnight_is_dim() {
        assert_eq!(compute_light_level(0.0), 0.3);
    }

    #[test]
    fn dawn_is_between_night_and_day() {
        let level = compute_light_level(6.0);
        assert!(level > 0.3 && level < 1.0, "dawn level was {level}");
    }

    #[test]
    fn night_before_midnight_is_dim() {
        assert_eq!(compute_light_level(23.0), 0.3);
    }
}
