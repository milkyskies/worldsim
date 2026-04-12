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
use crate::agent::mind::perception::VisibleObjects;
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

fn placeholder(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .italics()
            .color(Color32::from_gray(140)),
    );
}

// ============================================================================
// PLUGIN
// ============================================================================

pub struct CharacterSheetPlugin;

impl Plugin for CharacterSheetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CharacterSheetState>()
            .add_systems(EguiPrimaryContextPass, character_sheet_system);
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
    Vitals,
    Drives,
    Plans,
    Perception,
    Channels,
    Personality,
    Skills,
    Social,
    Health,
    Knowledge,
    Inventory,
    Activity,
    Brain,
}

impl CharSheetTab {
    pub fn label(self) -> &'static str {
        match self {
            CharSheetTab::Overview => "Overview",
            CharSheetTab::Vitals => "Vitals",
            CharSheetTab::Drives => "Drives",
            CharSheetTab::Plans => "Plans",
            CharSheetTab::Perception => "Perception",
            CharSheetTab::Channels => "Channels",
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
                    CharSheetTab::Vitals => render_vitals(ui, world, entity),
                    CharSheetTab::Drives => render_drives(ui, world, entity),
                    CharSheetTab::Plans => render_plans(ui, world, entity),
                    CharSheetTab::Perception => render_perception(ui, world, entity),
                    CharSheetTab::Channels => render_channels(ui, world, entity),
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
}

fn build_header_summary(world: &World, entity: Entity) -> HeaderSummary {
    let (mood_label, mood_color) = world
        .get::<EmotionalState>(entity)
        .map(mood_descriptor)
        .unwrap_or_else(|| ("Unknown".into(), Color32::GRAY));

    let action_text = current_action_summary(world, entity);

    HeaderSummary {
        mood_label,
        mood_color,
        action_text,
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
    let is_agent_like = world.get::<EmotionalState>(entity).is_some()
        || world.get::<PhysicalNeeds>(entity).is_some()
        || world.get::<ActiveActions>(entity).is_some()
        || world.get::<MindGraph>(entity).is_some();
    if !is_agent_like {
        return Vec::new();
    }

    let mut tabs = vec![
        CharSheetTab::Overview,
        CharSheetTab::Vitals,
        CharSheetTab::Drives,
        CharSheetTab::Plans,
        CharSheetTab::Perception,
        CharSheetTab::Channels,
        CharSheetTab::Personality,
        CharSheetTab::Skills,
        CharSheetTab::Social,
        CharSheetTab::Health,
        CharSheetTab::Knowledge,
        CharSheetTab::Inventory,
        CharSheetTab::Activity,
    ];

    if debug_enabled {
        tabs.push(CharSheetTab::Brain);
    }

    tabs
}

// ============================================================================
// OVERVIEW TAB
// ============================================================================

fn render_overview(ui: &mut egui::Ui, world: &World, entity: Entity) {
    // ── Right now ─────────────────────────────────────────────────────
    let action_text = current_action_summary(world, entity);
    let reason_text = world
        .get::<BrainState>(entity)
        .and_then(winning_reasoning)
        .map(|s| s.to_string());

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("▶").color(Color32::YELLOW));
        ui.label(egui::RichText::new(&action_text).strong());
    });
    if let Some(reason) = reason_text {
        ui.label(
            egui::RichText::new(format!("— {}", reason))
                .italics()
                .color(Color32::LIGHT_GRAY),
        );
    }
    if let Some(transform) = world.get::<Transform>(entity) {
        let pos = transform.translation.truncate();
        ui.label(
            egui::RichText::new(format!("at ({:.0}, {:.0})", pos.x, pos.y))
                .color(Color32::GRAY)
                .small(),
        );
    }

    ui.separator();

    // ── Survival vitals (the numbers that actually predict death) ─────
    if let Some(needs) = world.get::<PhysicalNeeds>(entity) {
        let urgencies = world
            .get::<CentralNervousSystem>(entity)
            .map(|cns| cns.urgencies.as_slice())
            .unwrap_or(&[]);
        let urgency_for = |source: UrgencySource| -> f32 {
            urgencies
                .iter()
                .find(|u| u.source == source)
                .map(|u| u.value)
                .unwrap_or(0.0)
        };

        // Hunger is a 3-pool chain. Show all three so the cause is
        // obvious: empty stomach with full reserves = fine; full stomach
        // with zero reserves = starving but eating; all three empty =
        // imminent death. A single "hunger %" bar can't disambiguate.
        let m = &needs.metabolism;
        ui.label(egui::RichText::new("Fuel").strong());
        vital_row(
            ui,
            "Stomach",
            m.stomach_fullness(),
            crate::agent::body::metabolism::STOMACH_CAPACITY,
            0.1,
            0.3,
        );
        vital_row(
            ui,
            "Glucose",
            m.glucose,
            crate::agent::body::metabolism::GLUCOSE_MAX,
            0.2,
            0.4,
        );
        vital_row(
            ui,
            "Reserves",
            m.reserves,
            crate::agent::body::metabolism::RESERVES_MAX,
            0.2,
            0.5,
        );
        urgency_line(ui, "Hunger urgency", urgency_for(UrgencySource::Hunger));
        if m.is_starving() {
            ui.colored_label(SEVERITY_BAD, "⚠ STARVING — health is dropping");
        }

        ui.add_space(4.0);
        ui.label(egui::RichText::new("Body").strong());
        vital_row(ui, "Health", needs.health, 100.0, 0.3, 0.7);
        // Thirst is "higher = worse" so flip the bar direction — show
        // hydration instead of the raw thirst integer.
        let hydration = (needs.hydration / 100.0).clamp(0.0, 1.0);
        vital_row_fraction(ui, "Hydration", hydration, 0.3, 0.7);
        urgency_line(ui, "Thirst urgency", urgency_for(UrgencySource::Thirst));
        vital_row(
            ui,
            "Stamina",
            needs.stamina.aerobic,
            needs.stamina.aerobic_max,
            0.3,
            0.6,
        );
        urgency_line(ui, "Fatigue urgency", urgency_for(UrgencySource::Stamina));

        if let Some(src) = needs.last_health_damage {
            ui.label(
                egui::RichText::new(format!("Last damage: {:?}", src))
                    .color(Color32::LIGHT_RED)
                    .small(),
            );
        }
    }

    ui.separator();

    // ── Plan (what does the agent think it's doing across time?) ─────
    if let Some(memory) = world.get::<PlanMemory>(entity) {
        let executing: Vec<_> = memory.in_state(PlanState::Executing).collect();
        if executing.is_empty() {
            ui.label(
                egui::RichText::new("No plan")
                    .italics()
                    .color(Color32::GRAY),
            );
        } else {
            ui.label(egui::RichText::new("Plan").strong());
            for plan in executing {
                if plan.steps.is_empty() {
                    continue;
                }
                let steps: Vec<String> = plan
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(i, step)| {
                        if i < plan.current_step {
                            format!("✓{}", step.name)
                        } else if i == plan.current_step {
                            format!("▶{}", step.name)
                        } else {
                            step.name.clone()
                        }
                    })
                    .collect();
                ui.label(steps.join("  →  "));
            }
        }
        let background_count = memory.count_state(PlanState::Background)
            + memory.count_state(PlanState::Considering)
            + memory.count_state(PlanState::Suspended);
        if background_count > 0 {
            ui.label(
                egui::RichText::new(format!("+{background_count} in background"))
                    .small()
                    .color(Color32::GRAY),
            );
        }
    }

    ui.separator();

    // ── Inventory (one line, not a whole tab for 2 items) ────────────
    if let Some(inv) = world.get::<ItemSlots>(entity) {
        let items: Vec<String> = inv
            .group_by_concept()
            .into_iter()
            .map(|(c, q)| format!("{:?}×{}", c, q))
            .collect();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Holding").strong());
            if items.is_empty() {
                ui.label(
                    egui::RichText::new("nothing")
                        .italics()
                        .color(Color32::GRAY),
                );
            } else {
                ui.label(items.join(", "));
            }
        });
    }

    ui.separator();

    // ── Mood / emotions (compact) ────────────────────────────────────
    if let Some(emotions) = world.get::<EmotionalState>(entity) {
        let mood = emotions.current_mood;
        let mood_label = mood_text(mood);
        let mood_color = if mood > 0.2 {
            Color32::from_rgb(100, 220, 140)
        } else if mood < -0.2 {
            Color32::from_rgb(220, 120, 120)
        } else {
            Color32::LIGHT_GRAY
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Mood").strong());
            ui.colored_label(mood_color, mood_label);
            ui.label(
                egui::RichText::new(format!("({:+.2})", mood))
                    .small()
                    .color(Color32::GRAY),
            );
            let stress = emotions.stress_level;
            if stress > 20.0 {
                ui.separator();
                ui.colored_label(stress_color(stress), format!("stress {:.0}", stress));
            }
        });

        if !emotions.active_emotions.is_empty() {
            let mut sorted: Vec<_> = emotions.active_emotions.iter().collect();
            sorted.sort_by(|a, b| {
                b.intensity
                    .partial_cmp(&a.intensity)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            ui.horizontal_wrapped(|ui| {
                for emo in sorted.iter().take(4) {
                    let (label, color) = emotion_label_color(emo.emotion_type, emo.intensity);
                    ui.colored_label(color, format!("{} {:.0}%", label, emo.intensity * 100.0));
                }
            });
        }
    }
}

