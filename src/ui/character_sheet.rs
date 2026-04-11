//! Player-facing character sheet UI.
//!
//! Reads: UiState, DebugUiEnabled, Name, SpeciesProfile, PhysicalNeeds, Consciousness, PsychologicalDrives, EmotionalState, Personality, Skills, Body, BrainState, RationalBrain, CentralNervousSystem, MindGraph, WorkingMemory, RelationshipHistory, ItemSlots, ActiveActions
//! Writes: CharacterSheetState
//! Upstream: handle_game_click (populates selected entity)
//! Downstream: none - terminal UI

use bevy::prelude::*;
use bevy_egui::{EguiContext, EguiPrimaryContextPass, PrimaryEguiContext, egui};
use egui::Color32;

use crate::agent::actions::ActionType;
use crate::agent::actions::channel::Channel;
use crate::agent::actions::registry::ActiveActions;
use crate::agent::biology::body::{Body, BodyPart, InjuryType};
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::body::species::SpeciesProfile;
use crate::agent::brains::plan_memory::{PlanMemory, PlanState};
use crate::agent::brains::proposal::{BrainState, BrainType};
use crate::agent::events::ConversationTopic;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::conversation::{
    ConversationManager, InConversation, Intent as ConvIntent, Topic as ConvTopic,
};
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::agent::mind::memory::WorkingMemory;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::nervous_system::urgency::UrgencySource;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::agent::psyche::personality::{Personality, PersonalityTrait};
use crate::agent::psyche::relationships::{InteractionRecord, RelationshipHistory};
use crate::agent::skills::{SkillKind, Skills};
use crate::core::GameLog;
use crate::core::tick::TickCount;

use super::{DebugUiEnabled, UiState};

// ============================================================================
// THEME
// ============================================================================

const SEVERITY_GOOD: Color32 = Color32::from_rgb(80, 200, 100);
const SEVERITY_WARN: Color32 = Color32::from_rgb(220, 190, 60);
const SEVERITY_BAD: Color32 = Color32::from_rgb(220, 80, 60);

/// Pick a traffic-light color from a 0..1 value against two thresholds.
/// Values above `warn_above` are good, values below `bad_below` are bad,
/// anything in between is warn.
fn severity_color(value: f32, bad_below: f32, warn_above: f32) -> Color32 {
    if value < bad_below {
        SEVERITY_BAD
    } else if value < warn_above {
        SEVERITY_WARN
    } else {
        SEVERITY_GOOD
    }
}

// ============================================================================
// PLUGIN
// ============================================================================

pub struct CharacterSheetPlugin;

impl Plugin for CharacterSheetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CharacterSheetState>().add_systems(
            EguiPrimaryContextPass,
            character_sheet_system.run_if(crate::menu::sim_interactive),
        );
    }
}

// ============================================================================
// STATE
// ============================================================================

#[derive(Resource, Default)]
pub struct CharacterSheetState {
    /// Currently displayed tab in the right-side character panel.
    pub active_tab: CharSheetTab,
    /// User dismissed the panel for this specific entity. Cleared when the
    /// player selects a different agent so the next click reopens it.
    dismissed_for: Option<Entity>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CharSheetTab {
    #[default]
    Overview,
    Needs,
    Personality,
    Skills,
    Social,
    Health,
    Knowledge,
    Inventory,
    /// Recent activity log: SimEvents this agent appeared in.
    Activity,
    /// Developer-only tab showing raw brain proposals. Hidden unless F12 debug
    /// mode is on.
    Brain,
}

impl CharSheetTab {
    pub fn label(self) -> &'static str {
        match self {
            CharSheetTab::Overview => "Overview",
            CharSheetTab::Needs => "Needs",
            CharSheetTab::Personality => "Personality",
            CharSheetTab::Skills => "Skills",
            CharSheetTab::Social => "Social",
            CharSheetTab::Health => "Health",
            CharSheetTab::Knowledge => "Knowledge",
            CharSheetTab::Inventory => "Inventory",
            CharSheetTab::Activity => "Activity",
            CharSheetTab::Brain => "Brain",
        }
    }
}

// ============================================================================
// MAIN SYSTEM
// ============================================================================

fn character_sheet_system(world: &mut World) {
    let Ok(egui_context) = world
        .query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>()
        .single(world)
    else {
        return;
    };
    let mut egui_context = egui_context.clone();
    let ctx = egui_context.get_mut();

    let selected = world
        .resource::<UiState>()
        .selected_entities
        .as_slice()
        .first()
        .copied();

    // Reset the per-entity dismissal whenever the selected agent changes so
    // the next click on a fresh agent reopens the panel.
    {
        let mut cs = world.resource_mut::<CharacterSheetState>();
        if cs.dismissed_for != selected {
            cs.dismissed_for = None;
        }
    }

    let Some(entity) = selected else {
        return;
    };
    if world.get_entity(entity).is_err() {
        return;
    }
    if world.resource::<CharacterSheetState>().dismissed_for == Some(entity) {
        return;
    }

    let debug_enabled = world.resource::<DebugUiEnabled>().0;
    let visible_tabs = visible_tabs_for_entity(world, entity, debug_enabled);
    if visible_tabs.is_empty() {
        return;
    }

    let active_tab = {
        let cs = world.resource::<CharacterSheetState>();
        if visible_tabs.contains(&cs.active_tab) {
            cs.active_tab
        } else {
            visible_tabs[0]
        }
    };

    let agent_name = world
        .get::<Name>(entity)
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("{:?}", entity));
    let species_label = world
        .get::<SpeciesProfile>(entity)
        .map(|s| format!("{:?}", s.species))
        .unwrap_or_else(|| "Unknown".to_string());
    let summary = build_header_summary(world, entity);

    let mut new_tab = active_tab;
    let mut dismiss = false;

    egui::SidePanel::right("character_sheet_panel")
        .resizable(true)
        .default_width(360.0)
        .min_width(280.0)
        .max_width(360.0)
        .show(ctx, |ui| {
            // Header
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(&agent_name);
                ui.label(
                    egui::RichText::new(format!("({})", species_label)).color(Color32::LIGHT_GRAY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Close panel").clicked() {
                        dismiss = true;
                    }
                });
            });

            ui.horizontal(|ui| {
                ui.colored_label(summary.mood_color, &summary.mood_label);
                ui.separator();
                ui.label(egui::RichText::new(&summary.action_text).italics());
            });

            ui.horizontal(|ui| {
                ui.label("❤");
                ui.add(
                    egui::ProgressBar::new(summary.hp_pct)
                        .desired_width(110.0)
                        .text(format!("{:.0}", summary.hp_pct * 100.0)),
                );
                ui.label("⚡");
                ui.add(
                    egui::ProgressBar::new(summary.stamina_pct)
                        .desired_width(110.0)
                        .text(format!("{:.0}", summary.stamina_pct * 100.0)),
                );
            });

            ui.add_space(4.0);
            ui.separator();

            // Tab strip
            ui.horizontal_wrapped(|ui| {
                for tab in &visible_tabs {
                    if ui.selectable_label(*tab == new_tab, tab.label()).clicked() {
                        new_tab = *tab;
                    }
                }
            });
            ui.separator();

            // Tab content
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| match new_tab {
                    CharSheetTab::Overview => render_overview(ui, world, entity),
                    CharSheetTab::Needs => render_needs(ui, world, entity),
                    CharSheetTab::Personality => render_personality(ui, world, entity),
                    CharSheetTab::Skills => render_skills(ui, world, entity),
                    CharSheetTab::Social => render_social(ui, world, entity),
                    CharSheetTab::Health => render_health(ui, world, entity),
                    CharSheetTab::Knowledge => render_knowledge(ui, world, entity),
                    CharSheetTab::Inventory => render_inventory(ui, world, entity),
                    CharSheetTab::Activity => render_activity(ui, world, entity),
                    CharSheetTab::Brain => render_brain(ui, world, entity),
                });
        });

    let mut cs = world.resource_mut::<CharacterSheetState>();
    cs.active_tab = new_tab;
    if dismiss {
        cs.dismissed_for = Some(entity);
    }
}

