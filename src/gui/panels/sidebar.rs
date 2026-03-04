use eframe::egui::{self, RichText};

use crate::gui::state::{GuiState, SidebarTab};
use crate::gui::theme::LairesTheme;
use crate::gui::ProjectSnapshot;

pub fn render(
    ctx: &egui::Context,
    state: &mut GuiState,
    snapshot: &Option<ProjectSnapshot>,
    theme: &LairesTheme,
) {
    if !state.sidebar_visible {
        return;
    }

    egui::SidePanel::left("sidebar")
        .default_width(200.0)
        .min_width(150.0)
        .max_width(350.0)
        .frame(
            egui::Frame::NONE
                .fill(theme.bg_panel)
                .inner_margin(egui::Margin::same(8))
                .stroke(egui::Stroke::new(1.0, theme.border)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.sidebar_tab, SidebarTab::Scenes, "Scenes");
                ui.selectable_value(&mut state.sidebar_tab, SidebarTab::Files, "Files");
            });
            ui.separator();

            egui::ScrollArea::vertical().id_salt("sidebar_scroll").show(ui, |ui| {
                let Some(snap) = snapshot else {
                    ui.label(
                        RichText::new("No project loaded.")
                            .color(theme.text_secondary)
                            .italics(),
                    );
                    return;
                };

                match state.sidebar_tab {
                    SidebarTab::Scenes => {
                        if snap.all_scenes.is_empty() {
                            ui.label(
                                RichText::new("No scenes found.")
                                    .color(theme.text_secondary),
                            );
                            ui.add_space(8.0);
                            if ui.button("Scan Story").clicked() {
                                state.scan_requested = true;
                            }
                        } else {
                            // Show scan button when graph is empty (0 characters)
                            if state.char_count == 0 {
                                ui.add_space(4.0);
                                if ui.button("Scan Story").clicked() {
                                    state.scan_requested = true;
                                }
                                ui.add_space(4.0);
                                ui.separator();
                            }

                            let multi_file = snap.all_scenes.len() > 1;
                            let mut global_idx = 0usize;

                            for group in &snap.all_scenes {
                                if multi_file {
                                    ui.add_space(6.0);
                                    let display_name = if group.file_path.is_empty() {
                                        "Story".to_string()
                                    } else {
                                        group.file_path.clone()
                                    };
                                    ui.label(
                                        RichText::new(display_name)
                                            .strong()
                                            .small()
                                            .color(theme.text_secondary),
                                    );
                                    ui.separator();
                                }

                                for (id, title) in &group.scenes {
                                    global_idx += 1;
                                    let fallback = format!("Scene {}", global_idx);
                                    let label = title.as_deref().unwrap_or(&fallback);
                                    let is_selected =
                                        state.selected_scene_id.as_deref() == Some(id.as_str());

                                    if ui.selectable_label(is_selected, label).clicked() {
                                        state.selected_scene_id = Some(id.clone());
                                    }
                                }
                            }
                        }
                    }
                    SidebarTab::Files => {
                        if snap.story_files.is_empty() && snap.context_files.is_empty() {
                            ui.label(
                                RichText::new("No manifest found.")
                                    .color(theme.text_secondary),
                            );
                            ui.add_space(8.0);
                            if ui.button("Scan Story").clicked() {
                                state.scan_requested = true;
                            }
                        } else {
                            if !snap.story_files.is_empty() {
                                ui.label(RichText::new("Story Files").strong().small());
                                for path in &snap.story_files {
                                    ui.label(
                                        RichText::new(path).small().color(theme.text_primary),
                                    );
                                }
                            }
                            if !snap.context_files.is_empty() {
                                ui.add_space(4.0);
                                ui.label(RichText::new("Context Files").strong().small());
                                for path in &snap.context_files {
                                    ui.label(
                                        RichText::new(path).small().color(theme.text_secondary),
                                    );
                                }
                            }
                        }
                    }
                }
            });
        });
}
