use eframe::egui::{self, Color32, CornerRadius, FontId, RichText, Stroke, Vec2};

use crate::gui::state::GuiState;
use crate::gui::panels::graph_view::GraphNodeInfo;
use crate::gui::theme::LairesTheme;
use crate::gui::ProjectSnapshot;

/// Renders the Story Dashboard view — metrics, graph summary, and quick insights.
pub fn render(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    snapshot: &Option<ProjectSnapshot>,
    theme: &LairesTheme,
) {
    egui::ScrollArea::vertical()
        .id_salt("dashboard_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let Some(snap) = snapshot else {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("No project loaded.")
                            .color(theme.text_secondary)
                            .italics()
                            .size(14.0),
                    );
                });
                return;
            };

            // === Top row: Graph + Story Overview side by side ===
            let available_w = ui.available_width();
            let graph_w = (available_w * 0.58).max(300.0);
            let overview_w = available_w - graph_w - 12.0;

            ui.horizontal(|ui| {
                // Left: Narrative Graph card
                ui.allocate_ui(Vec2::new(graph_w, 360.0), |ui| {
                    render_graph_card(ui, snap, theme);
                });

                ui.add_space(4.0);

                // Right: Story Overview card
                ui.allocate_ui(Vec2::new(overview_w, 360.0), |ui| {
                    render_overview_card(ui, state, snap, theme);
                });
            });

            ui.add_space(8.0);

            // === Bottom row: Quick Insights ===
            render_insights_row(ui, snap, theme);
        });
}

/// The Narrative Graph summary card — shows node/edge counts and legend.
/// The full interactive graph is on the Graph tab.
fn render_graph_card(
    ui: &mut egui::Ui,
    snap: &ProjectSnapshot,
    theme: &LairesTheme,
) {
    theme.card_frame().show(ui, |ui| {
        ui.set_min_size(ui.available_size());

        // Header
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("\u{25C9} Narrative Graph")
                    .color(theme.text_primary)
                    .size(14.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new("Switch to Graph tab for full view")
                        .color(theme.text_secondary)
                        .size(10.0),
                );
            });
        });
        ui.add_space(12.0);

        if snap.graph_nodes.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Run Scan to build the narrative graph.")
                        .color(theme.text_secondary)
                        .italics()
                        .size(13.0),
                );
            });
        } else {
            // Node type breakdown
            let characters = count_by_type(&snap.graph_nodes, "character");
            let scenes = count_by_type(&snap.graph_nodes, "scene");
            let objectives = count_by_type(&snap.graph_nodes, "objective");
            let conflicts = count_by_type(&snap.graph_nodes, "conflict");
            let total_edges = snap.graph_edges.len();

            ui.label(
                RichText::new(format!(
                    "{} nodes \u{00B7} {} connections",
                    snap.graph_nodes.len(),
                    total_edges
                ))
                .color(theme.text_secondary)
                .size(12.0),
            );
            ui.add_space(16.0);

            // Visual node breakdown as colored bars
            let categories = [
                ("Characters", characters, theme.character_color),
                ("Scenes", scenes, theme.scene_color),
                ("Objectives", objectives, theme.objective_color),
                ("Conflicts", conflicts, theme.conflict_color),
            ];

            let max_count = categories.iter().map(|(_, c, _)| *c).max().unwrap_or(1).max(1);

            for (label, count, color) in &categories {
                if *count == 0 {
                    continue;
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    // Colored dot
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 5.0, *color);

                    ui.label(
                        RichText::new(*label)
                            .color(theme.text_primary)
                            .size(12.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(count.to_string())
                                .color(*color)
                                .size(12.0)
                                .strong(),
                        );
                    });
                });

                // Bar
                let bar_w = ui.available_width();
                let bar_h = 6.0;
                let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_w, bar_h), egui::Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius::same(3), theme.bg_input);
                let frac = *count as f32 / max_count as f32;
                let filled = egui::Rect::from_min_size(
                    rect.min,
                    Vec2::new(rect.width() * frac, rect.height()),
                );
                ui.painter().rect_filled(filled, CornerRadius::same(3), *color);
            }

            // Legend at bottom
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Graph Legend").color(theme.text_secondary).size(10.0).strong());
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for (label, _, color) in &categories {
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 4.0, *color);
                    ui.label(
                        RichText::new(*label)
                            .color(theme.text_secondary)
                            .size(10.0),
                    );
                    ui.add_space(8.0);
                }
            });
        }
    });
}