// ============================================================================
// HEADER SUMMARY
// ============================================================================

struct HeaderSummary {
    mood_label: String,
    mood_color: Color32,
    action_text: String,
    hp_pct: f32,
    stamina_pct: f32,
}

fn build_header_summary(world: &World, entity: Entity) -> HeaderSummary {
    let (mood_label, mood_color) = world
        .get::<EmotionalState>(entity)
        .map(mood_descriptor)
        .unwrap_or_else(|| ("Unknown".into(), Color32::GRAY));

    let action_text = current_action_summary(world, entity);

    let (hp_pct, stamina_pct) = world
        .get::<PhysicalNeeds>(entity)
        .map(|n| (n.health / 100.0, n.stamina.aerobic_fraction()))
        .unwrap_or((0.0, 0.0));

    HeaderSummary {
        mood_label,
        mood_color,
        action_text,
        hp_pct,
        stamina_pct,
    }
}

// ============================================================================
// TAB VISIBILITY
// ============================================================================

fn visible_tabs_for_entity(
    world: &World,
    entity: Entity,
    debug_enabled: bool,
) -> Vec<CharSheetTab> {
    let mut tabs = Vec::new();

    // Overview - always visible if agent has any core components
    if world.get::<EmotionalState>(entity).is_some()
        || world.get::<PhysicalNeeds>(entity).is_some()
        || world.get::<ActiveActions>(entity).is_some()
    {
        tabs.push(CharSheetTab::Overview);
    }

    if world.get::<PhysicalNeeds>(entity).is_some() {
        tabs.push(CharSheetTab::Needs);
    }

    if world.get::<Personality>(entity).is_some() {
        tabs.push(CharSheetTab::Personality);
    }

    if world.get::<Skills>(entity).is_some() {
        tabs.push(CharSheetTab::Skills);
    }

    // Social: has interaction history OR any known entities in MindGraph
    let has_history = world
        .get::<RelationshipHistory>(entity)
        .map(|r| !r.logs.is_empty())
        .unwrap_or(false);
    let has_known = world
        .get::<MindGraph>(entity)
        .map(|m| {
            !m.query(None, Some(Predicate::Knows), Some(&Value::Boolean(true)))
                .is_empty()
        })
        .unwrap_or(false);
    if has_history || has_known {
        tabs.push(CharSheetTab::Social);
    }

    if world.get::<Body>(entity).is_some() {
        tabs.push(CharSheetTab::Health);
    }

    if world.get::<MindGraph>(entity).is_some() {
        tabs.push(CharSheetTab::Knowledge);
    }

    let has_items = world
        .get::<ItemSlots>(entity)
        .map(|i| i.all_items().next().is_some())
        .unwrap_or(false);
    if has_items {
        tabs.push(CharSheetTab::Inventory);
    }

    // Activity is always available — even a fresh agent has at least the
    // spawn event in the GameLog.
    tabs.push(CharSheetTab::Activity);

    if debug_enabled && world.get::<BrainState>(entity).is_some() {
        tabs.push(CharSheetTab::Brain);
    }

    tabs
}

// ============================================================================
// OVERVIEW TAB
// ============================================================================

