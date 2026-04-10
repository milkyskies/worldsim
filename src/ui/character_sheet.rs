//! Player-facing character sheet UI.
//!
//! Reads: UiState, DebugUiEnabled, Name, SpeciesProfile, PhysicalNeeds, Consciousness, PsychologicalDrives, EmotionalState, Personality, Body, BrainState, RationalBrain, CentralNervousSystem, MindGraph, WorkingMemory, RelationshipHistory, ItemSlots, ActiveActions
//! Writes: CharacterSheetState
//! Upstream: handle_game_click (populates selected entity)
//! Downstream: none - terminal UI

use bevy::prelude::*;
use bevy_egui::{EguiContext, EguiPrimaryContextPass, PrimaryEguiContext, egui};
use egui::Color32;

use crate::agent::actions::ActionType;
use crate::agent::actions::registry::ActiveActions;
use crate::agent::biology::body::{Body, BodyPart, InjuryType};
use crate::agent::body::needs::{Consciousness, PhysicalNeeds, PsychologicalDrives};
use crate::agent::body::species::SpeciesProfile;
use crate::agent::brains::proposal::{BrainState, BrainType};
use crate::agent::brains::rational::RationalBrain;
use crate::agent::item_slots::ItemSlots;
use crate::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Value};
use crate::agent::mind::memory::WorkingMemory;
use crate::agent::nervous_system::cns::CentralNervousSystem;
use crate::agent::psyche::emotions::{EmotionType, EmotionalState};
use crate::agent::psyche::personality::{Personality, PersonalityTrait};
use crate::agent::psyche::relationships::RelationshipHistory;

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
        app.init_resource::<CharacterSheetState>()
            .add_systems(EguiPrimaryContextPass, character_sheet_system);
    }
}

// ============================================================================
// STATE
// ============================================================================

#[derive(Resource, Default)]
pub struct CharacterSheetState {
    /// Is the floating sheet window open?
    pub open: bool,
    /// Which tab is currently active.
    pub active_tab: CharSheetTab,
    /// Tracks the last selection so we can auto-open on change.
    last_selected: Option<Entity>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CharSheetTab {
    #[default]
    Overview,
    Needs,
    Personality,
    Social,
    Health,
    Knowledge,
    Inventory,
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
            CharSheetTab::Social => "Social",
            CharSheetTab::Health => "Health",
            CharSheetTab::Knowledge => "Knowledge",
            CharSheetTab::Inventory => "Inventory",
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

    // Detect selection edge so the sheet auto-opens when the player picks a
    // new agent, without clobbering a manual close for the same selection.
    {
        let mut cs = world.resource_mut::<CharacterSheetState>();
        if cs.last_selected != selected {
            cs.last_selected = selected;
            if selected.is_some() {
                cs.open = true;
            }
        }
    }

    let Some(entity) = selected else {
        return;
    };

    // A selected entity may have been despawned this frame.
    if world.get_entity(entity).is_err() {
        return;
    }

    let debug_enabled = world.resource::<DebugUiEnabled>().0;

    let summary = build_bottom_summary(world, entity);
    let toggle = render_bottom_bar(ctx, &summary);
    if toggle {
        let mut cs = world.resource_mut::<CharacterSheetState>();
        cs.open = !cs.open;
    }

    let (open, active_tab) = {
        let cs = world.resource::<CharacterSheetState>();
        (cs.open, cs.active_tab)
    };
    if !open {
        return;
    }

    let visible_tabs = visible_tabs_for_entity(world, entity, debug_enabled);
    if visible_tabs.is_empty() {
        return;
    }
    let active_tab = if visible_tabs.contains(&active_tab) {
        active_tab
    } else {
        visible_tabs[0]
    };

    let agent_name = world
        .get::<Name>(entity)
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("{:?}", entity));
    let species_label = world
        .get::<SpeciesProfile>(entity)
        .map(|s| format!("{:?}", s.species))
        .unwrap_or_else(|| "Unknown".to_string());

    let mut new_open = true;
    let mut new_tab = active_tab;