/// Story Overview card with metric tiles and progress.
fn render_overview_card(
    ui: &mut egui::Ui,
    state: &GuiState,
    snap: &ProjectSnapshot,
    theme: &LairesTheme,
) {
    theme.card_frame().show(ui, |ui| {
        ui.set_min_size(ui.available_size());

        ui.label(
            RichText::new("\u{2606} Story Overview")
                .color(theme.text_primary)
                .size(14.0)
                .strong(),
        );
        ui.add_space(12.0);

        // Metric tiles — 2x2 grid
        let tile_w = (ui.available_width() - 12.0).max(0.0) / 2.0;
        let tile_h = 72.0;

        // Row 1: Words + Characters
        ui.horizontal(|ui| {
            render_metric_tile(ui, tile_w, tile_h, "WORDS", &format_number(state.word_count), theme.accent, theme);
            ui.add_space(4.0);
            render_metric_tile(ui, tile_w, tile_h, "CHARACTERS", &state.char_count.to_string(), theme.objective_color, theme);
        });
        ui.add_space(4.0);

        // Row 2: Scenes + Analysis %
        let total_scenes = state.scene_count;
        let analyzed = count_by_type(&snap.graph_nodes, "scene");
        let pct = if total_scenes > 0 {
            (analyzed as f32 / total_scenes as f32 * 100.0) as usize
        } else {
            0
        };

        ui.horizontal(|ui| {
            render_metric_tile(ui, tile_w, tile_h, "SCENES", &total_scenes.to_string(), theme.scene_color, theme);
            ui.add_space(4.0);
            render_metric_tile(ui, tile_w, tile_h, "ANALYZED", &format!("{}%", pct), theme.accent_secondary, theme);
        });

        ui.add_space(16.0);

        // Progress section
        ui.label(
            RichText::new("Current Progress")
                .color(theme.text_secondary)
                .size(11.0)
                .strong(),
        );
        ui.add_space(6.0);

        // Progress bar
        let progress_frac = if total_scenes > 0 {
            analyzed as f32 / total_scenes as f32
        } else {
            0.0
        };
        render_progress_bar(ui, progress_frac, theme);

        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "{} of {} scenes analyzed",
                analyzed, total_scenes
            ))
            .color(theme.text_secondary)
            .size(11.0),
        );

        // Laires Insight callout (if we have characters)
        if state.char_count > 0 {
            ui.add_space(12.0);
            render_insight_callout(ui, state, snap, theme);
        }
    });
}

/// A single metric tile (large number + label).
fn render_metric_tile(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    label: &str,
    value: &str,
    accent: Color32,
    theme: &LairesTheme,
) {
    ui.allocate_ui(Vec2::new(width, height), |ui| {
        theme.metric_card_frame().show(ui, |ui| {
            ui.set_min_size(Vec2::new((width - 16.0).max(0.0), (height - 16.0).max(0.0)));
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(label)
                        .color(theme.text_secondary)
                        .size(10.0)
                        .strong(),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(value)
                        .color(accent)
                        .font(FontId::proportional(26.0))
                        .strong(),
                );
            });
        });
    });
}

/// A horizontal progress bar.
fn render_progress_bar(ui: &mut egui::Ui, fraction: f32, theme: &LairesTheme) {
    let width = ui.available_width();
    let height = 8.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());

    let painter = ui.painter();

    // Background track
    painter.rect_filled(
        rect,
        CornerRadius::same(4),
        theme.bg_input,
    );

    // Filled portion
    if fraction > 0.0 {
        let filled_rect = egui::Rect::from_min_size(
            rect.min,
            Vec2::new(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
        );
        painter.rect_filled(
            filled_rect,
            CornerRadius::same(4),
            theme.accent,
        );
    }
}

/// A highlighted insight callout box.
fn render_insight_callout(
    ui: &mut egui::Ui,
    state: &GuiState,
    snap: &ProjectSnapshot,
    theme: &LairesTheme,
) {
    egui::Frame::NONE
        .fill(Color32::from_rgb(0xFE, 0xF5, 0xE5)) // warm amber tint
        .stroke(Stroke::new(1.0, Color32::from_rgb(0xE6, 0x77, 0x00)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("\u{2605}")
                        .color(Color32::from_rgb(0xE6, 0x77, 0x00))
                        .size(14.0),
                );
                ui.label(
                    RichText::new("LAIRES INSIGHT")
                        .color(Color32::from_rgb(0xE6, 0x77, 0x00))
                        .size(10.0)
                        .strong(),
                );
            });
            ui.add_space(4.0);

            // Generate a simple insight based on available data
            let insight = generate_insight(state, snap);
            ui.label(
                RichText::new(insight)
                    .color(theme.text_primary)
                    .size(12.0),
            );
        });
}