fn render_overview(ui: &mut egui::Ui, world: &World, entity: Entity) {
    // Position
    if let Some(transform) = world.get::<Transform>(entity) {
        let pos = transform.translation.truncate();
        ui.label(
            egui::RichText::new(format!("({:.0}, {:.0})", pos.x, pos.y))
                .color(egui::Color32::GRAY)
                .small(),
        );
    }

    // Current action + target
    ui.heading("Current Action");
    ui.label(current_action_summary(world, entity));

    // Why (from winning brain proposal)
    if let Some(brain) = world.get::<BrainState>(entity)
        && let Some(reason) = winning_reasoning(brain)
    {
        ui.label(egui::RichText::new(format!("Why: {}", reason)).italics());
    }
    ui.separator();

    // Plan steps — show every Executing plan's remaining steps and the
    // top Considering / Background plan's outline so the HUD reflects
    // the full cognitive memory instead of a single active plan.
    if let Some(memory) = world.get::<PlanMemory>(entity) {
        let executing: Vec<_> = memory.in_state(PlanState::Executing).collect();
        if !executing.is_empty() {
            ui.heading("Executing plans");
            for plan in executing {
                if plan.steps.is_empty() {
                    continue;
                }
                for (i, step) in plan.steps.iter().enumerate() {
                    let is_current = i == plan.current_step;
                    let is_done = i < plan.current_step;
                    let step_text = format!("{}. {}", i + 1, step.name);
                    ui.horizontal(|ui| {
                        if is_current {
                            ui.colored_label(Color32::YELLOW, "▶");
                            ui.strong(&step_text);
                        } else if is_done {
                            ui.colored_label(Color32::GRAY, "✓");
                            ui.label(
                                egui::RichText::new(&step_text)
                                    .strikethrough()
                                    .color(Color32::GRAY),
                            );
                        } else {
                            ui.label(&step_text);
                        }
                    });
                }
            }
            ui.separator();
        }
        let background_count = memory.count_state(PlanState::Background)
            + memory.count_state(PlanState::Considering)
            + memory.count_state(PlanState::Suspended);
        if background_count > 0 {
            ui.label(
                egui::RichText::new(format!("{background_count} plan(s) in background")).italics(),
            );
            ui.separator();
        }
    }

    if let Some(emotions) = world.get::<EmotionalState>(entity) {
        ui.heading("Emotions");
        if emotions.active_emotions.is_empty() {
            ui.label(egui::RichText::new("No strong feelings").italics());
        } else {
            let mut sorted: Vec<_> = emotions.active_emotions.iter().collect();
            sorted.sort_by(|a, b| {
                b.intensity
                    .partial_cmp(&a.intensity)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for emo in &sorted {
                ui.horizontal(|ui| {
                    let (label, color) = emotion_label_color(emo.emotion_type, emo.intensity);
                    ui.colored_label(color, format!("{:?}", emo.emotion_type));
                    ui.add(
                        egui::ProgressBar::new(emo.intensity)
                            .desired_width(160.0)
                            .text(label),
                    );
                });
            }
        }
        ui.separator();

        // Mood and stress
        let mood = emotions.current_mood;
        let mood_label = mood_text(mood);
        let mood_color = if mood > 0.0 {
            Color32::GREEN
        } else if mood < 0.0 {
            Color32::RED
        } else {
            Color32::GRAY
        };
        ui.horizontal(|ui| {
            ui.label("Mood:");
            ui.colored_label(mood_color, mood_label);
            ui.label(format!("({:+.2})", mood));
        });

        let stress = emotions.stress_level;
        let stress_color = stress_color(stress);
        ui.horizontal(|ui| {
            ui.label("Stress:");
            ui.add(
                egui::ProgressBar::new(stress / 100.0)
                    .desired_width(160.0)
                    .fill(stress_color)
                    .text(format!("{:.0}", stress)),
            );
        });
    }
}

// ============================================================================
// NEEDS TAB
// ============================================================================

fn render_needs(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let urgencies = world
        .get::<CentralNervousSystem>(entity)
        .map(|cns| cns.urgencies.as_slice())
        .unwrap_or(&[]);
    let urgency_for = |source: UrgencySource| -> Option<f32> {
        urgencies
            .iter()
            .find(|u| u.source == source)
            .map(|u| u.value)
    };

    let needs = world.get::<PhysicalNeeds>(entity);
    let consc = world.get::<Consciousness>(entity);
    let drives = world.get::<PsychologicalDrives>(entity);

    // ── Stomach / metabolism ────────────────────────────────────────────
    // Everything tied to food and fuel: stomach content, short-term
    // glucose, long-term reserves. Hunger urgency surfaces under Glucose
    // because that's the primary pool the brain's hunger drive watches.
    egui::CollapsingHeader::new("Stomach")
        .default_open(true)
        .show(ui, |ui| {
            if let Some(needs) = needs {
                need_bar(
                    ui,
                    "Stomach",
                    needs.metabolism.stomach_fullness(),
                    crate::agent::body::metabolism::STOMACH_CAPACITY,
                    None,
                );
                need_bar(
                    ui,
                    "Glucose",
                    needs.metabolism.glucose,
                    crate::agent::body::metabolism::GLUCOSE_MAX,
                    urgency_for(UrgencySource::Hunger),
                );
                need_bar(
                    ui,
                    "Reserves",
                    needs.metabolism.reserves,
                    crate::agent::body::metabolism::RESERVES_MAX,
                    None,
                );
            }
        });

    // ── Body ────────────────────────────────────────────────────────────
    // Physical condition: fluids (thirst), stamina pools, health.
    // Thirst urgency surfaces here next to the raw thirst value.
    egui::CollapsingHeader::new("Body")
        .default_open(true)
        .show(ui, |ui| {
            if let Some(needs) = needs {
                need_bar(
                    ui,
                    "Thirst",
                    needs.thirst,
                    100.0,
                    urgency_for(UrgencySource::Thirst),
                );
                need_bar(
                    ui,
                    "Aerobic",
                    needs.stamina.aerobic,
                    needs.stamina.aerobic_max,
                    urgency_for(UrgencySource::Stamina),
                );
                need_bar(
                    ui,
                    "Anaerobic",
                    needs.stamina.anaerobic,
                    needs.stamina.anaerobic_max,
                    None,
                );
                need_bar(ui, "Health", needs.health, 100.0, None);
            }
        });

    // ── Mind ────────────────────────────────────────────────────────────
    // Cognitive state and individual-facing drives: alertness is how
    // clear-headed the agent is right now, curiosity and fun are the
    // internal pressures toward novelty and play.
    egui::CollapsingHeader::new("Mind")
        .default_open(true)
        .show(ui, |ui| {
            if let Some(consc) = consc {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Alertness").strong());
                    ui.add(
                        egui::ProgressBar::new(consc.alertness)
                            .desired_width(180.0)
                            .text(format!("{:.2}", consc.alertness)),
                    );
                });
            }
            if let Some(drives) = drives {
                drive_bar(
                    ui,
                    "Curiosity",
                    drives.curiosity,
                    urgency_for(UrgencySource::Curiosity),
                );
                drive_bar(ui, "Fun", drives.fun, urgency_for(UrgencySource::Fun));
                drive_bar(ui, "Autonomy", drives.autonomy, None);
            }
        });

    // ── Social ──────────────────────────────────────────────────────────
    // Outward-facing drives: being with others, standing in the group,
    // holding territory. Social urgency surfaces under Social drive.
    egui::CollapsingHeader::new("Social")
        .default_open(true)
        .show(ui, |ui| {
            if let Some(drives) = drives {
                drive_bar(
                    ui,
                    "Social",
                    drives.social,
                    urgency_for(UrgencySource::Social),
                );
                drive_bar(ui, "Status", drives.status, None);
                drive_bar(ui, "Security", drives.security, None);
                drive_bar(
                    ui,
                    "Territoriality",
                    drives.territoriality,
                    urgency_for(UrgencySource::Territoriality),
                );
            }
        });
}

