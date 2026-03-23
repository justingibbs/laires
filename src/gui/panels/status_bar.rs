use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

use crate::gui::state::{AgentStatus, AppMode, GuiState, SessionMode};
use crate::gui::theme::LairesTheme;

/// Renders the top navigation bar (branded header with search + actions).
pub fn render(ctx: &egui::Context, state: &GuiState, theme: &LairesTheme) {
    if state.app_mode != AppMode::Project {
        return;
    }

    egui::TopBottomPanel::top("top_bar")
        .exact_height(52.0)
        .frame(
            egui::Frame::NONE
                .fill(Color32::WHITE)
                .inner_margin(egui::Margin::symmetric(16, 0))
                .stroke(Stroke::new(1.0, theme.border)),
        )
        .show(ctx, |ui| {
            let bar_width = ui.available_width();
            let is_narrow = bar_width < 900.0;
            let is_very_narrow = bar_width < 600.0;

            ui.set_max_width(bar_width);
            ui.horizontal_centered(|ui| {
                ui.set_max_width(bar_width);
                // === Left section: Branding ===
                ui.label(
                    RichText::new("\u{25C8}")
                        .color(theme.accent)
                        .size(22.0),
                );
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Laires")
                            .color(theme.text_primary)
                            .size(16.0)
                            .strong(),
                    );
                    if !is_very_narrow {
                        ui.label(
                            RichText::new("Writing Analysis")
                                .color(theme.text_secondary)
                                .size(10.0),
                        );
                    }
                });

                ui.add_space(8.0);

                // === Center-left: Project title + provider info ===
                if !state.project_title.is_empty() {
                    ui.label(
                        RichText::new("\u{2502}")
                            .color(theme.border)
                            .size(20.0),
                    );
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        let title = RichText::new(&state.project_title)
                            .color(theme.text_primary)
                            .size(14.0)
                            .strong();
                        if ui.link(title).on_hover_text("Switch project").clicked() {
                            ctx.memory_mut(|mem| {
                                mem.data
                                    .insert_temp(egui::Id::new("switch_to_welcome"), true);
                            });
                        }
                        if !is_very_narrow && !state.model_name.is_empty() {
                            ui.label(
                                RichText::new(format!("{} \u{00B7} {}", state.privacy_label, state.model_name))
                                    .color(theme.text_secondary)
                                    .size(11.0),
                            );
                        }
                    });
                }

                // === Session mode toggle badge ===
                ui.add_space(8.0);
                let (mode_label, mode_color) = match state.session_mode {
                    SessionMode::Consultant => ("Consultant", Color32::from_rgb(59, 130, 246)),
                    SessionMode::Workshop => ("Workshop", Color32::from_rgb(34, 197, 94)),
                };
                let btn = egui::Button::new(
                    RichText::new(mode_label)
                        .color(mode_color)
                        .size(10.0)
                        .strong(),
                )
                .fill(mode_color.gamma_multiply(0.15))
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(4));
                let tooltip = match state.session_mode {
                    SessionMode::Consultant => "Switch to Workshop mode (live editing)",
                    SessionMode::Workshop => "Switch to Consultant mode (read-only analysis)",
                };
                if ui.add(btn).on_hover_text(tooltip).clicked() {
                    ctx.memory_mut(|mem| {
                        mem.data
                            .insert_temp(egui::Id::new("switch_mode"), true);
                    });
                }

                // === Center: Search bar (only if there's room) ===
                if !is_narrow {
                    ui.add_space(16.0);
                    let search_width = (ui.available_width() - 280.0).max(80.0);
                    ui.allocate_ui(egui::vec2(search_width, 32.0), |ui| {
                        ui.add_space(4.0);
                        egui::Frame::NONE
                            .fill(theme.bg_secondary)
                            .stroke(Stroke::new(1.0, theme.border))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal_centered(|ui| {
                                    ui.label(
                                        RichText::new("\u{1F50D}")
                                            .color(theme.text_secondary)
                                            .size(12.0),
                                    );
                                    ui.label(
                                        RichText::new("Search story elements...")
                                            .color(theme.text_secondary)
                                            .size(12.0),
                                    );
                                });
                            });
                    });
                }

                // === Right section: Stats + Actions ===
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Settings (rightmost)
                    let gear = RichText::new("\u{2699}").size(18.0).color(theme.text_secondary);
                    if ui.link(gear).on_hover_text("Settings (Cmd+,)").clicked() {
                        ctx.memory_mut(|mem| {
                            mem.data
                                .insert_temp(egui::Id::new("open_settings"), true);
                        });
                    }

                    ui.add_space(8.0);

                    // Agent status
                    match &state.agent_status {
                        AgentStatus::Idle => {
                            ui.label(
                                RichText::new("\u{25CF}")
                                    .color(theme.objective_color)
                                    .size(11.0),
                            );
                        }
                        AgentStatus::Thinking => {
                            ui.spinner();
                        }
                        AgentStatus::ToolCall(name) => {
                            ui.spinner();
                            if !is_very_narrow {
                                ui.label(
                                    RichText::new(format!("{name}..."))
                                        .color(theme.accent)
                                        .size(11.0),
                                );
                            }
                        }
                    }

                    ui.add_space(8.0);

                    // Scan / action button
                    if state.char_count == 0 {
                        let scan_btn = egui::Button::new(
                            RichText::new("Scan Story").color(Color32::WHITE).size(12.0),
                        )
                        .fill(theme.accent)
                        .corner_radius(CornerRadius::same(8));
                        if ui.add(scan_btn).clicked() {
                            ctx.memory_mut(|mem| {
                                mem.data.insert_temp(egui::Id::new("scan_requested"), true);
                            });
                        }
                    }

                    // Compact stats (hide on very narrow)
                    if !is_very_narrow {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!(
                                "{} scenes \u{00B7} {} chars \u{00B7} {} words",
                                state.scene_count, state.char_count, state.word_count
                            ))
                            .color(theme.text_secondary)
                            .size(11.0),
                        );
                    }

                    if !is_narrow {
                        if let Some(usage) = &state.last_usage {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(usage)
                                    .color(theme.text_secondary)
                                    .size(10.0),
                            );
                        }
                    }
                });
            });
        });
}