/// Quick Insights row — Character Arcs, Pacing, Conflict Density.
fn render_insights_row(ui: &mut egui::Ui, snap: &ProjectSnapshot, theme: &LairesTheme) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("\u{2726} Quick Insights")
                .color(theme.text_primary)
                .size(14.0)
                .strong(),
        );
    });
    ui.add_space(8.0);

    let card_w = (ui.available_width() - 16.0).max(0.0) / 3.0;

    ui.horizontal(|ui| {
        // Character Arcs
        ui.allocate_ui(Vec2::new(card_w, 160.0), |ui| {
            render_character_arcs_card(ui, snap, theme);
        });
        ui.add_space(4.0);

        // Pacing
        ui.allocate_ui(Vec2::new(card_w, 160.0), |ui| {
            render_pacing_card(ui, snap, theme);
        });
        ui.add_space(4.0);

        // Conflict Density
        ui.allocate_ui(Vec2::new(card_w, 160.0), |ui| {
            render_conflicts_card(ui, snap, theme);
        });
    });
}

/// Character Arcs insight card.
fn render_character_arcs_card(ui: &mut egui::Ui, snap: &ProjectSnapshot, theme: &LairesTheme) {
    theme.card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(
            RichText::new("Character Arcs")
                .color(theme.text_primary)
                .size(12.0)
                .strong(),
        );
        ui.add_space(8.0);

        let characters: Vec<&GraphNodeInfo> = snap
            .graph_nodes
            .iter()
            .filter(|n| n.node_type == "character")
            .collect();

        if characters.is_empty() {
            ui.label(
                RichText::new("No characters detected yet.")
                    .color(theme.text_secondary)
                    .size(11.0)
                    .italics(),
            );
        } else {
            for (i, ch) in characters.iter().take(5).enumerate() {
                ui.horizontal(|ui| {
                    // Colored dot
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, theme.character_color);

                    ui.label(
                        RichText::new(&ch.label)
                            .color(theme.text_primary)
                            .size(12.0),
                    );

                    // Connection count as a simple strength proxy
                    let connections = snap
                        .graph_edges
                        .iter()
                        .filter(|e| e.source == ch.id || e.target == ch.id)
                        .count();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} links", connections))
                                .color(theme.text_secondary)
                                .size(10.0),
                        );
                    });
                });
                if i < characters.len().min(5) - 1 {
                    ui.add_space(2.0);
                }
            }
        }
    });
}

/// Pacing insight card with simple bar chart.
fn render_pacing_card(ui: &mut egui::Ui, snap: &ProjectSnapshot, theme: &LairesTheme) {
    theme.card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(
            RichText::new("Pacing Analysis")
                .color(theme.text_primary)
                .size(12.0)
                .strong(),
        );
        ui.add_space(8.0);

        // Compute word counts per scene group (approximate from scene boundary lines)
        let lines: Vec<&str> = snap.story_text.lines().collect();
        let total_lines = lines.len();

        if total_lines == 0 || snap.scene_boundary_lines.is_empty() {
            ui.label(
                RichText::new("Not enough data for pacing analysis.")
                    .color(theme.text_secondary)
                    .size(11.0)
                    .italics(),
            );
            return;
        }

        // Gather scene word counts
        let mut boundaries: Vec<usize> = snap.scene_boundary_lines.iter().copied().collect();
        boundaries.sort();

        let mut scene_words: Vec<usize> = Vec::new();
        for i in 0..boundaries.len() {
            let start = boundaries[i];
            let end = if i + 1 < boundaries.len() {
                boundaries[i + 1]
            } else {
                total_lines
            };
            let wc: usize = lines[start..end]
                .iter()
                .map(|l| l.split_whitespace().count())
                .sum();
            scene_words.push(wc);
        }

        let max_wc = scene_words.iter().copied().max().unwrap_or(1).max(1);
        let bar_area_w = (ui.available_width() - 8.0).max(0.0);
        let bar_h = 10.0;
        let gap = 3.0;

        // Draw bars (limited to 12 to fit)
        let display_count = scene_words.len().min(12);
        for wc in scene_words.iter().take(display_count) {
            let frac = *wc as f32 / max_wc as f32;
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(bar_area_w, bar_h),
                egui::Sense::hover(),
            );

            // Background
            ui.painter().rect_filled(rect, CornerRadius::same(2), theme.bg_input);

            // Filled bar
            let bar_rect = egui::Rect::from_min_size(
                rect.min,
                Vec2::new(rect.width() * frac, rect.height()),
            );
            ui.painter().rect_filled(bar_rect, CornerRadius::same(2), theme.accent);

            ui.add_space(gap);
        }

        if scene_words.len() > 12 {
            ui.label(
                RichText::new(format!("+ {} more scenes", scene_words.len() - 12))
                    .color(theme.text_secondary)
                    .size(10.0),
            );
        }
    });
}