/// Draws a need bar with the raw need value (e.g. hunger 0..100). When the
/// nervous system has emitted an urgency for this drive, the score is shown
/// as a thin secondary bar underneath — that's the brain's actual opinion of
/// how much it cares right now, not a hand-picked threshold.
fn need_bar(ui: &mut egui::Ui, label: &str, value: f32, max: f32, urgency: Option<f32>) {
    let pct = (value / max).clamp(0.0, 1.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).strong());
        ui.add(
            egui::ProgressBar::new(pct)
                .desired_width(180.0)
                .text(format!("{:.0}/{:.0}", value, max)),
        );
    });
    if let Some(u) = urgency {
        urgency_subbar(ui, u);
    }
}

fn drive_bar(ui: &mut egui::Ui, label: &str, value: f32, urgency: Option<f32>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).strong());
        ui.add(
            egui::ProgressBar::new(value.clamp(0.0, 1.0))
                .desired_width(180.0)
                .text(format!("{:.2}", value)),
        );
    });
    if let Some(u) = urgency {
        urgency_subbar(ui, u);
    }
}

/// Secondary thin bar showing the live urgency score from the CNS for the
/// drive above. This is what the brain actually thinks about the need right
/// now, after curves, sensitivities, modifiers and gating.
fn urgency_subbar(ui: &mut egui::Ui, urgency: f32) {
    let normalized = urgency.clamp(0.0, 1.0);
    let color = severity_color(1.0 - normalized, 0.3, 0.6);
    ui.horizontal(|ui| {
        ui.add_space(28.0);
        ui.label(
            egui::RichText::new("urgency")
                .small()
                .color(Color32::LIGHT_GRAY),
        );
        ui.add(
            egui::ProgressBar::new(normalized)
                .desired_width(150.0)
                .fill(color)
                .text(egui::RichText::new(format!("{:.2}", urgency)).small()),
        );
    });
}

// ============================================================================
// PERSONALITY TAB
// ============================================================================

fn render_personality(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let Some(p) = world.get::<Personality>(entity) else {
        ui.label("No personality data.");
        return;
    };
    ui.heading("Big Five Traits");
    ui.add_space(4.0);

    for trait_kind in PersonalityTrait::ALL {
        trait_block(
            ui,
            trait_kind.display_name(),
            trait_kind.get(&p.traits),
            &trait_kind.descriptions(),
        );
    }
}

fn trait_block(ui: &mut egui::Ui, name: &str, value: f32, descriptions: &[&str; 3]) {
    let bucket = if value < 0.33 {
        0
    } else if value < 0.67 {
        1
    } else {
        2
    };
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.strong(name);
            ui.add(
                egui::ProgressBar::new(value.clamp(0.0, 1.0))
                    .desired_width(160.0)
                    .text(format!("{:.2}", value)),
            );
        });
        ui.label(
            egui::RichText::new(descriptions[bucket])
                .italics()
                .color(Color32::LIGHT_GRAY),
        );
    });
    ui.add_space(2.0);
}

// ============================================================================
// SKILLS TAB
// ============================================================================

/// Rimworld-style scale the 0..1 internal float is rendered against. The
/// simulation keeps the continuous representation (decay math, multipliers,
/// personality modulation all prefer a normalised factor); the character
/// sheet discretises it into a 0..20 level plus a progress-to-next-level
/// bar for readability.
const SKILL_DISPLAY_LEVELS: u32 = 20;

fn render_skills(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let Some(skills) = world.get::<Skills>(entity) else {
        ui.label("No skills data.");
        return;
    };

    ui.heading("Skills");
    ui.label(
        egui::RichText::new(format!(
            "0-{SKILL_DISPLAY_LEVELS} scale; diminishing returns near mastery"
        ))
        .small()
        .color(Color32::LIGHT_GRAY),
    );
    ui.add_space(4.0);

    for kind in SkillKind::ALL {
        skill_block(ui, kind.display_name(), skills.level(kind));
    }
}

/// Tier label for a displayed level. Rough Rimworld-ish bands so the eye
/// can jump straight to "this one is a master" without counting pips.
fn skill_tier_label(display_level: u32) -> &'static str {
    match display_level {
        0..=2 => "Untrained",
        3..=5 => "Novice",
        6..=9 => "Apprentice",
        10..=13 => "Skilled",
        14..=17 => "Expert",
        _ => "Master",
    }
}

fn skill_block(ui: &mut egui::Ui, name: &str, level: f32) {
    let scaled = level.clamp(0.0, 1.0) * SKILL_DISPLAY_LEVELS as f32;
    let display_level = (scaled.floor() as u32).min(SKILL_DISPLAY_LEVELS);
    let progress_in_level = if display_level >= SKILL_DISPLAY_LEVELS {
        1.0
    } else {
        scaled - scaled.floor()
    };

    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.strong(name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("Lv {display_level}/{SKILL_DISPLAY_LEVELS}"))
                        .strong(),
                );
            });
        });
        ui.add(
            egui::ProgressBar::new(progress_in_level)
                .desired_width(200.0)
                .text(format!("{:.0}%", progress_in_level * 100.0)),
        );
        ui.label(
            egui::RichText::new(format!(
                "{}  ({:.3} raw)",
                skill_tier_label(display_level),
                level
            ))
            .italics()
            .small()
            .color(Color32::LIGHT_GRAY),
        );
    });
    ui.add_space(2.0);
}

// ============================================================================
// SOCIAL TAB
// ============================================================================