/// Horizontal "label | progress bar | number" row keyed to a 0..max
/// value, colored red-yellow-green by the value's fraction against
/// `bad_below` and `warn_above` thresholds (both in 0..1).
fn vital_row(
    ui: &mut egui::Ui,
    label: &str,
    value: f32,
    max: f32,
    bad_below: f32,
    warn_above: f32,
) {
    let frac = (value / max).clamp(0.0, 1.0);
    let color = severity_color(frac, bad_below, warn_above);
    ui.horizontal(|ui| {
        ui.add_sized([80.0, 0.0], egui::Label::new(label));
        ui.add(
            egui::ProgressBar::new(frac)
                .desired_width(160.0)
                .fill(color)
                .text(format!("{:.0}/{:.0}", value, max)),
        );
    });
}

/// Like `vital_row` but the caller already computed a 0..1 fraction.
fn vital_row_fraction(ui: &mut egui::Ui, label: &str, frac: f32, bad_below: f32, warn_above: f32) {
    let color = severity_color(frac.clamp(0.0, 1.0), bad_below, warn_above);
    ui.horizontal(|ui| {
        ui.add_sized([80.0, 0.0], egui::Label::new(label));
        ui.add(
            egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                .desired_width(160.0)
                .fill(color)
                .text(format!("{:.0}%", frac * 100.0)),
        );
    });
}

