//! Floating status icons above agents.
//!
//! Reads: Agent, ActiveActions, EmotionalState, PhysicalNeeds, InConversation, Camera Projection
//! Writes: Text2d child entities (StatusIcon) spawned as children of agent root entities
//! Upstream: agent (actions, emotions, needs, conversation), world (spawning agents)
//! Downstream: Bevy renderer (visual overlay)

use crate::agent::actions::{ActionType, ActiveActions};
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::mind::conversation::InConversation;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::agent::Agent;
use bevy::prelude::*;

/// Camera scale above which status icons are hidden.
const HIDE_ZOOM_THRESHOLD: f32 = 2.5;

/// Y offset above the agent root transform where the icon floats.
const ICON_Y_OFFSET: f32 = 32.0;

/// Fear intensity above which the scared icon is shown.
const FEAR_THRESHOLD: f32 = 0.5;

/// Mood value above which the happy icon is shown.
const MOOD_HAPPY_THRESHOLD: f32 = 0.5;

/// Hunger level above which the hungry icon is shown (when not eating).
const HUNGER_THRESHOLD: f32 = 80.0;

pub struct StatusIconPlugin;

impl Plugin for StatusIconPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_status_icons, update_status_icons).chain());
    }
}

/// Marker component for the floating status icon child entity.
#[derive(Component)]
pub struct StatusIcon;

/// Spawns a `StatusIcon` child entity for each newly added agent.
fn spawn_status_icons(mut commands: Commands, new_agents: Query<Entity, Added<Agent>>) {
    for agent in new_agents.iter() {
        let icon = commands
            .spawn((
                Text2d::new(ICON_IDLE),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                Transform::from_translation(Vec3::new(0.0, ICON_Y_OFFSET, 2.0)),
                StatusIcon,
            ))
            .id();
        commands.entity(agent).add_child(icon);
    }
}