fn render_social(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let now = world
        .get_resource::<TickCount>()
        .map(|t| t.current)
        .unwrap_or(0);

    render_current_conversation(ui, world, entity, now);

    ui.heading("Relationships");

    let Some(mind) = world.get::<MindGraph>(entity) else {
        ui.label("No social knowledge.");
        return;
    };

    let known = mind.query(None, Some(Predicate::Knows), Some(&Value::Boolean(true)));
    if known.is_empty() {
        ui.label(egui::RichText::new("Has not met anyone yet.").italics());
        return;
    }

    let history = world.get::<RelationshipHistory>(entity);

    let mut rows: Vec<SocialRow> = Vec::new();
    for triple in known {
        let Node::Entity(other) = triple.subject else {
            continue;
        };

        let name = world
            .get::<Name>(other)
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("{:?}", other));

        let category = relationship_category(mind, other);
        let trust = query_float(mind, other, Predicate::Trust).unwrap_or(0.5);
        let affection = query_float(mind, other, Predicate::Affection).unwrap_or(0.5);
        let respect = query_float(mind, other, Predicate::Respect).unwrap_or(0.5);

        let recent_interactions: Vec<InteractionRecord> = history
            .map(|h| h.get(other).iter().rev().take(5).cloned().collect())
            .unwrap_or_default();

        rows.push(SocialRow {
            name,
            category,
            trust,
            affection,
            respect,
            recent_interactions,
        });
    }

    rows.sort_by(|a, b| {
        b.trust
            .partial_cmp(&a.trust)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for (idx, row) in rows.iter().enumerate() {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong(&row.name);
                let (label, color) = category_label_color(row.category);
                ui.colored_label(color, label);
            });
            egui::Grid::new(egui::Id::new(("rel_grid", idx)))
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Trust");
                    ui.add(
                        egui::ProgressBar::new(row.trust)
                            .desired_width(180.0)
                            .text(format!("{:.2}", row.trust)),
                    );
                    ui.end_row();

                    ui.label("Affection");
                    ui.add(
                        egui::ProgressBar::new(row.affection)
                            .desired_width(180.0)
                            .text(format!("{:.2}", row.affection)),
                    );
                    ui.end_row();

                    ui.label("Respect");
                    ui.add(
                        egui::ProgressBar::new(row.respect)
                            .desired_width(180.0)
                            .text(format!("{:.2}", row.respect)),
                    );
                    ui.end_row();
                });
            if !row.recent_interactions.is_empty() {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Recent interactions")
                        .small()
                        .color(Color32::LIGHT_GRAY),
                );
                for record in &row.recent_interactions {
                    let age = now.saturating_sub(record.tick);
                    let topic = record
                        .topic
                        .map(conversation_topic_label)
                        .unwrap_or("contact");
                    let valence_label = if record.valence > 0.3 {
                        "+"
                    } else if record.valence < -0.3 {
                        "-"
                    } else {
                        "·"
                    };
                    let valence_color = if record.valence > 0.3 {
                        Color32::from_rgb(140, 220, 140)
                    } else if record.valence < -0.3 {
                        Color32::from_rgb(220, 120, 120)
                    } else {
                        Color32::LIGHT_GRAY
                    };
                    ui.horizontal(|ui| {
                        ui.colored_label(valence_color, valence_label);
                        ui.label(egui::RichText::new(format!("{} ({}t ago)", topic, age)).small());
                    });
                }
            }
        });
        ui.add_space(2.0);
    }
}

fn render_current_conversation(ui: &mut egui::Ui, world: &World, entity: Entity, now: u64) {
    let Some(in_conv) = world.get::<InConversation>(entity) else {
        return;
    };
    let Some(manager) = world.get_resource::<ConversationManager>() else {
        return;
    };
    let Some(conv) = manager.get(in_conv.conversation_id) else {
        return;
    };

    ui.heading("💬 In Conversation");
    ui.group(|ui| {
        let participants: Vec<String> = conv
            .participants
            .iter()
            .filter(|p| **p != entity)
            .map(|p| {
                world
                    .get::<Name>(*p)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| format!("{:?}", p))
            })
            .collect();
        ui.label(format!("Talking to: {}", participants.join(", ")));
        ui.label(
            egui::RichText::new(format!(
                "{:?} · started {}t ago",
                conv.state,
                now.saturating_sub(conv.started_at)
            ))
            .small()
            .color(Color32::LIGHT_GRAY),
        );

        if !conv.turns.is_empty() {
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Recent turns")
                    .small()
                    .color(Color32::LIGHT_GRAY),
            );
            for turn in conv.turns.iter().rev().take(5).rev() {
                render_conversation_turn(ui, world, turn, now);
            }
        }
    });
    ui.add_space(4.0);
}

fn render_conversation_turn(
    ui: &mut egui::Ui,
    world: &World,
    turn: &crate::agent::mind::conversation::Turn,
    now: u64,
) {
    let speaker_name = world
        .get::<Name>(turn.speaker)
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("{:?}", turn.speaker));
    let intent = conv_intent_label(turn.intent);
    let topic = conv_topic_label(world, turn.topic);
    let age = now.saturating_sub(turn.timestamp);
    ui.label(
        egui::RichText::new(format!(
            "• {} {} {} ({}t ago)",
            speaker_name, intent, topic, age
        ))
        .small(),
    );
}

fn conv_intent_label(intent: ConvIntent) -> &'static str {
    match intent {
        ConvIntent::Greet => "greets",
        ConvIntent::Ask => "asks about",
        ConvIntent::Answer => "answers about",
        ConvIntent::Share => "shares",
        ConvIntent::Empathize => "empathizes about",
        ConvIntent::Agree => "agrees about",
        ConvIntent::Disagree => "disagrees about",
        ConvIntent::Thank => "thanks for",
        ConvIntent::Farewell => "says goodbye",
        ConvIntent::Acknowledge => "acknowledges",
    }
}

fn conv_topic_label(world: &World, topic: ConvTopic) -> String {
    match topic {
        ConvTopic::General => "small talk".to_string(),
        ConvTopic::Help => "help".to_string(),
        ConvTopic::Location(c) => format!("{:?}", c),
        ConvTopic::State(e) | ConvTopic::Person(e) => world
            .get::<Name>(e)
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("{:?}", e)),
    }
}

fn conversation_topic_label(topic: ConversationTopic) -> &'static str {
    match topic {
        ConversationTopic::Greetings => "greeted",
        ConversationTopic::Knowledge => "shared knowledge",
        ConversationTopic::Feelings => "shared feelings",
        ConversationTopic::Gossip => "gossiped",
        ConversationTopic::Request => "asked help",
    }
}

struct SocialRow {
    name: String,
    category: RelCategory,
    trust: f32,
    affection: f32,
    respect: f32,
    recent_interactions: Vec<InteractionRecord>,
}

#[derive(Clone, Copy)]
enum RelCategory {
    Stranger,
    Acquaintance,
    Friend,
    Rival,
    Enemy,
}

