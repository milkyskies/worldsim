//! Floating status icons above agents.
//!
//! Reads: Agent, ActiveActions, EmotionalState, PhysicalNeeds, InConversation,
//!        Cornered, Lame, Dazed, Body (bleed/wounds), Camera Projection
//! Writes: Text2d child entities (StatusIcon) spawned as children of agent root entities
//! Upstream: agent (actions, emotions, needs, conversation, condition flags)
//! Downstream: Bevy renderer (visual overlay)
//!
//! The icon picked is the highest-priority `Condition` for which the
//! agent currently qualifies. New conditions plug in by adding a row to
//! [`CONDITIONS`] — no fan-out across other systems.

use crate::agent::Agent;
use crate::agent::actions::{ActionType, ActiveActions};
use crate::agent::biology::body::Body;
use crate::agent::body::needs::PhysicalNeeds;
use crate::agent::mind::conversation::InConversation;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use bevy::prelude::*;

const HIDE_ZOOM_THRESHOLD: f32 = 2.5;
const ICON_Y_OFFSET: f32 = 32.0;
/// Tuned for 1× zoom. Keep small — these float over agent sprites.
const ICON_FONT_SIZE: f32 = 6.0;

const FEAR_THRESHOLD: f32 = 0.5;
const MOOD_HAPPY_THRESHOLD: f32 = 0.5;
const HUNGER_THRESHOLD: f32 = 0.8;
const COLD_THRESHOLD: f32 = 0.3;
const TIRED_AEROBIC_FRACTION: f32 = 0.2;

pub struct StatusIconPlugin;

impl Plugin for StatusIconPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_status_icons, update_status_icons).chain());
    }
}

#[derive(Component)]
pub struct StatusIcon;