    egui::Window::new(format!("📋 {}  ({})", agent_name, species_label))
        .id(egui::Id::new("character_sheet_window"))
        .resizable(true)
        .default_width(440.0)
        .default_height(560.0)
        .open(&mut new_open)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for tab in &visible_tabs {
                    if ui.selectable_label(*tab == new_tab, tab.label()).clicked() {
                        new_tab = *tab;
                    }
                }
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match new_tab {
                    CharSheetTab::Overview => render_overview(ui, world, entity),
                    CharSheetTab::Needs => render_needs(ui, world, entity),
                    CharSheetTab::Personality => render_personality(ui, world, entity),
                    CharSheetTab::Social => render_social(ui, world, entity),
                    CharSheetTab::Health => render_health(ui, world, entity),
                    CharSheetTab::Knowledge => render_knowledge(ui, world, entity),
                    CharSheetTab::Inventory => render_inventory(ui, world, entity),
                    CharSheetTab::Brain => render_brain(ui, world, entity),
                });
        });

    let mut cs = world.resource_mut::<CharacterSheetState>();
    cs.open = new_open;
    cs.active_tab = new_tab;
}

// ============================================================================
// BOTTOM BAR
// ============================================================================

struct BottomSummary {
    name: String,
    mood_label: String,
    mood_color: Color32,
    action_text: String,
    hp_pct: f32,
    energy_pct: f32,
}

fn build_bottom_summary(world: &World, entity: Entity) -> BottomSummary {
    let name = world
        .get::<Name>(entity)
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("{:?}", entity));

    let (mood_label, mood_color) = world
        .get::<EmotionalState>(entity)
        .map(|e| mood_descriptor(e))
        .unwrap_or_else(|| ("Unknown".into(), Color32::GRAY));

    let action_text = current_action_summary(world, entity);

    let (hp_pct, energy_pct) = world
        .get::<PhysicalNeeds>(entity)
        .map(|n| (n.health / 100.0, n.energy / 100.0))
        .unwrap_or((0.0, 0.0));

    BottomSummary {
        name,
        mood_label,
        mood_color,
        action_text,
        hp_pct,
        energy_pct,
    }
}

/// Renders the bottom bar. Returns true if the user clicked the name area
/// (which toggles the character sheet window).
fn render_bottom_bar(ctx: &mut egui::Context, summary: &BottomSummary) -> bool {
    let mut clicked = false;
    egui::TopBottomPanel::bottom("character_sheet_bottom_bar")
        .resizable(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;

                let name_resp = ui.add(
                    egui::Label::new(egui::RichText::new(&summary.name).strong().size(16.0))
                        .sense(egui::Sense::click()),
                );
                if name_resp.clicked() {
                    clicked = true;
                }

                ui.separator();
                ui.colored_label(summary.mood_color, &summary.mood_label);

                ui.separator();
                ui.label(egui::RichText::new(&summary.action_text).italics());

                ui.separator();
                ui.label("❤");
                ui.add(
                    egui::ProgressBar::new(summary.hp_pct)
                        .desired_width(80.0)
                        .text(format!("{:.0}", summary.hp_pct * 100.0)),
                );

                ui.label("⚡");
                ui.add(
                    egui::ProgressBar::new(summary.energy_pct)
                        .desired_width(80.0)
                        .text(format!("{:.0}", summary.energy_pct * 100.0)),
                );
            });
        });
    clicked
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
        .map(|i| i.all_items().any(|s| s.quantity > 0))
        .unwrap_or(false);
    if has_items {
        tabs.push(CharSheetTab::Inventory);
    }

    if debug_enabled && world.get::<BrainState>(entity).is_some() {
        tabs.push(CharSheetTab::Brain);
    }

    tabs
}

// ============================================================================
// OVERVIEW TAB
// ============================================================================