fn relationship_category(mind: &MindGraph, other: Entity) -> RelCategory {
    // Check explicit category concepts in MindGraph
    let is_concept = |c: Concept| {
        !mind
            .query(
                Some(&Node::Entity(other)),
                Some(Predicate::IsA),
                Some(&Value::Concept(c)),
            )
            .is_empty()
    };
    if is_concept(Concept::Enemy) {
        RelCategory::Enemy
    } else if is_concept(Concept::Rival) {
        RelCategory::Rival
    } else if is_concept(Concept::Friend) {
        RelCategory::Friend
    } else if is_concept(Concept::Acquaintance) {
        RelCategory::Acquaintance
    } else {
        RelCategory::Stranger
    }
}

fn category_label_color(cat: RelCategory) -> (&'static str, Color32) {
    match cat {
        RelCategory::Friend => ("Friend", Color32::from_rgb(100, 220, 140)),
        RelCategory::Acquaintance => ("Acquaintance", Color32::LIGHT_GRAY),
        RelCategory::Rival => ("Rival", Color32::from_rgb(220, 180, 60)),
        RelCategory::Enemy => ("Enemy", Color32::from_rgb(220, 80, 60)),
        RelCategory::Stranger => ("Stranger", Color32::GRAY),
    }
}

fn query_float(mind: &MindGraph, other: Entity, predicate: Predicate) -> Option<f32> {
    mind.query(Some(&Node::Entity(other)), Some(predicate), None)
        .into_iter()
        .find_map(|t| match t.object {
            Value::Float(f) => Some(f),
            _ => None,
        })
}

// ============================================================================
// HEALTH TAB
// ============================================================================

fn render_health(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let Some(body) = world.get::<Body>(entity) else {
        ui.label("No body data.");
        return;
    };

    ui.heading("Anatomy");

    egui::Grid::new("anatomy_grid")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Part");
            ui.strong("Condition");
            ui.strong("Status");
            ui.end_row();

            for part in body.parts() {
                render_body_part_row(ui, part);
            }
        });

    ui.separator();
    ui.heading("Capabilities");
    capability_bar(ui, "Mobility", body.channel_capacity(Channel::Locomotion));
    capability_bar(
        ui,
        "Manipulation",
        body.channel_capacity(Channel::Manipulation),
    );
    if body.is_incapacitated() {
        ui.colored_label(
            Color32::RED,
            egui::RichText::new("⚠ Incapacitated").strong(),
        );
    }

    let has_injuries = body.parts().any(|p| !p.injuries.is_empty());
    if has_injuries {
        ui.separator();
        ui.heading("Injuries");
        for part in body.parts() {
            for injury in &part.injuries {
                let kind = injury_label(injury.injury_type);
                let healed_pct = (injury.healed_amount * 100.0).min(100.0);
                ui.horizontal(|ui| {
                    ui.label(format!("{} (severity {:.1})", kind, injury.severity));
                    ui.add(
                        egui::ProgressBar::new(injury.healed_amount.clamp(0.0, 1.0))
                            .desired_width(140.0)
                            .text(format!("healed {:.0}%", healed_pct)),
                    );
                });
            }
        }
    }
}

fn injury_label(kind: InjuryType) -> &'static str {
    match kind {
        InjuryType::Cut => "Cut",
        InjuryType::Bruise => "Bruise",
        InjuryType::Fracture => "Fracture",
        InjuryType::Burn => "Burn",
        InjuryType::Infection => "Infection",
        InjuryType::Pierce => "Pierce",
        InjuryType::Slash => "Slash",
        InjuryType::Crush => "Crush",
    }
}

fn render_body_part_row(ui: &mut egui::Ui, part: &BodyPart) {
    ui.label(part.name());
    let hp_pct = (part.current_hp / part.max_hp).clamp(0.0, 1.0);
    ui.add(
        egui::ProgressBar::new(hp_pct)
            .desired_width(140.0)
            .fill(severity_color(hp_pct, 0.4, 0.7))
            .text(format!("{:.0}/{:.0}", part.current_hp, part.max_hp)),
    );
    if part.injuries.is_empty() {
        ui.label("OK");
    } else {
        ui.colored_label(Color32::RED, format!("{} inj", part.injuries.len()));
    }
    ui.end_row();
}

fn capability_bar(ui: &mut egui::Ui, label: &str, value: f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::ProgressBar::new(value.clamp(0.0, 1.0))
                .desired_width(200.0)
                .fill(severity_color(value, 0.4, 0.7))
                .text(format!("{:.0}%", value * 100.0)),
        );
    });
}

// ============================================================================
// KNOWLEDGE TAB
// ============================================================================

fn render_knowledge(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let Some(mind) = world.get::<MindGraph>(entity) else {
        ui.label("No knowledge.");
        return;
    };

    // Places I know — things with LocatedAt
    ui.collapsing("📍 Places I know", |ui| {
        let located = mind.query(None, Some(Predicate::LocatedAt), None);
        if located.is_empty() {
            ui.label(egui::RichText::new("None").italics());
        } else {
            for triple in located.iter().take(30) {
                if let Value::Tile(pos) = triple.object {
                    let subject_label = node_label(world, &triple.subject);
                    ui.label(format!("• {} at ({}, {})", subject_label, pos.0, pos.1));
                }
            }
        }
    });

    // People I know
    ui.collapsing("👤 People I know", |ui| {
        let known = mind.query(None, Some(Predicate::Knows), Some(&Value::Boolean(true)));
        if known.is_empty() {
            ui.label(egui::RichText::new("None").italics());
        } else {
            for triple in &known {
                if let Node::Entity(other) = triple.subject {
                    let name = world
                        .get::<Name>(other)
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| format!("{:?}", other));
                    let cat = relationship_category(mind, other);
                    let (label, color) = category_label_color(cat);
                    ui.horizontal(|ui| {
                        ui.label(format!("• {}", name));
                        ui.colored_label(color, format!("({})", label));
                    });
                }
            }
        }
    });

    // Dangers
    ui.collapsing("⚠ Dangers", |ui| {
        let dangerous = mind.query(
            None,
            Some(Predicate::HasTrait),
            Some(&Value::Concept(Concept::Dangerous)),
        );
        if dangerous.is_empty() {
            ui.label(egui::RichText::new("No known dangers").italics());
        } else {
            for triple in &dangerous {
                let label = node_label(world, &triple.subject);
                ui.label(format!("• {}", label));
            }
        }
    });

    // Beliefs — HasTrait triples
    ui.collapsing("💭 Beliefs", |ui| {
        let beliefs = mind.query(None, Some(Predicate::HasTrait), None);
        if beliefs.is_empty() {
            ui.label(egui::RichText::new("No beliefs yet").italics());
        } else {
            for triple in beliefs.iter().take(30) {
                if let (Node::Entity(_) | Node::Concept(_), Value::Concept(c)) =
                    (&triple.subject, &triple.object)
                {
                    let subj = node_label(world, &triple.subject);
                    ui.label(format!("• {} is {:?}", subj, c));
                }
            }
        }
    });

    // Memories from working memory
    if let Some(wm) = world.get::<WorkingMemory>(entity) {
        ui.collapsing("📜 Recent Memories", |ui| {
            if wm.buffer.is_empty() {
                ui.label(egui::RichText::new("Nothing recent").italics());
            } else {
                for item in wm.buffer.iter().rev().take(20) {
                    ui.label(format!("• {:?}", item.event));
                }
            }
        });
    }
}

