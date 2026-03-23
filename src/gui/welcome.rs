use std::path::PathBuf;

use chrono::{DateTime, Utc};
use eframe::egui::{self, RichText};
use serde::{Deserialize, Serialize};

use super::theme::LairesTheme;

// ---------------------------------------------------------------------------
// RecentProjects model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentProject {
    pub path: PathBuf,
    pub title: String,
    pub last_opened: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecentProjects {
    pub projects: Vec<RecentProject>,
}

const MAX_RECENT: usize = 10;

impl RecentProjects {
    /// Load from `~/.laires/recent.json`. Returns empty if missing or corrupt.
    pub fn load() -> Self {
        let Some(path) = Self::file_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save to `~/.laires/recent.json`, creating the directory if needed.
    pub fn save(&self) {
        let Some(path) = Self::file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Add or bump a project to the top. Truncates to MAX_RECENT.
    pub fn touch(&mut self, path: PathBuf, title: String) {
        self.projects.retain(|p| p.path != path);
        self.projects.insert(
            0,
            RecentProject {
                path,
                title,
                last_opened: Utc::now(),
            },
        );
        self.projects.truncate(MAX_RECENT);
    }

    /// Remove entries where `.laires/` no longer exists.
    pub fn prune(&mut self) {
        self.projects.retain(|p| p.path.join(".laires").is_dir());
    }

    fn file_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".laires").join("recent.json"))
    }
}

// ---------------------------------------------------------------------------
// Welcome screen
// ---------------------------------------------------------------------------

/// Action returned from the welcome screen each frame.
pub enum WelcomeAction {
    None,
    OpenProject,
    NewProject,
    OpenRecent(PathBuf),
}

/// Render the welcome screen. Returns the action the user chose (if any).
pub fn render_welcome(
    ui: &mut egui::Ui,
    recent_projects: &RecentProjects,
    theme: &LairesTheme,
    load_error: &Option<String>,
) -> WelcomeAction {
    let mut action = WelcomeAction::None;

    ui.vertical_centered(|ui| {
        ui.add_space(80.0);

        // Title
        ui.label(
            RichText::new("L A I R E S")
                .size(42.0)
                .color(theme.accent)
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Narrative intelligence for fiction writers")
                .size(16.0)
                .color(theme.text_secondary),
        );

        ui.add_space(40.0);

        // Buttons
        ui.horizontal(|ui| {
            // Center the buttons
            let button_width = 160.0;
            let spacing = 16.0;
            let total = button_width * 2.0 + spacing;
            let avail = ui.available_width();
            if avail > total {
                ui.add_space((avail - total) / 2.0);
            }

            let open_btn = egui::Button::new(RichText::new("Open Project").size(15.0))
                .min_size(egui::vec2(button_width, 40.0));
            if ui.add(open_btn).clicked() {
                action = WelcomeAction::OpenProject;
            }

            ui.add_space(spacing);

            let new_btn = egui::Button::new(RichText::new("New Project").size(15.0))
                .min_size(egui::vec2(button_width, 40.0));
            if ui.add(new_btn).clicked() {
                action = WelcomeAction::NewProject;
            }
        });

        ui.add_space(32.0);

        // Error display
        if let Some(err) = load_error {
            ui.colored_label(theme.error_msg_color, err);
            ui.add_space(12.0);
        }

        // Recent projects list
        if !recent_projects.projects.is_empty() {
            ui.label(
                RichText::new("Recent Projects")
                    .size(14.0)
                    .color(theme.text_secondary),
            );
            ui.add_space(8.0);

            let max_width = 500.0f32;
            let avail = ui.available_width();
            let left_pad = if avail > max_width {
                (avail - max_width) / 2.0
            } else {
                0.0
            };

            for project in &recent_projects.projects {
                ui.horizontal(|ui| {
                    if left_pad > 0.0 {
                        ui.add_space(left_pad);
                    }

                    let title_text = RichText::new(&project.title)
                        .size(14.0)
                        .color(theme.text_accent);

                    if ui.link(title_text).clicked() {
                        action = WelcomeAction::OpenRecent(project.path.clone());
                    }

                    ui.label(
                        RichText::new(project.path.display().to_string())
                            .size(11.0)
                            .color(theme.text_secondary),
                    );
                });
            }
        }
    });

    action
}