/// Small subordinate line showing a CNS urgency score. Hidden if the
/// urgency is essentially zero to avoid noise.
fn urgency_line(ui: &mut egui::Ui, label: &str, urgency: f32) {
    if urgency < 0.05 {
        return;
    }
    let color = severity_color(1.0 - urgency, 0.3, 0.6);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new(format!("{}:", label))
                .small()
                .color(Color32::GRAY),
        );
        ui.colored_label(color, format!("{:.2}", urgency));
    });
}

// ============================================================================
// VITALS TAB — PhysicalNeeds only (things that can kill you)
// ============================================================================

fn render_vitals(ui: &mut egui::Ui, world: &World, entity: Entity) {
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

    let Some(needs) = world.get::<PhysicalNeeds>(entity) else {
        placeholder(ui, "(no physical-needs component on this entity)");
        return;
    };

    ui.label(
        egui::RichText::new(
            "Physical needs — empty any of these for long enough and the agent dies.",
        )
        .italics()
        .color(Color32::LIGHT_GRAY),
    );
    ui.add_space(4.0);

    egui::CollapsingHeader::new("Fuel")
        .default_open(true)
        .show(ui, |ui| {
            satisfaction_bar(
                ui,
                "Satiety (stomach)",
                needs.metabolism.stomach_fullness(),
                crate::agent::body::metabolism::STOMACH_CAPACITY,
                None,
            );
            satisfaction_bar(
                ui,
                "Energy (glucose)",
                needs.metabolism.glucose,
                crate::agent::body::metabolism::GLUCOSE_MAX,
                urgency_for(UrgencySource::Hunger),
            );
            satisfaction_bar(
                ui,
                "Reserves",
                needs.metabolism.reserves,
                crate::agent::body::metabolism::RESERVES_MAX,
                None,
            );
        });

    egui::CollapsingHeader::new("Body")
        .default_open(true)
        .show(ui, |ui| {
            satisfaction_bar(
                ui,
                "Hydration",
                needs.hydration,
                100.0,
                urgency_for(UrgencySource::Thirst),
            );
            satisfaction_bar(
                ui,
                "Stamina (aerobic)",
                needs.stamina.aerobic,
                needs.stamina.aerobic_max,
                urgency_for(UrgencySource::Stamina),
            );
            satisfaction_bar(
                ui,
                "Sprint (anaerobic)",
                needs.stamina.anaerobic,
                needs.stamina.anaerobic_max,
                None,
            );
            satisfaction_bar(ui, "Health", needs.health, 100.0, None);
        });

    if let Some(src) = needs.last_health_damage {
        ui.add_space(4.0);
        ui.colored_label(Color32::LIGHT_RED, format!("Last damage: {:?}", src));
    }
}