fn node_label(world: &World, node: &Node) -> String {
    match node {
        Node::Self_ => "Self".to_string(),
        Node::Entity(e) => world
            .get::<Name>(*e)
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("Entity({:?})", e)),
        Node::Concept(c) => format!("{:?}", c),
        Node::Tile((x, y)) => format!("Tile({}, {})", x, y),
        Node::Chunk((x, y)) => format!("Chunk({}, {})", x, y),
        Node::Area(a) => format!("Area({:?})", a),
        Node::Event(id) => format!("Event(#{})", id),
        Node::Action(a) => format!("{:?}", a),
        Node::Direction(d) => format!("{:?}", d),
    }
}

// ============================================================================
// INVENTORY TAB
// ============================================================================

fn render_inventory(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let Some(slots) = world.get::<ItemSlots>(entity) else {
        ui.label("No inventory.");
        return;
    };
    ui.heading("Carrying");

    // Things are now per-instance — group by concept for the count column,
    // and average freshness when present so the player sees "how good" the
    // pile is at a glance.
    let mut grouped: std::collections::BTreeMap<String, (u32, f32, u32)> =
        std::collections::BTreeMap::new();
    for thing in slots.all_items() {
        let key = format!("{:?}", thing.concept);
        let entry = grouped.entry(key).or_insert((0, 0.0, 0));
        entry.0 += 1;
        if let Some(f) = thing.properties.freshness {
            entry.1 += f;
            entry.2 += 1;
        }
    }

    if grouped.is_empty() {
        ui.label(egui::RichText::new("Empty").italics());
        return;
    }
    egui::Grid::new("inventory_grid")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Item");
            ui.strong("Qty");
            ui.strong("Freshness");
            ui.end_row();
            for (concept, (qty, fresh_sum, fresh_count)) in grouped {
                ui.label(concept);
                ui.label(format!("{}", qty));
                if fresh_count > 0 {
                    let avg = fresh_sum / fresh_count as f32;
                    ui.label(format!("{:.0}%", avg * 100.0));
                } else {
                    ui.label("—");
                }
                ui.end_row();
            }
        });
}

// ============================================================================
// ACTIVITY TAB
// ============================================================================

fn render_activity(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let Some(log) = world.get_resource::<GameLog>() else {
        ui.label("No game log available.");
        return;
    };

    ui.heading("Recent Activity");
    ui.label(
        egui::RichText::new("Game-log entries that mention this agent, newest first.")
            .small()
            .color(Color32::LIGHT_GRAY),
    );
    ui.add_space(2.0);

    let mut entries: Vec<_> = log
        .all_entries()
        .filter(|e| e.entity == Some(entity))
        .collect();
    entries.reverse();
    entries.truncate(60);

    if entries.is_empty() {
        ui.label(egui::RichText::new("Nothing logged for this agent yet.").italics());
        return;
    }

    for entry in entries {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("[{}]", entry.timestamp))
                    .small()
                    .color(Color32::LIGHT_GRAY),
            );
            let count_suffix = if entry.count > 1 {
                format!(" (×{})", entry.count)
            } else {
                String::new()
            };
            ui.label(format!(
                "{} {}{}",
                entry.category.prefix(),
                entry.message,
                count_suffix
            ));
        });
    }
}

// ============================================================================
// BRAIN TAB (debug only)
// ============================================================================

fn render_brain(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let Some(brain) = world.get::<BrainState>(entity) else {
        ui.label("No brain state.");
        return;
    };
    ui.heading("Arbitration Powers");
    ui.columns(3, |cols| {
        let p = &brain.powers;
        cols[0].vertical_centered(|ui| {
            ui.label("Survival");
            ui.add(egui::ProgressBar::new(p.survival).text(format!("{:.2}", p.survival)));
        });
        cols[1].vertical_centered(|ui| {
            ui.label("Emotional");
            ui.add(egui::ProgressBar::new(p.emotional).text(format!("{:.2}", p.emotional)));
        });
        cols[2].vertical_centered(|ui| {
            ui.label("Rational");
            ui.add(egui::ProgressBar::new(p.rational).text(format!("{:.2}", p.rational)));
        });
    });

    if let Some(winner) = brain.winner {
        ui.horizontal(|ui| {
            ui.label("Winner:");
            let color = match winner {
                BrainType::Survival => Color32::RED,
                BrainType::Emotional => Color32::from_rgb(255, 105, 180),
                BrainType::Rational => Color32::CYAN,
            };
            ui.colored_label(color, winner.display_name());
        });
    }

    ui.separator();
    ui.heading("Proposals");
    for prop in &brain.proposals {
        let color = match prop.brain {
            BrainType::Survival => Color32::LIGHT_RED,
            BrainType::Emotional => Color32::from_rgb(255, 182, 193),
            BrainType::Rational => Color32::LIGHT_BLUE,
        };
        ui.colored_label(
            color,
            format!(
                "• {}: {} (urgency {:.1}) — {}",
                prop.brain.display_name(),
                prop.action.name,
                prop.urgency,
                prop.reasoning
            ),
        );
    }
}

// ============================================================================
// HELPERS
// ============================================================================

fn current_action_summary(world: &World, entity: Entity) -> String {
    let Some(active) = world.get::<ActiveActions>(entity) else {
        return "Idle".into();
    };

    let action = active
        .iter()
        .find(|a| !matches!(a.action_type, ActionType::Idle))
        .or_else(|| active.iter().next());

    let Some(a) = action else {
        return "Idle".into();
    };

    let base = a.action_type.verb();
    if let Some(target) = a.target_entity {
        let target_name = world
            .get::<Name>(target)
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("{:?}", target));
        format!("{} {}", base, target_name)
    } else if let Some(pos) = a.target_position
        && !matches!(a.action_type, ActionType::Flee)
    {
        format!("{} ({:.0}, {:.0})", base, pos.x, pos.y)
    } else {
        base.to_string()
    }
}