/// Conflict Density insight card.
fn render_conflicts_card(ui: &mut egui::Ui, snap: &ProjectSnapshot, theme: &LairesTheme) {
    theme.card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(
            RichText::new("Conflict Density")
                .color(theme.text_primary)
                .size(12.0)
                .strong(),
        );
        ui.add_space(8.0);

        let conflicts: Vec<&GraphNodeInfo> = snap
            .graph_nodes
            .iter()
            .filter(|n| n.node_type == "conflict")
            .collect();

        let objectives: Vec<&GraphNodeInfo> = snap
            .graph_nodes
            .iter()
            .filter(|n| n.node_type == "objective")
            .collect();

        if conflicts.is_empty() && objectives.is_empty() {
            ui.label(
                RichText::new("No conflicts or objectives detected.")
                    .color(theme.text_secondary)
                    .size(11.0)
                    .italics(),
            );
            return;
        }

        // Conflict count bar
        if !conflicts.is_empty() {
            render_density_row(ui, "Conflicts", conflicts.len(), theme.conflict_color, theme);
            ui.add_space(6.0);
        }

        // Objective count bar
        if !objectives.is_empty() {
            render_density_row(ui, "Objectives", objectives.len(), theme.objective_color, theme);
            ui.add_space(6.0);
        }

        // Character count bar for reference
        let char_count = snap
            .graph_nodes
            .iter()
            .filter(|n| n.node_type == "character")
            .count();
        if char_count > 0 {
            render_density_row(ui, "Characters", char_count, theme.character_color, theme);
        }
    });
}

/// A labeled horizontal density bar.
fn render_density_row(
    ui: &mut egui::Ui,
    label: &str,
    count: usize,
    color: Color32,
    theme: &LairesTheme,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .color(theme.text_primary)
                .size(11.0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(count.to_string())
                    .color(color)
                    .size(11.0)
                    .strong(),
            );
        });
    });

    // Bar
    let bar_w = ui.available_width();
    let bar_h = 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_w, bar_h), egui::Sense::hover());

    ui.painter().rect_filled(rect, CornerRadius::same(3), theme.bg_input);

    // Filled portion — scale relative to 10 (arbitrary ceiling for visual)
    let frac = (count as f32 / 10.0).clamp(0.0, 1.0);
    let filled = egui::Rect::from_min_size(
        rect.min,
        Vec2::new(rect.width() * frac, rect.height()),
    );
    ui.painter().rect_filled(filled, CornerRadius::same(3), color);
}

/// Generate a simple text insight from available data.
fn generate_insight(state: &GuiState, snap: &ProjectSnapshot) -> String {
    let characters = snap
        .graph_nodes
        .iter()
        .filter(|n| n.node_type == "character")
        .count();
    let conflicts = snap
        .graph_nodes
        .iter()
        .filter(|n| n.node_type == "conflict")
        .count();

    if characters == 0 {
        return "Scan your story to get narrative insights.".to_string();
    }

    if conflicts == 0 && characters > 0 {
        return format!(
            "Found {} characters but no conflicts yet. Consider adding tension between characters.",
            characters
        );
    }

    let ratio = conflicts as f32 / characters as f32;
    if ratio < 0.5 {
        format!(
            "Your story has {} characters and {} conflicts. The conflict density is low \u{2014} consider raising the stakes.",
            characters, conflicts
        )
    } else {
        format!(
            "Good conflict density: {} conflicts across {} characters. Your narrative has strong tension.",
            conflicts, characters
        )
    }
}

fn count_by_type(nodes: &[GraphNodeInfo], node_type: &str) -> usize {
    nodes.iter().filter(|n| n.node_type == node_type).count()
}

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}