// ============================================================================
// DRIVES TAB — psychological drives + consciousness
// ============================================================================

fn render_drives(ui: &mut egui::Ui, world: &World, entity: Entity) {
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

    ui.label(
        egui::RichText::new(
            "Psychological motivations — push behavior but don't kill on their own.",
        )
        .italics()
        .color(Color32::LIGHT_GRAY),
    );
    ui.add_space(4.0);

    if let Some(consc) = world.get::<Consciousness>(entity) {
        satisfaction_bar(ui, "Alertness", consc.alertness, 1.0, None);
        ui.add_space(4.0);
    }

    let Some(drives) = world.get::<PsychologicalDrives>(entity) else {
        placeholder(ui, "(no psychological drives on this entity)");
        return;
    };

    satisfaction_bar(
        ui,
        "Companionship (social)",
        drives.companionship,
        1.0,
        urgency_for(UrgencySource::Social),
    );
    satisfaction_bar(
        ui,
        "Enjoyment (fun)",
        drives.enjoyment,
        1.0,
        urgency_for(UrgencySource::Fun),
    );
    satisfaction_bar(
        ui,
        "Stimulation (curiosity)",
        drives.stimulation,
        1.0,
        urgency_for(UrgencySource::Curiosity),
    );
    satisfaction_bar(ui, "Esteem (status)", drives.esteem, 1.0, None);
    satisfaction_bar(ui, "Safety (security)", drives.safety, 1.0, None);
    satisfaction_bar(ui, "Autonomy", drives.autonomy, 1.0, None);
    satisfaction_bar(
        ui,
        "Dominion (territory)",
        drives.dominion,
        1.0,
        urgency_for(UrgencySource::Territoriality),
    );
}