fn winning_reasoning(brain: &BrainState) -> Option<&str> {
    let winner = brain.winner?;
    brain
        .proposals
        .iter()
        .find(|p| p.brain == winner)
        .map(|p| p.reasoning.as_str())
}

fn mood_descriptor(emotions: &EmotionalState) -> (String, Color32) {
    // If there's a dominant emotion, use its label
    if let Some(top) = emotions.active_emotions.iter().max_by(|a, b| {
        a.intensity
            .partial_cmp(&b.intensity)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) && top.intensity > 0.2
    {
        let (label, color) = emotion_label_color(top.emotion_type, top.intensity);
        return (label.to_string(), color);
    }
    let mood = emotions.current_mood;
    let label = mood_text(mood).to_string();
    let color = if mood > 0.0 {
        Color32::from_rgb(100, 220, 140)
    } else if mood < 0.0 {
        Color32::from_rgb(220, 120, 120)
    } else {
        Color32::LIGHT_GRAY
    };
    (label, color)
}

fn emotion_label_color(e: EmotionType, intensity: f32) -> (&'static str, Color32) {
    let strong = intensity > 0.7;
    match e {
        EmotionType::Joy => (
            if strong { "Joyful" } else { "Happy" },
            Color32::from_rgb(255, 220, 80),
        ),
        EmotionType::Sadness => (
            if strong { "Miserable" } else { "Sad" },
            Color32::from_rgb(100, 140, 220),
        ),
        EmotionType::Fear => (
            if strong { "Terrified" } else { "Scared" },
            Color32::from_rgb(180, 120, 220),
        ),
        EmotionType::Anger => (
            if strong { "Furious" } else { "Angry" },
            Color32::from_rgb(220, 80, 60),
        ),
        EmotionType::Disgust => ("Disgusted", Color32::from_rgb(140, 200, 80)),
        EmotionType::Surprise => ("Surprised", Color32::from_rgb(255, 180, 100)),
    }
}

fn mood_text(mood: f32) -> &'static str {
    if mood > 0.6 {
        "Joyful"
    } else if mood > 0.2 {
        "Content"
    } else if mood > -0.2 {
        "Neutral"
    } else if mood > -0.6 {
        "Unhappy"
    } else {
        "Miserable"
    }
}

fn stress_color(stress: f32) -> Color32 {
    // Inverted: high stress is bad. Map to severity by inverting.
    severity_color(1.0 - (stress / 100.0).clamp(0.0, 1.0), 0.3, 0.6)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::biology::body::Body;
    use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
    use crate::agent::body::species::{Species, SpeciesProfile};
    use crate::agent::mind::knowledge::MindGraph;
    use crate::agent::psyche::emotions::{Emotion, EmotionType, EmotionalState};
    use crate::agent::psyche::personality::{Personality, PersonalityTraits};

    fn spawn_human(world: &mut World) -> Entity {
        world
            .spawn((
                Name::new("TestHuman"),
                SpeciesProfile::human(),
                PhysicalNeeds::default(),
                Consciousness::default(),
                PsychologicalDrives::default(),
                EmotionalState::default(),
                Personality {
                    traits: PersonalityTraits::default(),
                },
                Body::default(),
                MindGraph::default(),
            ))
            .id()
    }

    fn spawn_minimal_deer(world: &mut World) -> Entity {
        world
            .spawn((
                Name::new("TestDeer"),
                SpeciesProfile {
                    species: Species::Deer,
                    ..SpeciesProfile::human()
                },
                PhysicalNeeds::default(),
                EmotionalState::default(),
                Body::default(),
            ))
            .id()
    }

    #[test]
    fn visible_tabs_include_personality_for_human() {
        let mut world = World::new();
        let entity = spawn_human(&mut world);
        let tabs = visible_tabs_for_entity(&world, entity, false);
        assert!(tabs.contains(&CharSheetTab::Overview));
        assert!(tabs.contains(&CharSheetTab::Needs));
        assert!(tabs.contains(&CharSheetTab::Personality));
        assert!(tabs.contains(&CharSheetTab::Health));
        assert!(tabs.contains(&CharSheetTab::Knowledge));
    }

    #[test]
    fn visible_tabs_exclude_personality_for_deer_without_component() {
        let mut world = World::new();
        let entity = spawn_minimal_deer(&mut world);
        let tabs = visible_tabs_for_entity(&world, entity, false);
        assert!(tabs.contains(&CharSheetTab::Overview));
        assert!(tabs.contains(&CharSheetTab::Needs));
        assert!(tabs.contains(&CharSheetTab::Health));
        assert!(!tabs.contains(&CharSheetTab::Personality));
        assert!(!tabs.contains(&CharSheetTab::Knowledge));
        assert!(!tabs.contains(&CharSheetTab::Inventory));
    }

    #[test]
    fn brain_tab_only_visible_when_debug_enabled() {
        let mut world = World::new();
        let entity = world
            .spawn((
                Name::new("TestAgent"),
                PhysicalNeeds::default(),
                EmotionalState::default(),
                Body::default(),
                BrainState::default(),
            ))
            .id();

        let tabs_off = visible_tabs_for_entity(&world, entity, false);
        assert!(!tabs_off.contains(&CharSheetTab::Brain));

        let tabs_on = visible_tabs_for_entity(&world, entity, true);
        assert!(tabs_on.contains(&CharSheetTab::Brain));
    }

    #[test]
    fn dominant_emotion_overrides_mood_in_bottom_bar() {
        let mut state = EmotionalState {
            current_mood: 0.5,
            stress_level: 0.0,
            active_emotions: Vec::new(),
        };
        state
            .active_emotions
            .push(Emotion::new(EmotionType::Fear, 0.9));
        let (label, _) = mood_descriptor(&state);
        assert_eq!(label, "Terrified");
    }

    #[test]
    fn mood_text_buckets() {
        assert_eq!(mood_text(0.8), "Joyful");
        assert_eq!(mood_text(0.3), "Content");
        assert_eq!(mood_text(0.0), "Neutral");
        assert_eq!(mood_text(-0.3), "Unhappy");
        assert_eq!(mood_text(-0.8), "Miserable");
    }
}
