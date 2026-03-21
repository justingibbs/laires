use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

use crate::gui::state::GuiState;
use crate::gui::theme::LairesTheme;
use crate::gui::ProjectSnapshot;

/// Renders the revision brief panel.
///
/// Shows the accumulated brief as formatted markdown-like content,
/// or an empty state message if no revisions have been added yet.
pub fn render(
    ui: &mut egui::Ui,
    _state: &GuiState,
    snapshot: &Option<ProjectSnapshot>,
    theme: &LairesTheme,
) {
    let (revision_count, markdown) = snapshot
        .as_ref()
        .map(|s| (s.brief_revision_count, s.brief_markdown.as_str()))
        .unwrap_or((0, ""));

    if revision_count == 0 {
        render_empty_state(ui, theme);
        return;
    }

    // Header
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Revision Brief")
                .color(theme.text_primary)
                .size(16.0)
                .strong(),
        );
        ui.add_space(8.0);
        let count_badge = egui::Frame::NONE
            .fill(Color32::from_rgb(59, 130, 246).gamma_multiply(0.15))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(egui::Margin::symmetric(6, 2));
        count_badge.show(ui, |ui| {
            ui.label(
                RichText::new(format!("{} revision{}", revision_count, if revision_count == 1 { "" } else { "s" }))
                    .color(Color32::from_rgb(59, 130, 246))
                    .size(11.0),
            );
        });
    });
    ui.add_space(8.0);

    // Render the markdown content as simple formatted text
    egui::ScrollArea::vertical()
        .id_salt("brief_scroll")
        .show(ui, |ui| {
            render_brief_markdown(ui, markdown, theme);
        });
}

fn render_empty_state(ui: &mut egui::Ui, theme: &LairesTheme) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new("No revisions yet")
                .color(theme.text_secondary)
                .size(16.0),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "In Consultant mode, ask the agent to analyze your story\n\
                 and suggest improvements. Revision notes will appear here.",
            )
            .color(theme.text_secondary)
            .size(12.0),
        );
        ui.add_space(16.0);

        // Suggestion pills
        let suggestions = [
            "\"Analyze the pacing in Act 2\"",
            "\"Check Sarah's character arc for consistency\"",
            "\"Find scenes that don't advance the plot\"",
        ];
        for suggestion in &suggestions {
            egui::Frame::NONE
                .fill(theme.bg_secondary)
                .stroke(Stroke::new(1.0, theme.border))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(*suggestion)
                            .color(theme.text_secondary)
                            .italics()
                            .size(11.0),
                    );
                });
            ui.add_space(4.0);
        }
    });
}

/// Simple markdown-ish renderer for the brief content.
///
/// Handles headings, bold, blockquotes, bullet lists, and horizontal rules.
fn render_brief_markdown(ui: &mut egui::Ui, markdown: &str, theme: &LairesTheme) {
    for line in markdown.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }

        // Horizontal rule
        if trimmed == "---" {
            ui.separator();
            continue;
        }

        // Headings
        if trimmed.starts_with("### ") {
            ui.add_space(6.0);
            ui.label(
                RichText::new(&trimmed[4..])
                    .color(theme.text_primary)
                    .size(14.0)
                    .strong(),
            );
            continue;
        }
        if trimmed.starts_with("## ") {
            ui.add_space(8.0);
            ui.label(
                RichText::new(&trimmed[3..])
                    .color(theme.text_primary)
                    .size(16.0)
                    .strong(),
            );
            continue;
        }
        if trimmed.starts_with("# ") {
            ui.add_space(10.0);
            ui.label(
                RichText::new(&trimmed[2..])
                    .color(theme.text_primary)
                    .size(18.0)
                    .strong(),
            );
            continue;
        }

        // Blockquotes
        if trimmed.starts_with("> ") {
            let quote_text = &trimmed[2..];
            egui::Frame::NONE
                .fill(theme.bg_secondary)
                .inner_margin(egui::Margin { left: 12, right: 8, top: 2, bottom: 2 })
                .stroke(Stroke::NONE)
                .show(ui, |ui| {
                    // Left border bar
                    let rect = ui.available_rect_before_wrap();
                    let bar_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x - 8.0, rect.min.y),
                        egui::pos2(rect.min.x - 5.0, rect.max.y.max(rect.min.y + 16.0)),
                    );
                    ui.painter().rect_filled(
                        bar_rect,
                        CornerRadius::same(1),
                        theme.accent.gamma_multiply(0.4),
                    );
                    ui.label(
                        RichText::new(quote_text)
                            .color(theme.text_secondary)
                            .italics()
                            .size(12.0),
                    );
                });
            continue;
        }

        // Checklist items
        if trimmed.starts_with("- [ ] ") {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("\u{2610}") // empty checkbox
                        .color(theme.text_secondary)
                        .size(13.0),
                );
                ui.label(
                    RichText::new(&trimmed[6..])
                        .color(theme.text_primary)
                        .size(12.0),
                );
            });
            continue;
        }

        // Bullet list
        if trimmed.starts_with("- ") {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("\u{2022}")
                        .color(theme.text_secondary)
                        .size(12.0),
                );
                render_inline_bold(ui, &trimmed[2..], theme, 12.0);
            });
            continue;
        }

        // Bold lines (e.g., **Focus**: ...)
        render_inline_bold(ui, trimmed, theme, 12.0);
    }
}

/// Render a line with **bold** segments inline.
fn render_inline_bold(ui: &mut egui::Ui, text: &str, theme: &LairesTheme, size: f32) {
    // If no bold markers, render as plain text
    if !text.contains("**") {
        ui.label(RichText::new(text).color(theme.text_primary).size(size));
        return;
    }

    ui.horizontal_wrapped(|ui| {
        let mut remaining = text;
        while let Some(start) = remaining.find("**") {
            // Text before bold
            let before = &remaining[..start];
            if !before.is_empty() {
                ui.label(RichText::new(before).color(theme.text_primary).size(size));
            }
            remaining = &remaining[start + 2..];
            // Find closing **
            if let Some(end) = remaining.find("**") {
                let bold_text = &remaining[..end];
                ui.label(
                    RichText::new(bold_text)
                        .color(theme.text_primary)
                        .size(size)
                        .strong(),
                );
                remaining = &remaining[end + 2..];
            } else {
                // No closing **, just render rest as-is
                ui.label(
                    RichText::new(remaining)
                        .color(theme.text_primary)
                        .size(size),
                );
                return;
            }
        }
        // Remainder after last bold
        if !remaining.is_empty() {
            ui.label(RichText::new(remaining).color(theme.text_primary).size(size));
        }
    });
}