fn render_overview(ui: &mut egui::Ui, world: &World, entity: Entity) {
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

    // Plan steps
    if let Some(rational) = world.get::<RationalBrain>(entity)
        && let Some(plan) = &rational.current_plan
        && !plan.is_empty()
    {
        ui.heading("Plan");
        for (i, step) in plan.iter().enumerate() {
            let is_current = i == rational.plan_index;
            let is_done = i < rational.plan_index;
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
        ui.separator();
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
    ui.heading("Physical Needs");
    if let Some(needs) = world.get::<PhysicalNeeds>(entity) {
        need_bar(ui, "Hunger", needs.hunger, 100.0, 60.0, true);
        need_bar(ui, "Thirst", needs.thirst, 100.0, 60.0, true);
        need_bar(ui, "Energy", needs.energy, 100.0, 30.0, false);
        need_bar(ui, "Health", needs.health, 100.0, 30.0, false);
    }

    if let Some(consc) = world.get::<Consciousness>(entity) {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Alertness");
            ui.add(
                egui::ProgressBar::new(consc.alertness)
                    .desired_width(200.0)
                    .text(format!("{:.2}", consc.alertness)),
            );
        });
    }

    if let Some(drives) = world.get::<PsychologicalDrives>(entity) {
        ui.separator();
        ui.heading("Psychological Drives");
        drive_bar(ui, "Social", drives.social);
        drive_bar(ui, "Fun", drives.fun);
        drive_bar(ui, "Curiosity", drives.curiosity);
        drive_bar(ui, "Status", drives.status);
        drive_bar(ui, "Security", drives.security);
        drive_bar(ui, "Autonomy", drives.autonomy);
        drive_bar(ui, "Territoriality", drives.territoriality);
    }

    if let Some(cns) = world.get::<CentralNervousSystem>(entity) {
        ui.separator();
        ui.heading("Active Urgencies");
        if cns.urgencies.is_empty() {
            ui.label(egui::RichText::new("No urgent drives").italics());
        } else {
            for u in &cns.urgencies {
                ui.horizontal(|ui| {
                    ui.label(format!("{:?}", u.source));
                    ui.add(
                        egui::ProgressBar::new(u.value)
                            .desired_width(160.0)
                            .text(format!("{:.2}", u.value)),
                    );
                });
            }
        }
    }
}

/// Draws a need bar with a threshold marker. `threshold_urgent` is the value
/// where the brain starts caring; the color shifts from green to red past it.
/// `high_is_bad` = true for hunger/thirst (high = urgent).
fn need_bar(
    ui: &mut egui::Ui,
    label: &str,
    value: f32,
    max: f32,
    threshold: f32,
    high_is_bad: bool,
) {
    let pct = (value / max).clamp(0.0, 1.0);
    let urgent = if high_is_bad {
        value >= threshold
    } else {
        value <= threshold
    };
    let color = if urgent {
        Color32::from_rgb(220, 80, 60)
    } else if (high_is_bad && value >= threshold * 0.7)
        || (!high_is_bad && value <= threshold * 1.5)
    {
        Color32::from_rgb(220, 190, 60)
    } else {
        Color32::from_rgb(80, 200, 100)
    };
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::ProgressBar::new(pct)
                .desired_width(200.0)
                .fill(color)
                .text(format!("{:.0}/{:.0}", value, max)),
        );
        ui.label(
            egui::RichText::new(format!("⚠ {}", threshold as i32))
                .color(Color32::GRAY)
                .small(),
        );
    });
}

fn drive_bar(ui: &mut egui::Ui, label: &str, value: f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::ProgressBar::new(value.clamp(0.0, 1.0))
                .desired_width(200.0)
                .text(format!("{:.2}", value)),
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
// SOCIAL TAB
// ============================================================================

fn render_social(ui: &mut egui::Ui, world: &World, entity: Entity) {
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

    // Build a list of (other_entity, category, trust, affection, respect)
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

        let recent_count = history.map(|h| h.get(other).len()).unwrap_or(0);

        rows.push(SocialRow {
            name,
            category,
            trust,
            affection,
            respect,
            recent_count,
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
            if row.recent_count > 0 {
                ui.label(
                    egui::RichText::new(format!("{} recent interactions", row.recent_count))
                        .small()
                        .color(Color32::GRAY),
                );
            }
        });
        ui.add_space(2.0);
    }
}

struct SocialRow {
    name: String,
    category: RelCategory,
    trust: f32,
    affection: f32,
    respect: f32,
    recent_count: usize,
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

            for (name, part) in body.labeled_parts() {
                render_body_part_row(ui, name, part);
            }
        });

    ui.separator();
    ui.heading("Capabilities");
    capability_bar(ui, "Mobility", body.mobility());
    capability_bar(ui, "Manipulation", body.manipulation());
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
    }
}

fn render_body_part_row(ui: &mut egui::Ui, name: &str, part: &BodyPart) {
    ui.label(name);
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

    let items: Vec<_> = slots.all_items().filter(|s| s.quantity > 0).collect();
    if items.is_empty() {
        ui.label(egui::RichText::new("Empty").italics());
        return;
    }
    egui::Grid::new("inventory_grid")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Item");
            ui.strong("Qty");
            ui.end_row();
            for stack in items {
                ui.label(format!("{:?}", stack.concept));
                ui.label(format!("{}", stack.quantity));
                ui.end_row();
            }
        });
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
    } else if let Some(pos) = a.target_position {
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
