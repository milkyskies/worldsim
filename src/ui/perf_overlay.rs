//! F3-toggled live performance overlay.
//!
//! Reads: [`PerfTracker`] (per-bucket rolling windows), [`PerfOverlayEnabled`],
//! Bevy's [`FrameTimeDiagnosticsPlugin`] for FPS.
//! Writes: paints a floating egui window with a sorted text table.
//! Upstream: `core::PerfPlugin` populates the tracker; this module is purely a
//! renderer + keyboard toggle.
//! Downstream: the user's eyes.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::{EguiContext, EguiPrimaryContextPass, PrimaryEguiContext, egui};
use egui::Color32;

use crate::core::{PerfOverlayEnabled, PerfTracker};
use crate::menu::sim_interactive;

/// Show any bucket using at least this fraction of the tick in red.
const HOT_PCT_THRESHOLD: f64 = 25.0;

pub struct PerfOverlayPlugin;

impl Plugin for PerfOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, toggle_perf_overlay.run_if(sim_interactive))
            .add_systems(
                EguiPrimaryContextPass,
                perf_overlay_system.run_if(perf_overlay_enabled),
            );
    }
}

fn perf_overlay_enabled(overlay: Res<PerfOverlayEnabled>) -> bool {
    overlay.0
}

fn toggle_perf_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut overlay: ResMut<PerfOverlayEnabled>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        overlay.0 = !overlay.0;
    }
}

fn perf_overlay_system(
    mut egui_contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>,
    tracker: Res<PerfTracker>,
    diagnostics: Res<DiagnosticsStore>,
) {
    let Ok(mut egui_context) = egui_contexts.single_mut() else {
        return;
    };
    let ctx = egui_context.get_mut();

    let snapshot = tracker.snapshot();
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.average());
    let frame_time_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.average());

    egui::Window::new("⏱ perf (F3)")
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-10.0, 10.0))
        .resizable(false)
        .collapsible(true)
        .default_width(320.0)
        .show(ctx, |ui| {
            egui::Grid::new("perf_header")
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    ui.label("tick avg");
                    ui.monospace(format!("{:>7.1} µs", snapshot.total_avg_us));
                    ui.end_row();

                    ui.label("tick max");
                    ui.monospace(format!("{:>7.1} µs", snapshot.total_max_us));
                    ui.end_row();

                    if let Some(fps) = fps {
                        ui.label("fps");
                        ui.monospace(format!("{:>7.1}", fps));
                        ui.end_row();
                    }
                    if let Some(ft) = frame_time_ms {
                        ui.label("frame");
                        ui.monospace(format!("{:>7.2} ms", ft));
                        ui.end_row();
                    }

                    ui.label("window");
                    ui.monospace(format!(
                        "{} / {} ticks",
                        snapshot.samples,
                        tracker.capacity()
                    ));
                    ui.end_row();
                });

            ui.separator();

            // Header row for the bucket table.
            egui::Grid::new("perf_bucket_header")
                .num_columns(4)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    ui.strong("system");
                    ui.strong("avg µs");
                    ui.strong("max µs");
                    ui.strong("% tick");
                    ui.end_row();
                });

            for row in &snapshot.buckets {
                let color = if row.pct_of_tick >= HOT_PCT_THRESHOLD {
                    Color32::from_rgb(230, 90, 90)
                } else {
                    Color32::GRAY
                };
                let children: Vec<&crate::core::SubBucketStats> = snapshot
                    .sub_buckets
                    .iter()
                    .filter(|s| s.parent == row.name)
                    .collect();

                if children.is_empty() {
                    // Leaf bucket — no sub-rows, render a plain line matching
                    // the collapsing-header alignment so the two styles read
                    // as one table.
                    ui.horizontal(|ui| {
                        ui.add_space(18.0); // align under the chevron
                        ui.colored_label(
                            color,
                            egui::RichText::new(format!(
                                "{:<14} {:>8.1} µs  {:>8.1} max  {:>5.1}%",
                                row.name, row.avg_us, row.max_us, row.pct_of_tick
                            ))
                            .monospace(),
                        );
                    });
                } else {
                    egui::CollapsingHeader::new(
                        egui::RichText::new(format!(
                            "{:<14} {:>8.1} µs  {:>8.1} max  {:>5.1}%",
                            row.name, row.avg_us, row.max_us, row.pct_of_tick
                        ))
                        .monospace()
                        .color(color),
                    )
                    .id_salt(format!("perf_parent_{}", row.name))
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::Grid::new(format!("perf_children_{}", row.name))
                            .num_columns(4)
                            .spacing([12.0, 2.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for child in children {
                                    let child_color = if child.pct_of_tick >= HOT_PCT_THRESHOLD {
                                        Color32::from_rgb(230, 90, 90)
                                    } else {
                                        Color32::DARK_GRAY
                                    };
                                    ui.colored_label(child_color, format!("  └ {}", child.name));
                                    ui.monospace(format!("{:>8.1}", child.avg_us));
                                    ui.monospace(format!("{:>8.1}", child.max_us));
                                    ui.monospace(format!("{:>6.1}%", child.pct_of_tick));
                                    ui.end_row();
                                }
                            });
                    });
                }
            }

            ui.add_space(4.0);
            ui.small("press F3 to hide");
        });
}