/// Updates each `StatusIcon` text and visibility based on the parent agent's state.
fn update_status_icons(
    agents: Query<
        (
            &ActiveActions,
            &EmotionalState,
            &PhysicalNeeds,
            Option<&InConversation>,
        ),
        With<Agent>,
    >,
    mut icons: Query<(&ChildOf, &mut Text2d, &mut Visibility), With<StatusIcon>>,
    cameras: Query<&Projection, With<Camera>>,
) {
    let far_zoom = cameras
        .iter()
        .any(|p| matches!(p, Projection::Orthographic(o) if o.scale > HIDE_ZOOM_THRESHOLD));

    for (parent, mut text, mut visibility) in icons.iter_mut() {
        let Ok((actions, emotions, needs, in_conversation)) = agents.get(parent.parent()) else {
            continue;
        };

        let target_vis = if far_zoom {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != target_vis {
            *visibility = target_vis;
        }

        if !far_zoom {
            let icon = status_icon(actions, emotions, needs, in_conversation);
            if text.0 != icon {
                text.0 = icon.to_string();
            }
        }
    }
}

// ─── Icon constants ────────────────────────────────────────────────────────

const ICON_SLEEP: &str = "zz";
const ICON_SCARED: &str = "!";
const ICON_TALKING: &str = "...";
const ICON_EATING: &str = "nom";
const ICON_DRINKING: &str = "sip";
const ICON_HARVESTING: &str = "get";
const ICON_BUILDING: &str = "build";
const ICON_HUNGRY: &str = "hungry";
const ICON_HAPPY: &str = ":)";
const ICON_IDLE: &str = ".";

// ─── Pure priority logic ───────────────────────────────────────────────────

/// Returns the highest-priority status icon string for the given agent state.
///
/// Priority (highest first):
/// 1. Sleeping
/// 2. Scared (Fear > FEAR_THRESHOLD)
/// 3. Talking (InConversation present)
/// 4. Eating
/// 5. Drinking
/// 6. Harvesting
/// 7. Building
/// 8. Hungry (hunger > HUNGER_THRESHOLD, not eating)
/// 9. Happy (mood > MOOD_HAPPY_THRESHOLD)
/// 10. Idle (fallback)
pub fn status_icon(
    actions: &ActiveActions,
    emotions: &EmotionalState,
    needs: &PhysicalNeeds,
    in_conversation: Option<&InConversation>,
) -> &'static str {
    if actions.contains(ActionType::Sleep) {
        return ICON_SLEEP;
    }
    if emotions.get_emotion_intensity(EmotionType::Fear) > FEAR_THRESHOLD {
        return ICON_SCARED;
    }
    if in_conversation.is_some() {
        return ICON_TALKING;
    }
    if actions.contains(ActionType::Eat) {
        return ICON_EATING;
    }
    if actions.contains(ActionType::Drink) {
        return ICON_DRINKING;
    }
    if actions.contains(ActionType::Harvest) {
        return ICON_HARVESTING;
    }
    if actions.contains(ActionType::Build) {
        return ICON_BUILDING;
    }
    if needs.hunger > HUNGER_THRESHOLD {
        return ICON_HUNGRY;
    }
    if emotions.current_mood > MOOD_HAPPY_THRESHOLD {
        return ICON_HAPPY;
    }
    ICON_IDLE
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::{ActionState, ActiveActions};
    use crate::agent::body::needs::PhysicalNeeds;
    use crate::agent::psyche::emotions::{Emotion, EmotionType, EmotionalState};

    fn idle_actions() -> ActiveActions {
        ActiveActions::default()
    }

    fn no_actions() -> ActiveActions {
        ActiveActions::empty()
    }

    fn neutral_emotions() -> EmotionalState {
        EmotionalState::default()
    }

    fn normal_needs() -> PhysicalNeeds {
        PhysicalNeeds::default()
    }

    #[test]
    fn sleeping_agent_shows_zzz() {
        let mut actions = no_actions();
        actions.insert(ActionState::new(ActionType::Sleep, 0));
        assert_eq!(
            status_icon(&actions, &neutral_emotions(), &normal_needs(), None),
            ICON_SLEEP
        );
    }

    #[test]
    fn scared_overrides_eating() {
        let mut actions = no_actions();
        actions.insert(ActionState::new(ActionType::Eat, 0));
        let mut emotions = neutral_emotions();
        emotions.add_emotion(Emotion::new(EmotionType::Fear, 0.8));
        assert_eq!(
            status_icon(&actions, &emotions, &normal_needs(), None),
            ICON_SCARED
        );
    }

    #[test]
    fn idle_agent_shows_thought_dot() {
        assert_eq!(
            status_icon(&idle_actions(), &neutral_emotions(), &normal_needs(), None),
            ICON_IDLE
        );
    }

    #[test]
    fn sleep_overrides_fear() {
        let mut actions = no_actions();
        actions.insert(ActionState::new(ActionType::Sleep, 0));
        let mut emotions = neutral_emotions();
        emotions.add_emotion(Emotion::new(EmotionType::Fear, 0.9));
        assert_eq!(
            status_icon(&actions, &emotions, &normal_needs(), None),
            ICON_SLEEP,
            "sleep must override fear"
        );
    }

    #[test]
    fn icon_updates_from_eating_to_sleeping() {
        let mut actions = no_actions();
        let emotions = neutral_emotions();
        let needs = normal_needs();

        actions.insert(ActionState::new(ActionType::Eat, 0));
        assert_eq!(status_icon(&actions, &emotions, &needs, None), ICON_EATING);

        actions.remove(ActionType::Eat);
        actions.insert(ActionState::new(ActionType::Sleep, 1));
        assert_eq!(status_icon(&actions, &emotions, &needs, None), ICON_SLEEP);
    }

    #[test]
    fn hungry_agent_shows_warning() {
        let mut needs = normal_needs();
        needs.hunger = 85.0;
        assert_eq!(
            status_icon(&idle_actions(), &neutral_emotions(), &needs, None),
            ICON_HUNGRY
        );
    }

    #[test]
    fn eating_suppresses_hungry_icon() {
        let mut actions = no_actions();
        actions.insert(ActionState::new(ActionType::Eat, 0));
        let mut needs = normal_needs();
        needs.hunger = 85.0;
        // Eating takes priority over hungry
        assert_eq!(
            status_icon(&actions, &neutral_emotions(), &needs, None),
            ICON_EATING
        );
    }

    #[test]
    fn happy_agent_shows_smile() {
        let mut emotions = neutral_emotions();
        emotions.current_mood = 0.8;
        assert_eq!(
            status_icon(&idle_actions(), &emotions, &normal_needs(), None),
            ICON_HAPPY
        );
    }

    #[test]
    fn talking_agent_shows_ellipsis() {
        use crate::agent::mind::conversation::InConversation;
        let dummy = InConversation { conversation_id: 0 };
        assert_eq!(
            status_icon(
                &idle_actions(),
                &neutral_emotions(),
                &normal_needs(),
                Some(&dummy)
            ),
            ICON_TALKING
        );
    }
}
