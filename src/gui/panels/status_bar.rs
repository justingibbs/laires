use eframe::egui::{self, RichText};

use crate::gui::state::{AgentStatus, AppMode, GuiState};
use crate::gui::theme::LairesTheme;

pub fn render(ctx: &egui::Context, state: &GuiState, theme: &LairesTheme) {
    // Only show status bar in project mode
    if state.app_mode != AppMode::Project {
        return;
    }

    egui::TopBottomPanel::bottom("status_bar")
        .exact_height(28.0)
        .frame(egui::Frame::NONE.fill(theme.bg_panel).inner_margin(egui::Margin::symmetric(12, 4)))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                // Project title (clickable → return to welcome)
                if !state.project_title.is_empty() {
                    let title_label = RichText::new(&state.project_title)
                        .color(theme.accent)
                        .small()
                        .strong();
                    if ui.link(title_label).on_hover_text("Switch project").clicked() {
                        // We can't mutate state here (it's &GuiState), so we set a
                        // flag via interior approach — the caller checks switch_to_welcome.
                        // Instead, we use the response to signal via ctx memory.
                        ctx.memory_mut(|mem| mem.data.insert_temp(egui::Id::new("switch_to_welcome"), true));
                    }
                    ui.label(RichText::new("|").color(theme.text_secondary).small());
                }

                // Provider info (clickable → opens settings)
                let provider_label = RichText::new(format!("{} | {}", state.privacy_label, state.model_name))
                    .color(theme.accent)
                    .small();
                if ui.link(provider_label).on_hover_text("Open Settings").clicked() {
                    ctx.memory_mut(|mem| {
                        mem.data.insert_temp(egui::Id::new("open_settings"), true);
                    });
                }

                ui.add_space(16.0);

                // Counts
                ui.label(
                    RichText::new(format!("{} scenes", state.scene_count))
                        .color(theme.text_secondary)
                        .small(),
                );
                ui.label(RichText::new("|").color(theme.text_secondary).small());
                ui.label(
                    RichText::new(format!("{} characters", state.char_count))
                        .color(theme.text_secondary)
                        .small(),
                );
                ui.label(RichText::new("|").color(theme.text_secondary).small());
                ui.label(
                    RichText::new(format!("{} words", state.word_count))
                        .color(theme.text_secondary)
                        .small(),
                );

                // Token usage from last request
                if let Some(usage) = &state.last_usage {
                    ui.label(RichText::new("|").color(theme.text_secondary).small());
                    ui.label(
                        RichText::new(usage)
                            .color(theme.text_secondary)
                            .small(),
                    );
                }

                // Right-aligned: gear icon + agent status
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Settings button (rightmost)
                    let gear = RichText::new("Settings").small().color(theme.accent);
                    if ui.link(gear).on_hover_text("Cmd+,").clicked() {
                        ctx.memory_mut(|mem| {
                            mem.data.insert_temp(egui::Id::new("open_settings"), true);
                        });
                    }
                    ui.label(RichText::new("|").color(theme.text_secondary).small());
                    match &state.agent_status {
                        AgentStatus::Idle => {
                            ui.label(
                                RichText::new("Ready").color(theme.text_secondary).small(),
                            );
                        }
                        AgentStatus::Thinking => {
                            ui.spinner();
                            ui.label(RichText::new("Thinking...").small());
                        }
                        AgentStatus::ToolCall(name) => {
                            ui.spinner();
                            ui.label(RichText::new(format!("Calling {name}...")).small());
                        }
                        AgentStatus::Streaming => {
                            ui.spinner();
                            ui.label(RichText::new("Streaming...").small());
                        }
                    }
                });
            });
        });
}
