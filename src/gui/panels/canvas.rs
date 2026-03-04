use eframe::egui::{self, RichText};

use crate::gui::theme::LairesTheme;
use crate::gui::ProjectSnapshot;

pub fn render(ui: &mut egui::Ui, snapshot: &Option<ProjectSnapshot>, theme: &LairesTheme) {
    ui.vertical(|ui| {
        let Some(snap) = snapshot else {
            ui.label(
                RichText::new("No project loaded.")
                    .color(theme.text_secondary)
                    .italics(),
            );
            return;
        };

        egui::ScrollArea::vertical()
            .id_salt("canvas_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for (line_idx, line) in snap.story_text.lines().enumerate() {
                    let is_boundary = snap.scene_boundary_lines.contains(&line_idx);

                    ui.horizontal(|ui| {
                        // Line number
                        ui.label(
                            RichText::new(format!("{:4} ", line_idx + 1))
                                .color(theme.line_number_color)
                                .monospace(),
                        );

                        // Line text
                        if is_boundary {
                            ui.label(
                                RichText::new(line)
                                    .color(theme.scene_boundary_color)
                                    .strong(),
                            );
                        } else {
                            ui.label(RichText::new(line).color(theme.prose_color));
                        }
                    });
                }
            });
    });
}
