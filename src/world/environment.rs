use crate::core::GameTime;
use bevy::prelude::*;

pub struct EnvironmentPlugin;

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<LightLevel>()
            .init_resource::<LightLevel>()
            .add_systems(Update, (update_light_level, apply_visual_lighting));
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

fn update_light_level(time: Res<GameTime>, mut light: ResMut<LightLevel>) {
    // Simple day/night cycle
    // Day starts at 6:00 (6.0), Peak at 12:00 (12.0), Sunset at 18:00 (18.0)
    // Night is dark from 20:00 to 04:00

    let hour = time.hours as f32 + (time.minutes as f32 / 60.0);

    // Simple linear transition
    let target_light = if (6.0..18.0).contains(&hour) {
        // Day
        if hour < 8.0 {
            // Dawn (6-8): 0.2 -> 1.0
            0.2 + (hour - 6.0) * 0.4
        } else if hour > 16.0 {
            // Dusk (16-18): 1.0 -> 0.2
            1.0 - (hour - 16.0) * 0.4
        } else {
            // High Noon (8-16)
            1.0
        }
    } else {
        // Night
        0.1 // Not completely pitch black
    };

    light.0 = target_light;
}

fn apply_visual_lighting(light: Res<LightLevel>, mut clear_color: ResMut<ClearColor>) {
    // Interpolate background color based on light level
    // Day: Cornflower Blue-ish (0.4, 0.6, 0.9)
    // Night: Deep Midnight Blue (0.05, 0.05, 0.2)

    let day_color = Vec3::new(0.4, 0.6, 0.9);
    let night_color = Vec3::new(0.05, 0.05, 0.2);

    // Light 0.1 (Night) -> 1.0 (Day)
    // Map 0.1..1.0 to 0.0..1.0 for lerp
    let t = ((light.0 - 0.1) / 0.9).clamp(0.0, 1.0);

    let current = night_color.lerp(day_color, t);

    clear_color.0 = Color::srgb(current.x, current.y, current.z);
}