/// Horizontal satisfaction bar: label on the left, a colored bar
/// filled proportional to `value/max`, and an optional urgency
/// sub-line. Raw field name shown in parens in the label so the data
/// stays grep-able. Colors: red below 30%, yellow below 60%, green
/// above — matches the same convention used on Overview.
fn satisfaction_bar(ui: &mut egui::Ui, label: &str, value: f32, max: f32, urgency: Option<f32>) {
    let frac = (value / max).clamp(0.0, 1.0);
    let color = severity_color(frac, 0.3, 0.6);
    ui.horizontal(|ui| {
        ui.add_sized([180.0, 0.0], egui::Label::new(label));
        ui.add(
            egui::ProgressBar::new(frac)
                .desired_width(160.0)
                .fill(color)
                .text(format!("{:.0}%", frac * 100.0)),
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
// PERCEPTION TAB — what the agent senses right now
// ============================================================================

fn render_perception(ui: &mut egui::Ui, world: &World, entity: Entity) {
    ui.label(
        egui::RichText::new(
            "What this agent can see, hear, or feel right now. Anything missing from this list is invisible to the brain.",
        )
        .italics()
        .color(Color32::LIGHT_GRAY),
    );
    ui.add_space(4.0);

    let agent_pos = world
        .get::<Transform>(entity)
        .map(|t| t.translation.truncate());

    ui.heading("Visible entities");
    let visible = world.get::<VisibleObjects>(entity);
    match visible {
        None => placeholder(ui, "(this entity has no vision)"),
        Some(v) if v.entities.is_empty() => placeholder(ui, "(sees nothing)"),
        Some(v) => {
            let mut rows: Vec<(f32, String, String)> = Vec::new();
            for &other in &v.entities {
                let name = world
                    .get::<Name>(other)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| format!("{:?}", other));
                let kind = world
                    .get::<crate::agent::inventory::EntityType>(other)
                    .map(|t| format!("{:?}", t.0))
                    .unwrap_or_else(|| "?".into());
                let dist = match (agent_pos, world.get::<Transform>(other)) {
                    (Some(a), Some(t)) => a.distance(t.translation.truncate()),
                    _ => f32::INFINITY,
                };
                rows.push((dist, name, kind));
            }
            rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            egui::Grid::new("perception_visible")
                .striped(true)
                .num_columns(3)
                .show(ui, |ui| {
                    ui.strong("Entity");
                    ui.strong("Kind");
                    ui.strong("Distance");
                    ui.end_row();
                    for (dist, name, kind) in rows {
                        ui.label(name);
                        ui.label(kind);
                        ui.label(if dist.is_finite() {
                            format!("{:.0}", dist)
                        } else {
                            "?".into()
                        });
                        ui.end_row();
                    }
                });
        }
    }
}

// ============================================================================
// CHANNELS TAB — body-channel occupancy
// ============================================================================

fn render_channels(ui: &mut egui::Ui, world: &World, entity: Entity) {
    use crate::agent::actions::ActionRegistry;
    use crate::agent::actions::channel::{Channel, ChannelCapacities};

    ui.label(
        egui::RichText::new(
            "Body channels currently occupied by actions. An action can't start if its channels conflict.",
        )
        .italics()
        .color(Color32::LIGHT_GRAY),
    );
    ui.add_space(4.0);

    let active = world.get::<ActiveActions>(entity);
    let body = world.get::<Body>(entity);
    let physical = world.get::<PhysicalNeeds>(entity);
    let registry = world.get_resource::<ActionRegistry>();

    let Some(registry) = registry else {
        placeholder(ui, "(ActionRegistry resource unavailable)");
        return;
    };
    let Some(active) = active else {
        placeholder(ui, "(this entity has no ActiveActions)");
        return;
    };

    let capacities = ChannelCapacities::compute(body, physical);

    let mut per_channel: Vec<(Channel, f32, Vec<String>)> =
        Channel::ALL.iter().map(|c| (*c, 0.0, Vec::new())).collect();

    for state in active.iter() {
        let Some(def) = registry.get(state.action_type) else {
            continue;
        };
        for usage in def.body_channels() {
            let slot = per_channel
                .iter_mut()
                .find(|(c, _, _)| *c == usage.channel)
                .unwrap();
            slot.1 += usage.intensity;
            slot.2.push(format!("{:?}", state.action_type));
        }
    }

    egui::Grid::new("channels_grid")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Channel");
            ui.strong("Load");
            ui.strong("Capacity");
            ui.strong("Holders");
            ui.end_row();
            for (channel, load, holders) in per_channel {
                let cap = capacities.get(channel);
                ui.label(format!("{:?}", channel));
                let frac = if cap > 0.0 {
                    (load / cap).clamp(0.0, 1.5)
                } else {
                    0.0
                };
                let fill = if load == 0.0 {
                    Color32::DARK_GRAY
                } else if load >= cap {
                    Color32::from_rgb(200, 80, 80)
                } else {
                    Color32::from_rgb(140, 200, 255)
                };
                ui.add(
                    egui::ProgressBar::new((frac / 1.5).clamp(0.0, 1.0))
                        .desired_width(120.0)
                        .fill(fill)
                        .text(format!("{:.2}", load)),
                );
                ui.label(format!("{:.2}", cap));
                if holders.is_empty() {
                    ui.colored_label(Color32::from_gray(140), "—");
                } else {
                    ui.label(holders.join(", "));
                }
                ui.end_row();
            }
        });
}

// ============================================================================
// PERSONALITY TAB
// ============================================================================

fn render_personality(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let Some(p) = world.get::<Personality>(entity) else {
        placeholder(ui, "(this entity has no personality — probably an animal)");
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
        placeholder(ui, "(this entity has no learned skills)");
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
        placeholder(ui, "(no social knowledge — this entity has no mind)");
        return;
    };

    let known = mind.query(None, Some(Predicate::Knows), Some(&Value::Boolean(true)));
    if known.is_empty() {
        placeholder(ui, "(has not met anyone yet)");
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
    ui.heading("Conversation");
    let Some(in_conv) = world.get::<InConversation>(entity) else {
        placeholder(ui, "(not currently in a conversation)");
        ui.add_space(6.0);
        return;
    };
    let Some(manager) = world.get_resource::<ConversationManager>() else {
        placeholder(ui, "(conversation manager unavailable)");
        return;
    };
    let Some(conv) = manager.get(in_conv.conversation_id) else {
        placeholder(ui, "(conversation record missing)");
        return;
    };
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
        placeholder(
            ui,
            "(this entity has no body — disembodied or substrate-only)",
        );
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
        placeholder(ui, "(no mind on this entity — can't know anything)");
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
        placeholder(ui, "(this entity has no inventory slots)");
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
        placeholder(ui, "(not carrying anything)");
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
        placeholder(ui, "(game log unavailable)");
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
        placeholder(ui, "(nothing logged for this agent yet)");
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

// ============================================================================
// PLANS TAB — active plan memory + last brain arbitration
// ============================================================================

/// Renders everything the agent is currently "thinking about":
/// - The brain arbitration header: powers per brain, last-tick winner,
///   and the chosen actions the brain is driving this frame.
/// - All `HeldPlan`s in `PlanMemory`, grouped by state (Executing,
///   Considering, Background, Suspended), with step chains, commitment
///   bars against each plan's threshold, goal, and source.
/// - The full `BrainProposal` list sorted by effective score so the
///   player can see "Survival wants Eat at 76, Emotional wants
///   Converse at 22, Rational wants Harvest at 60, Survival won".
fn render_plans(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let brain = world.get::<BrainState>(entity);
    let memory = world.get::<PlanMemory>(entity);
    let personality = world.get::<Personality>(entity);

    // ── Arbitration: powers + winner ─────────────────────────────────
    if let Some(brain) = brain {
        ui.label(egui::RichText::new("Brain powers").strong());
        let p = &brain.powers;
        let total = (p.survival + p.emotional + p.rational).max(1.0);
        power_bar(ui, BrainType::Survival, p.survival, total);
        power_bar(ui, BrainType::Emotional, p.emotional, total);
        power_bar(ui, BrainType::Rational, p.rational, total);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Winner:").strong());
            if let Some(winner) = brain.winner {
                ui.colored_label(brain_color(winner), winner.display_name());
            } else {
                ui.label(egui::RichText::new("(none)").italics().color(Color32::GRAY));
            }
        });

        if !brain.chosen_actions.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Doing now:").strong());
                for action in &brain.chosen_actions {
                    ui.colored_label(Color32::YELLOW, &action.name);
                }
            });
        }
        ui.separator();
    }

    // ── Held plans grouped by lifecycle state ────────────────────────
    if let Some(memory) = memory {
        if memory.plans.is_empty() {
            ui.label(
                egui::RichText::new("No plans held")
                    .italics()
                    .color(Color32::GRAY),
            );
        } else {
            ui.label(egui::RichText::new(format!("Plans held: {}", memory.plans.len())).strong());
            ui.add_space(4.0);

            // Render in priority order: Executing first (these are what
            // the agent's actually doing), then Considering (next up),
            // then Background and Suspended (farther from acting).
            let order = [
                (
                    PlanState::Executing,
                    "Executing",
                    Color32::from_rgb(255, 210, 90),
                ),
                (
                    PlanState::Considering,
                    "Considering",
                    Color32::from_rgb(180, 210, 255),
                ),
                (PlanState::Background, "Background", Color32::LIGHT_GRAY),
                (
                    PlanState::Suspended,
                    "Suspended",
                    Color32::from_rgb(180, 140, 140),
                ),
            ];
            for (state, label, color) in order {
                let plans: Vec<_> = memory.in_state(state).collect();
                if plans.is_empty() {
                    continue;
                }
                ui.colored_label(color, format!("{} ({})", label, plans.len()));
                for plan in plans {
                    render_held_plan(ui, plan, personality);
                }
                ui.add_space(4.0);
            }
        }
    }

    // ── Full proposal list from the last arbitration ─────────────────
    if let Some(brain) = brain
        && !brain.proposals.is_empty()
    {
        ui.separator();
        ui.label(egui::RichText::new("Last arbitration").strong());
        let mut sorted: Vec<&crate::agent::brains::proposal::BrainProposal> =
            brain.proposals.iter().collect();
        sorted.sort_by(|a, b| {
            let a_score = a.urgency * a.brain.power(&brain.powers);
            let b_score = b.urgency * b.brain.power(&brain.powers);
            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for prop in sorted {
            let admitted = brain
                .chosen_actions
                .iter()
                .any(|a| a.name == prop.action.name);
            let effective = prop.urgency * prop.brain.power(&brain.powers);
            let mark = if admitted { "✓" } else { "·" };
            ui.horizontal(|ui| {
                ui.colored_label(
                    if admitted {
                        Color32::YELLOW
                    } else {
                        Color32::GRAY
                    },
                    mark,
                );
                ui.colored_label(brain_color(prop.brain), prop.brain.display_name());
                ui.label(egui::RichText::new(&prop.action.name).color(if admitted {
                    Color32::WHITE
                } else {
                    Color32::LIGHT_GRAY
                }));
                ui.label(
                    egui::RichText::new(format!(
                        "u={:.0}  ×{:.1}  = {:.0}",
                        prop.urgency,
                        prop.brain.power(&brain.powers),
                        effective,
                    ))
                    .small()
                    .color(Color32::GRAY),
                );
            });
            if !prop.reasoning.is_empty() {
                ui.label(
                    egui::RichText::new(format!("    {}", prop.reasoning))
                        .italics()
                        .small()
                        .color(Color32::from_gray(170)),
                );
            }
        }
    }
}

fn brain_color(brain: BrainType) -> Color32 {
    match brain {
        BrainType::Survival => Color32::from_rgb(230, 120, 100),
        BrainType::Emotional => Color32::from_rgb(255, 160, 210),
        BrainType::Rational => Color32::from_rgb(140, 200, 255),
    }
}

fn power_bar(ui: &mut egui::Ui, brain: BrainType, value: f32, total: f32) {
    let frac = (value / total).clamp(0.0, 1.0);
    ui.horizontal(|ui| {
        ui.add_sized([80.0, 0.0], egui::Label::new(brain.display_name()));
        ui.add(
            egui::ProgressBar::new(frac)
                .desired_width(180.0)
                .fill(brain_color(brain))
                .text(format!("{:.1}", value)),
        );
    });
}

fn render_held_plan(
    ui: &mut egui::Ui,
    plan: &crate::agent::brains::plan_memory::HeldPlan,
    personality: Option<&Personality>,
) {
    ui.group(|ui| {
        // Step chain (✓ done, ▶ current, bare = upcoming)
        if plan.steps.is_empty() {
            ui.label(
                egui::RichText::new("(stepless)")
                    .italics()
                    .color(Color32::GRAY),
            );
        } else {
            let chain: Vec<String> = plan
                .steps
                .iter()
                .enumerate()
                .map(|(i, step)| {
                    if i < plan.current_step {
                        format!("✓{}", step.name)
                    } else if i == plan.current_step {
                        format!("▶{}", step.name)
                    } else {
                        step.name.clone()
                    }
                })
                .collect();
            ui.label(egui::RichText::new(chain.join("  →  ")).strong());
        }

        // Goal
        let goal_text = plan
            .goal
            .conditions
            .iter()
            .map(|c| {
                format!(
                    "{:?}={}",
                    c.predicate
                        .map(|p| format!("{:?}", p))
                        .unwrap_or_else(|| "?".into()),
                    c.object
                        .as_ref()
                        .map(|o| format!("{:?}", o))
                        .unwrap_or_else(|| "?".into()),
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        ui.label(
            egui::RichText::new(format!(
                "goal: {}  (priority {:.2})",
                if goal_text.is_empty() {
                    "(none)".into()
                } else {
                    goal_text
                },
                plan.goal.priority,
            ))
            .small()
            .color(Color32::GRAY),
        );

        // Commitment bar vs cost-derived threshold — this is the
        // promotion ladder in visual form. A plan at Executing with
        // low commitment is about to be suspended; Background with
        // rising commitment is about to be Considering.
        let threshold = personality
            .map(|p| {
                crate::agent::brains::rational::compute_commit_threshold(
                    plan.subjective_cost,
                    p.traits.conscientiousness,
                )
            })
            .unwrap_or(plan.subjective_cost);
        let frac = if threshold > 0.0 {
            (plan.commitment / threshold).clamp(0.0, 1.5)
        } else {
            0.0
        };
        let fill = if plan.commitment >= threshold {
            Color32::from_rgb(120, 200, 140)
        } else {
            Color32::from_rgb(160, 170, 200)
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("commit").small());
            ui.add(
                egui::ProgressBar::new((frac / 1.5).clamp(0.0, 1.0))
                    .desired_width(160.0)
                    .fill(fill)
                    .text(format!("{:.2}/{:.2}", plan.commitment, threshold)),
            );
        });

        // Provenance line — was this the agent's own idea or a
        // commitment they made out loud?
        let source = match &plan.source {
            crate::agent::brains::plan_memory::PlanSource::Brain(b) => {
                format!("{} brain", b.display_name())
            }
            crate::agent::brains::plan_memory::PlanSource::VerbalCommitment { .. } => {
                "verbal commitment".into()
            }
        };
        ui.label(
            egui::RichText::new(format!(
                "source: {}  cost {:.2}",
                source, plan.subjective_cost
            ))
            .small()
            .color(Color32::from_gray(140)),
        );
    });
}

// ============================================================================
// BRAIN TAB — developer-only raw proposal dump
// ============================================================================

fn render_brain(ui: &mut egui::Ui, world: &World, entity: Entity) {
    let Some(brain) = world.get::<BrainState>(entity) else {
        placeholder(ui, "(no brain state on this entity)");
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