fn spawn_status_icons(mut commands: Commands, new_agents: Query<Entity, Added<Agent>>) {
    for agent in new_agents.iter() {
        let icon = commands
            .spawn((
                Text2d::new(""),
                TextFont {
                    font_size: ICON_FONT_SIZE,
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

/// Per-agent context the condition predicates inspect.
pub struct ConditionContext<'a> {
    pub actions: &'a ActiveActions,
    pub emotions: &'a EmotionalState,
    pub needs: &'a PhysicalNeeds,
    pub body: Option<&'a Body>,
    pub in_conversation: Option<&'a InConversation>,
    pub cornered: bool,
    pub lame: bool,
    pub dazed: bool,
}

#[allow(clippy::too_many_arguments)]
fn update_status_icons(
    agents: Query<
        (
            &ActiveActions,
            &EmotionalState,
            &PhysicalNeeds,
            Option<&Body>,
            Option<&InConversation>,
            Option<&crate::agent::Cornered>,
            Option<&crate::agent::Lame>,
            Option<&crate::agent::Dazed>,
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
        let Ok((actions, emotions, needs, body, in_conversation, cornered, lame, dazed)) =
            agents.get(parent.parent())
        else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
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
            let ctx = ConditionContext {
                actions,
                emotions,
                needs,
                body,
                in_conversation,
                cornered: cornered.is_some(),
                lame: lame.is_some(),
                dazed: dazed.is_some(),
            };
            let icon = pick_icon(&ctx);
            if text.0 != icon {
                text.0 = icon.to_string();
            }
        }
    }
}

// ─── Condition registry ────────────────────────────────────────────────────

/// One row per condition. The first row whose predicate fires wins;
/// adding a new condition is one extra row, sorted by priority.
struct Condition {
    icon: &'static str,
    matches: fn(&ConditionContext) -> bool,
}

const CONDITIONS: &[Condition] = &[
    Condition {
        icon: "zz",
        matches: |ctx| ctx.actions.contains(ActionType::Sleep),
    },
    Condition {
        icon: "*",
        matches: |ctx| ctx.dazed,
    },
    Condition {
        icon: "X",
        matches: |ctx| ctx.cornered,
    },
    Condition {
        icon: "!",
        matches: |ctx| ctx.emotions.get_emotion_intensity(EmotionType::Fear) > FEAR_THRESHOLD,
    },
    Condition {
        icon: "...",
        matches: |ctx| ctx.in_conversation.is_some(),
    },
    Condition {
        icon: "nom",
        matches: |ctx| ctx.actions.contains(ActionType::Eat),
    },
    Condition {
        icon: "sip",
        matches: |ctx| ctx.actions.contains(ActionType::Drink),
    },
    Condition {
        icon: "get",
        matches: |ctx| ctx.actions.contains(ActionType::Harvest),
    },
    Condition {
        icon: "build",
        matches: |ctx| ctx.actions.contains(ActionType::Build),
    },
    Condition {
        icon: "limp",
        matches: |ctx| ctx.lame,
    },
    Condition {
        icon: "blood",
        matches: |ctx| {
            ctx.body
                .map(|b| {
                    b.parts
                        .iter()
                        .any(|p| p.injuries.iter().any(|i| i.bleed_rate > 0.0))
                })
                .unwrap_or(false)
        },
    },
    Condition {
        icon: "cold",
        matches: |ctx| ctx.needs.warmth.value < COLD_THRESHOLD,
    },
    Condition {
        icon: "tired",
        matches: |ctx| ctx.needs.stamina.aerobic_fraction() < TIRED_AEROBIC_FRACTION,
    },
    Condition {
        icon: "hungry",
        matches: |ctx| ctx.needs.hunger_urgency() > HUNGER_THRESHOLD,
    },
    Condition {
        icon: ":)",
        matches: |ctx| ctx.emotions.current_mood > MOOD_HAPPY_THRESHOLD,
    },
];

/// Idle fallback shown when no condition matches.
const ICON_IDLE: &str = ".";

pub fn pick_icon(ctx: &ConditionContext) -> &'static str {
    CONDITIONS
        .iter()
        .find(|c| (c.matches)(ctx))
        .map(|c| c.icon)
        .unwrap_or(ICON_IDLE)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::actions::{ActionState, ActiveActions};
    use crate::agent::psyche::emotions::Emotion;

    fn ctx_with(
        actions: ActiveActions,
        emotions: EmotionalState,
        needs: PhysicalNeeds,
    ) -> ConditionContext<'static> {
        // Leak the references — only used in unit tests, doesn't matter.
        let actions = Box::leak(Box::new(actions));
        let emotions = Box::leak(Box::new(emotions));
        let needs = Box::leak(Box::new(needs));
        ConditionContext {
            actions,
            emotions,
            needs,
            body: None,
            in_conversation: None,
            cornered: false,
            lame: false,
            dazed: false,
        }
    }

    #[test]
    fn sleeping_agent_shows_zzz() {
        let mut actions = ActiveActions::empty();
        actions.insert(ActionState::new(ActionType::Sleep, 0));
        assert_eq!(
            pick_icon(&ctx_with(
                actions,
                EmotionalState::default(),
                PhysicalNeeds::default()
            )),
            "zz"
        );
    }

    #[test]
    fn dazed_overrides_eating() {
        let mut actions = ActiveActions::empty();
        actions.insert(ActionState::new(ActionType::Eat, 0));
        let mut ctx = ctx_with(actions, EmotionalState::default(), PhysicalNeeds::default());
        ctx.dazed = true;
        assert_eq!(pick_icon(&ctx), "*");
    }

    #[test]
    fn cornered_overrides_fear() {
        let mut emotions = EmotionalState::default();
        emotions.add_emotion(Emotion::new(EmotionType::Fear, 0.9));
        let mut ctx = ctx_with(ActiveActions::empty(), emotions, PhysicalNeeds::default());
        ctx.cornered = true;
        assert_eq!(pick_icon(&ctx), "X");
    }

    #[test]
    fn scared_overrides_eating_when_not_cornered() {
        let mut actions = ActiveActions::empty();
        actions.insert(ActionState::new(ActionType::Eat, 0));
        let mut emotions = EmotionalState::default();
        emotions.add_emotion(Emotion::new(EmotionType::Fear, 0.8));
        assert_eq!(
            pick_icon(&ctx_with(actions, emotions, PhysicalNeeds::default())),
            "!"
        );
    }

    #[test]
    fn idle_agent_shows_thought_dot() {
        assert_eq!(
            pick_icon(&ctx_with(
                ActiveActions::default(),
                EmotionalState::default(),
                PhysicalNeeds::default()
            )),
            ICON_IDLE
        );
    }

    #[test]
    fn lame_shows_limp_when_no_combat_state() {
        let mut ctx = ctx_with(
            ActiveActions::default(),
            EmotionalState::default(),
            PhysicalNeeds::default(),
        );
        ctx.lame = true;
        assert_eq!(pick_icon(&ctx), "limp");
    }

    #[test]
    fn happy_agent_shows_smile() {
        let emotions = EmotionalState {
            current_mood: 0.8,
            ..Default::default()
        };
        assert_eq!(
            pick_icon(&ctx_with(
                ActiveActions::default(),
                emotions,
                PhysicalNeeds::default()
            )),
            ":)"
        );
    }

    #[test]
    fn hungry_agent_shows_warning() {
        let needs = PhysicalNeeds {
            metabolism: crate::agent::body::metabolism::Metabolism::at_urgency(0.85),
            ..Default::default()
        };
        assert_eq!(
            pick_icon(&ctx_with(
                ActiveActions::default(),
                EmotionalState::default(),
                needs
            )),
            "hungry"
        );
    }

    #[test]
    fn talking_agent_shows_ellipsis() {
        let dummy = InConversation { conversation_id: 0 };
        let actions = Box::leak(Box::new(ActiveActions::default()));
        let emotions = Box::leak(Box::new(EmotionalState::default()));
        let needs = Box::leak(Box::new(PhysicalNeeds::default()));
        let ctx = ConditionContext {
            actions,
            emotions,
            needs,
            body: None,
            in_conversation: Some(&dummy),
            cornered: false,
            lame: false,
            dazed: false,
        };
        assert_eq!(pick_icon(&ctx), "...");
    }
}
