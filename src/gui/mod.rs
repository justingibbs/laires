mod agent;
mod panels;
mod state;
mod theme;
pub(crate) mod welcome;
mod widgets;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, RichText};
use tokio::sync::Mutex;

use crate::concepts::canvas::Canvas;
use crate::concepts::character_perspective::CharacterPerspective;
use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::file_buffer_manager::FileBufferManager;
use crate::concepts::manifest::Manifest;
use crate::concepts::narrative_graph::NarrativeGraph;
use crate::concepts::provider::Provider;
use crate::concepts::scene_map::{ParseMode, SceneMap};
use crate::concepts::skills::{Permission, Skills};
use crate::concepts::text_buffer::TextBuffer;
use crate::config::{self, ProjectConfig, LAIRES_DIR, OVERRIDES_FILE, PERSPECTIVES_CACHE_DIR};

use state::{AgentEvent, AgentStatus, AppMode, ChatMessage, ChatRole, GuiRequest, GuiState, RightTab, ToolCallDisplay};
use theme::LairesTheme;
use welcome::{RecentProjects, WelcomeAction};

/// Loaded project data — shared between GUI and agent via Arc<Mutex<>>.
pub(crate) struct ProjectData {
    pub text_buffer: TextBuffer,
    pub scene_map: SceneMap,
    pub graph: NarrativeGraph,
    pub intent: DeclaredIntent,
    pub perspectives: CharacterPerspective,
    pub canvas: Canvas,
    pub manifest: Option<Manifest>,
    pub file_buffer_manager: Option<FileBufferManager>,
    pub project_root: PathBuf,
    pub config: ProjectConfig,
}

/// State for the "New Project" dialog.
struct NewProjectDialog {
    title: String,
    fountain: bool,
    target_dir: PathBuf,
}

/// Connection test status for the settings dialog.
#[derive(Debug, Clone)]
enum TestConnectionStatus {
    Testing,
    Success(String),
    Failure(String),
}

/// State for the Settings dialog.
struct SettingsDialog {
    provider: String,
    model: String,
    api_key: String,
    api_key_env: String,
    base_url: String,
    test_status: Option<TestConnectionStatus>,
    error_message: Option<String>,
}

struct GuiApp {
    gui_state: GuiState,
    theme: LairesTheme,
    /// Shared domain state (read-only from GUI via try_lock).
    domain: Option<Arc<Mutex<ProjectData>>>,
    /// Cached snapshot of project data for rendering (avoids locking every frame).
    snapshot: Option<ProjectSnapshot>,
    /// Graph layout state (force-directed positions, pan, zoom).
    graph_layout: panels::graph_view::GraphLayoutState,
    /// Send requests to the agent task.
    gui_tx: Option<tokio::sync::mpsc::UnboundedSender<GuiRequest>>,
    /// Receive events from the agent task.
    gui_rx: Option<std::sync::mpsc::Receiver<AgentEvent>>,
    /// Accumulator for current assistant message being built from tool calls.
    pending_tool_calls: Vec<ToolCallDisplay>,

    // --- Project picker state ---
    recent_projects: RecentProjects,
    runtime_handle: tokio::runtime::Handle,
    pending_project_path: Option<PathBuf>,
    load_error: Option<String>,
    new_project_dialog: Option<NewProjectDialog>,
    settings_dialog: Option<SettingsDialog>,
}

/// A group of scenes belonging to a single story file.
pub(crate) struct FileSceneGroup {
    pub file_path: String,
    pub scenes: Vec<(String, Option<String>)>, // (scene_id, title)
}

/// Per-file text data for file-aware canvas rendering.
pub(crate) struct FileTextData {
    pub text: String,
    pub boundary_lines: std::collections::HashSet<usize>,
    pub word_count: usize,
}

/// Cached read-only snapshot of project data for rendering without locking.
pub(crate) struct ProjectSnapshot {
    story_text: String,
    all_scenes: Vec<FileSceneGroup>,
    scene_boundary_lines: std::collections::HashSet<usize>,
    /// Per-file text and boundary data, keyed by file path.
    file_texts: std::collections::HashMap<String, FileTextData>,
    story_files: Vec<String>,
    context_files: Vec<String>,
    graph_nodes: Vec<panels::graph_view::GraphNodeInfo>,
    graph_edges: Vec<panels::graph_view::GraphEdgeInfo>,
}

impl GuiApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        domain: Option<Arc<Mutex<ProjectData>>>,
        gui_state: GuiState,
        gui_tx: Option<tokio::sync::mpsc::UnboundedSender<GuiRequest>>,
        gui_rx: Option<std::sync::mpsc::Receiver<AgentEvent>>,
        recent_projects: RecentProjects,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        let theme = LairesTheme::default();
        theme.apply(&cc.egui_ctx);
        LairesTheme::configure_fonts(&cc.egui_ctx);

        // Build initial snapshot from domain
        let snapshot = domain.as_ref().and_then(|d| {
            d.try_lock().ok().map(|proj| build_snapshot(&proj))
        });

        Self {
            gui_state,
            theme,
            domain,
            snapshot,
            graph_layout: panels::graph_view::GraphLayoutState::default(),
            gui_tx,
            gui_rx,
            pending_tool_calls: Vec::new(),
            recent_projects,
            runtime_handle,
            pending_project_path: None,
            load_error: None,
            new_project_dialog: None,
            settings_dialog: None,
        }
    }

    fn process_agent_events(&mut self) {
        let Some(rx) = &self.gui_rx else { return };

        while let Ok(event) = rx.try_recv() {
            match event {
                AgentEvent::Thinking => {
                    self.gui_state.agent_status = AgentStatus::Thinking;
                }
                AgentEvent::ToolCall { name, args_summary } => {
                    self.gui_state.agent_status = AgentStatus::ToolCall(name.clone());
                    self.pending_tool_calls.push(ToolCallDisplay {
                        name,
                        args_summary,
                        result_summary: String::new(),
                    });
                }
                AgentEvent::ToolResult { name, result_summary } => {
                    // Update the matching pending tool call
                    if let Some(tc) = self
                        .pending_tool_calls
                        .iter_mut()
                        .rev()
                        .find(|tc| tc.name == name)
                    {
                        tc.result_summary = result_summary;
                    }
                }
                AgentEvent::StreamChunk(chunk) => {
                    self.gui_state.agent_status = AgentStatus::Streaming;
                    // Append to the last assistant message or create one
                    if let Some(last) = self.gui_state.chat_history.last_mut() {
                        if last.role == ChatRole::Assistant {
                            last.content.push_str(&chunk);
                        }
                    }
                }
                AgentEvent::Response(text) => {
                    let tool_calls = std::mem::take(&mut self.pending_tool_calls);
                    self.gui_state.chat_history.push(ChatMessage {
                        role: ChatRole::Assistant,
                        content: text,
                        tool_calls,
                    });
                }
                AgentEvent::Error(msg) => {
                    self.gui_state.chat_history.push(ChatMessage {
                        role: ChatRole::Error,
                        content: msg,
                        tool_calls: Vec::new(),
                    });
                }
                AgentEvent::Idle => {
                    self.gui_state.agent_status = AgentStatus::Idle;
                }
                AgentEvent::StateChanged => {
                    // Rebuild snapshot from domain
                    if let Some(d) = &self.domain {
                        if let Ok(proj) = d.try_lock() {
                            self.snapshot = Some(build_snapshot(&proj));
                            self.gui_state.scene_count = proj
                                .file_buffer_manager
                                .as_ref()
                                .map(|fbm| fbm.total_scene_count())
                                .unwrap_or_else(|| proj.scene_map.scene_count());
                            self.gui_state.char_count = proj.graph.get_characters().len();
                            self.gui_state.word_count = proj
                                .file_buffer_manager
                                .as_ref()
                                .map(|fbm| fbm.total_word_count())
                                .unwrap_or_else(|| proj.text_buffer.word_count());
                            self.gui_state.graph_needs_rebuild = true;
                        }
                    }
                }
                AgentEvent::UsageReport {
                    prompt_tokens,
                    completion_tokens,
                    context_estimate,
                } => {
                    let usage_str = if prompt_tokens > 0 || completion_tokens > 0 {
                        format!(
                            "{}K prompt / {}K completion",
                            prompt_tokens / 1000,
                            completion_tokens / 1000,
                        )
                    } else {
                        context_estimate
                    };
                    self.gui_state.last_usage = Some(usage_str);

                    // Update context window percentage
                    if prompt_tokens > 0 {
                        let max = self.gui_state.context_window_max;
                        if max > 0 {
                            self.gui_state.context_window_percent =
                                (prompt_tokens as f32 / max as f32 * 100.0).min(100.0);
                        }
                    }
                }
                AgentEvent::ConnectionTestResult(success, message) => {
                    if let Some(dialog) = &mut self.settings_dialog {
                        dialog.test_status = Some(if success {
                            TestConnectionStatus::Success(message)
                        } else {
                            TestConnectionStatus::Failure(message)
                        });
                    }
                }
            }
        }
    }

    fn send_chat(&mut self) {
        let input = self.gui_state.chat_input.trim().to_string();
        if input.is_empty() {
            return;
        }
        if !matches!(self.gui_state.agent_status, AgentStatus::Idle) {
            return;
        }

        self.gui_state.chat_history.push(ChatMessage {
            role: ChatRole::User,
            content: input.clone(),
            tool_calls: Vec::new(),
        });
        self.gui_state.chat_input.clear();
        self.gui_state.agent_status = AgentStatus::Thinking;

        if let Some(tx) = &self.gui_tx {
            let _ = tx.send(GuiRequest::Chat(input));
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input_mut(|input| {
            // Ctrl/Cmd+O → open project folder picker
            if input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::MAC_CMD,
                egui::Key::O,
            )) {
                self.trigger_open_project();
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::MAC_CMD,
                egui::Key::G,
            )) {
                self.gui_state.active_right_tab = RightTab::Graph;
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::MAC_CMD,
                egui::Key::L,
            )) {
                self.gui_state.active_right_tab = RightTab::Lint;
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::MAC_CMD,
                egui::Key::P,
            )) {
                self.gui_state.active_right_tab = RightTab::Pacing;
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::MAC_CMD,
                egui::Key::B,
            )) {
                self.gui_state.sidebar_visible = !self.gui_state.sidebar_visible;
            }
            if input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::Escape,
            )) {
                self.gui_state.active_right_tab = RightTab::Dashboard;
            }
            // Cmd/Ctrl+, → Settings
            if input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::MAC_CMD,
                egui::Key::Comma,
            )) {
                self.open_settings_dialog();
            }
        });
    }

    // -----------------------------------------------------------------------
    // Project loading / switching
    // -----------------------------------------------------------------------

    /// Open a native folder picker dialog and set pending_project_path.
    fn trigger_open_project(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            if folder.join(LAIRES_DIR).is_dir() {
                self.pending_project_path = Some(folder);
            } else {
                // Not a Laires project — offer to init via "New Project" dialog
                self.new_project_dialog = Some(NewProjectDialog {
                    title: folder
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Untitled".to_string()),
                    fountain: false,
                    target_dir: folder,
                });
            }
        }
    }

    /// Open a native folder picker for New Project.
    fn trigger_new_project(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.new_project_dialog = Some(NewProjectDialog {
                title: folder
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Untitled".to_string()),
                fountain: false,
                target_dir: folder,
            });
        }
    }

    /// Provider default settings lookup.
    fn provider_defaults(provider: &str) -> (&'static str, &'static str) {
        match provider {
            "anthropic" => ("ANTHROPIC_API_KEY", "https://api.anthropic.com"),
            "openai" => ("OPENAI_API_KEY", "https://api.openai.com/v1"),
            "gemini" => (
                "GEMINI_API_KEY",
                "https://generativelanguage.googleapis.com/v1beta/openai",
            ),
            "local" => ("", "http://localhost:11434/v1"),
            "pydantic-gateway" => ("PYDANTIC_API_KEY", "http://localhost:8000/v1"),
            _ => ("", ""),
        }
    }

    /// Populate the settings dialog from the current config.
    fn open_settings_dialog(&mut self) {
        let Some(domain) = &self.domain else { return };
        let Ok(proj) = domain.try_lock() else { return };

        let api_key_env = proj
            .config
            .llm
            .api_key_env
            .clone()
            .unwrap_or_default();

        // Read the current env var value
        let api_key = if !api_key_env.is_empty() {
            std::env::var(&api_key_env).unwrap_or_default()
        } else {
            String::new()
        };

        self.settings_dialog = Some(SettingsDialog {
            provider: proj.config.llm.provider.clone(),
            model: proj.config.llm.model.clone(),
            api_key,
            api_key_env,
            base_url: proj.config.llm.base_url.clone().unwrap_or_default(),
            test_status: None,
            error_message: None,
        });
    }

    /// Render the settings dialog window. Returns true if saved.
    fn render_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.settings_dialog else {
            return;
        };

        let mut open = true;
        let mut should_save = false;
        let mut should_test = false;
        let mut should_cancel = false;

        egui::Window::new("Settings")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                // Provider dropdown
                ui.horizontal(|ui| {
                    ui.label("Provider:");
                    let prev_provider = dialog.provider.clone();
                    egui::ComboBox::from_id_salt("provider_combo")
                        .selected_text(&dialog.provider)
                        .show_ui(ui, |ui| {
                            for p in &["anthropic", "openai", "gemini", "local", "pydantic-gateway"]
                            {
                                ui.selectable_value(&mut dialog.provider, p.to_string(), *p);
                            }
                        });
                    // Auto-fill defaults when provider changes
                    if dialog.provider != prev_provider {
                        let (env, url) = Self::provider_defaults(&dialog.provider);
                        dialog.api_key_env = env.to_string();
                        dialog.base_url = url.to_string();
                        dialog.api_key = if !env.is_empty() {
                            std::env::var(env).unwrap_or_default()
                        } else {
                            String::new()
                        };
                        dialog.test_status = None;
                        dialog.error_message = None;
                    }
                });

                // Model
                ui.horizontal(|ui| {
                    ui.label("Model:");
                    ui.text_edit_singleline(&mut dialog.model);
                });

                // API Key env var name
                ui.horizontal(|ui| {
                    ui.label("Env var:");
                    ui.text_edit_singleline(&mut dialog.api_key_env);
                });

                // API Key value (password field)
                ui.horizontal(|ui| {
                    ui.label("API Key:");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.api_key).password(true),
                    );
                });

                // Base URL
                ui.horizontal(|ui| {
                    ui.label("Base URL:");
                    ui.text_edit_singleline(&mut dialog.base_url);
                });

                ui.add_space(4.0);

                // Test status display
                if let Some(status) = &dialog.test_status {
                    match status {
                        TestConnectionStatus::Testing => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Testing connection...");
                            });
                        }
                        TestConnectionStatus::Success(msg) => {
                            ui.colored_label(egui::Color32::from_rgb(34, 139, 34), msg);
                        }
                        TestConnectionStatus::Failure(msg) => {
                            ui.colored_label(egui::Color32::from_rgb(200, 50, 50), msg);
                        }
                    }
                }

                // Error message
                if let Some(err) = &dialog.error_message {
                    ui.colored_label(egui::Color32::from_rgb(200, 50, 50), err);
                }

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Buttons
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        should_save = true;
                    }
                    if ui.button("Test Connection").clicked() {
                        should_test = true;
                    }
                    if ui.button("Cancel").clicked() {
                        should_cancel = true;
                    }
                });
            });

        if should_save {
            self.save_settings();
            return;
        }

        if should_test {
            if let Some(d) = &mut self.settings_dialog {
                d.test_status = Some(TestConnectionStatus::Testing);
            }
            // Set the env var so the test connection can read it
            if let Some(d) = &self.settings_dialog {
                if !d.api_key_env.is_empty() && !d.api_key.is_empty() {
                    // SAFETY: required unsafe in edition 2024
                    unsafe {
                        std::env::set_var(&d.api_key_env, &d.api_key);
                    }
                }
            }
            if let Some(tx) = &self.gui_tx {
                let _ = tx.send(GuiRequest::TestConnection);
            }
            return;
        }

        if !open || should_cancel {
            self.settings_dialog = None;
        }
    }

    /// Save settings: update env var, write .env, update config, send to agent.
    fn save_settings(&mut self) {
        let Some(dialog) = self.settings_dialog.take() else {
            return;
        };

        // 1. Set env var in process
        if !dialog.api_key_env.is_empty() && !dialog.api_key.is_empty() {
            // SAFETY: required unsafe in edition 2024
            unsafe {
                std::env::set_var(&dialog.api_key_env, &dialog.api_key);
            }
        }

        // 2. Persist to .env file and ensure .gitignore
        if let Some(domain) = &self.domain {
            if let Ok(proj) = domain.try_lock() {
                let env_path = proj.project_root.join(".env");
                if !dialog.api_key_env.is_empty() && !dialog.api_key.is_empty() {
                    if let Err(e) =
                        config::write_env_file(&env_path, &dialog.api_key_env, &dialog.api_key)
                    {
                        eprintln!("Warning: could not write .env: {e}");
                    }
                }
                if let Err(e) = config::ensure_gitignore_has_dotenv(&proj.project_root) {
                    eprintln!("Warning: could not update .gitignore: {e}");
                }
            }
        }

        // 3. Build updated config and save to config.toml
        let new_config = if let Some(domain) = &self.domain {
            if let Ok(proj) = domain.try_lock() {
                let mut cfg = proj.config.clone();
                cfg.llm.provider = dialog.provider.clone();
                cfg.llm.model = dialog.model.clone();
                cfg.llm.api_key_env = if dialog.api_key_env.is_empty() {
                    None
                } else {
                    Some(dialog.api_key_env.clone())
                };
                cfg.llm.base_url = if dialog.base_url.is_empty() {
                    None
                } else {
                    Some(dialog.base_url.clone())
                };
                if let Err(e) = cfg.save(&proj.project_root) {
                    eprintln!("Warning: could not save config.toml: {e}");
                }
                Some(cfg)
            } else {
                None
            }
        } else {
            None
        };

        // 4. Send to agent to switch provider
        if let Some(cfg) = new_config {
            // Update GUI display values immediately
            self.gui_state.model_name = dialog.model.clone();
            self.gui_state.privacy_label = if dialog.provider == "local" {
                "Local".to_string()
            } else {
                "Cloud".to_string()
            };

            if let Some(tx) = &self.gui_tx {
                let _ = tx.send(GuiRequest::UpdateProvider(cfg));
            }
        }
    }

    /// Called each frame: if `pending_project_path` is set, load it.
    fn process_pending_project_load(&mut self) {
        let Some(path) = self.pending_project_path.take() else {
            return;
        };

        // Drop old channels (signals agent loop to exit via channel close)
        self.gui_tx = None;
        self.gui_rx = None;
        self.domain = None;
        self.snapshot = None;

        match load_project_from_path(&path) {
            Ok((data, provider, skills, privacy, model_name, scene_count, char_count, word_count)) => {
                let title = data.config.project.title.clone();
                let domain = Arc::new(Mutex::new(data));

                // Create new channels
                let (tx_to_agent, rx_from_gui) =
                    tokio::sync::mpsc::unbounded_channel::<GuiRequest>();
                let (tx_to_gui, rx_from_agent) = std::sync::mpsc::channel::<AgentEvent>();

                // Spawn new agent
                let agent_domain = domain.clone();
                self.runtime_handle.spawn(async move {
                    agent::agent_loop(agent_domain, rx_from_gui, tx_to_gui, provider, skills).await;
                });

                self.domain = Some(domain);
                self.gui_tx = Some(tx_to_agent);
                self.gui_rx = Some(rx_from_agent);

                // Build snapshot
                if let Some(d) = &self.domain {
                    if let Ok(proj) = d.try_lock() {
                        self.snapshot = Some(build_snapshot(&proj));
                    }
                }

                // Reset GUI state for the new project
                let ctx_window_max = state::context_window_for_model(&model_name);
                self.gui_state = GuiState {
                    app_mode: AppMode::Project,
                    project_title: title.clone(),
                    privacy_label: privacy,
                    model_name,
                    scene_count,
                    char_count,
                    word_count,
                    chat_history: vec![ChatMessage {
                        role: ChatRole::System,
                        content: "Welcome to Laires. Ask me anything about your story.".to_string(),
                        tool_calls: Vec::new(),
                    }],
                    context_window_max: ctx_window_max,
                    ..GuiState::default()
                };
                self.graph_layout = panels::graph_view::GraphLayoutState::default();
                self.pending_tool_calls.clear();
                self.load_error = None;

                // Update recents
                self.recent_projects.touch(path, title);
                self.recent_projects.save();
            }
            Err(e) => {
                self.load_error = Some(format!("Could not load project: {e}"));
                // Stay in current mode (welcome or project)
            }
        }
    }

    /// Render the welcome screen (AppMode::Welcome).
    fn render_welcome_screen(&mut self, ctx: &egui::Context) {
        // Render new-project dialog window if open
        let mut should_init = false;
        let mut init_title = String::new();
        let mut init_fountain = false;
        let mut init_dir = PathBuf::new();
        let mut close_dialog = false;

        if let Some(dialog) = &mut self.new_project_dialog {
            let mut open = true;
            egui::Window::new("New Project")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Title:");
                        ui.text_edit_singleline(&mut dialog.title);
                    });
                    ui.checkbox(&mut dialog.fountain, "Fountain/screenplay format");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("Location: {}", dialog.target_dir.display()))
                            .small()
                            .color(self.theme.text_secondary),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() && !dialog.title.trim().is_empty() {
                            should_init = true;
                            init_title = dialog.title.trim().to_string();
                            init_fountain = dialog.fountain;
                            init_dir = dialog.target_dir.clone();
                        }
                        if ui.button("Cancel").clicked() {
                            close_dialog = true;
                        }
                    });
                });
            if !open {
                close_dialog = true;
            }
        }

        if close_dialog {
            self.new_project_dialog = None;
        }

        if should_init {
            self.new_project_dialog = None;
            match crate::cli::init::init_at(&init_dir, &init_title, init_fountain) {
                Ok(()) => {
                    self.pending_project_path = Some(init_dir);
                }
                Err(e) => {
                    self.load_error = Some(format!("Failed to initialize project: {e}"));
                }
            }
        }

        // Main welcome panel
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(self.theme.bg_primary)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ctx, |ui| {
                let action = welcome::render_welcome(
                    ui,
                    &self.recent_projects,
                    &self.theme,
                    &self.load_error,
                );
                match action {
                    WelcomeAction::None => {}
                    WelcomeAction::OpenProject => {
                        self.trigger_open_project();
                    }
                    WelcomeAction::NewProject => {
                        self.trigger_new_project();
                    }
                    WelcomeAction::OpenRecent(path) => {
                        self.pending_project_path = Some(path);
                    }
                }
            });
    }

    /// Render the project screen (AppMode::Project) — the existing 3-pane layout.
    fn render_project_screen(&mut self, ctx: &egui::Context) {
        // Top navigation bar
        panels::status_bar::render(ctx, &self.gui_state, &self.theme);

        // Check if scan was requested via top bar button
        let top_bar_scan = ctx.memory_mut(|mem| {
            mem.data
                .get_temp::<bool>(egui::Id::new("scan_requested"))
                .unwrap_or(false)
        });
        if top_bar_scan {
            ctx.memory_mut(|mem| {
                mem.data
                    .insert_temp(egui::Id::new("scan_requested"), false);
            });
            self.gui_state.scan_requested = true;
        }

        // Sidebar (left, collapsible)
        panels::sidebar::render(ctx, &mut self.gui_state, &self.snapshot, &self.theme);

        // Check if scan was requested via sidebar button or top bar
        if self.gui_state.scan_requested {
            self.gui_state.scan_requested = false;
            if let Some(tx) = &self.gui_tx {
                let _ = tx.send(GuiRequest::Scan);
            }
            self.gui_state.chat_history.push(ChatMessage {
                role: ChatRole::System,
                content: "Starting story scan...".to_string(),
                tool_calls: Vec::new(),
            });
            self.gui_state.agent_status = AgentStatus::Thinking;
        }

        // Chat pane (bottom panel — below content, right of sidebar)
        let mut should_send = false;
        egui::TopBottomPanel::bottom("chat_pane")
            .default_height(280.0)
            .min_height(150.0)
            .max_height(600.0)
            .resizable(true)
            .frame(
                egui::Frame::NONE
                    .fill(self.theme.bg_primary)
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .stroke(egui::Stroke {
                        width: 1.0,
                        color: self.theme.border,
                    }),
            )
            .show(ctx, |ui| {
                should_send = panels::chat::render(ui, &mut self.gui_state, &self.theme);
            });
        if should_send {
            self.send_chat();
        }

        // Handle session management requests from chat panel
        if self.gui_state.new_session_requested {
            self.gui_state.new_session_requested = false;
            if let Some(tx) = &self.gui_tx {
                let _ = tx.send(GuiRequest::NewSession);
            }
            self.gui_state.chat_history = vec![ChatMessage {
                role: ChatRole::System,
                content: "New session started.".to_string(),
                tool_calls: Vec::new(),
            }];
            self.gui_state.context_window_percent = 0.0;
            self.gui_state.agent_status = AgentStatus::Idle;
        }
        if self.gui_state.compact_context_requested {
            self.gui_state.compact_context_requested = false;
            if let Some(tx) = &self.gui_tx {
                let _ = tx.send(GuiRequest::CompactContext);
            }
            self.gui_state.chat_history.push(ChatMessage {
                role: ChatRole::System,
                content: "Compacting conversation context...".to_string(),
                tool_calls: Vec::new(),
            });
        }

        // Content area (CentralPanel — fills remaining space above chat)
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(self.theme.bg_secondary)
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ctx, |ui| {
                // Tab bar
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.gui_state.active_right_tab,
                        RightTab::Dashboard,
                        "Dashboard",
                    );
                    ui.selectable_value(
                        &mut self.gui_state.active_right_tab,
                        RightTab::Canvas,
                        "Canvas",
                    );
                    ui.selectable_value(
                        &mut self.gui_state.active_right_tab,
                        RightTab::Graph,
                        "Graph",
                    );
                    ui.selectable_value(
                        &mut self.gui_state.active_right_tab,
                        RightTab::Lint,
                        "Lint",
                    );
                    ui.selectable_value(
                        &mut self.gui_state.active_right_tab,
                        RightTab::Pacing,
                        "Pacing",
                    );
                });
                ui.separator();

                match self.gui_state.active_right_tab {
                    RightTab::Dashboard => {
                        panels::dashboard::render(
                            ui,
                            &mut self.gui_state,
                            &self.snapshot,
                            &self.theme,
                        );
                    }
                    RightTab::Canvas => {
                        if self.gui_state.analysis_sidebar_visible {
                            // Split: canvas on left, analysis sidebar on right
                            let sidebar_w = panels::analysis_sidebar::SIDEBAR_WIDTH;
                            let canvas_w = (ui.available_width() - sidebar_w - 12.0).max(200.0);

                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(canvas_w, ui.available_height()),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| {
                                        panels::canvas::render(
                                            ui,
                                            &mut self.gui_state,
                                            &self.snapshot,
                                            &self.theme,
                                        );
                                    },
                                );

                                ui.add_space(4.0);

                                // Analysis sidebar in a card frame
                                ui.allocate_ui_with_layout(
                                    egui::vec2(sidebar_w, ui.available_height()),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| {
                                        self.theme.card_frame().show(ui, |ui| {
                                            panels::analysis_sidebar::render(
                                                ui,
                                                &mut self.gui_state,
                                                &self.snapshot,
                                                &self.theme,
                                            );
                                        });
                                    },
                                );
                            });
                        } else {
                            panels::canvas::render(
                                ui,
                                &mut self.gui_state,
                                &self.snapshot,
                                &self.theme,
                            );
                        }
                    }
                    RightTab::Graph => {
                        panels::graph_view::render(
                            ui,
                            &mut self.gui_state,
                            &self.snapshot,
                            &mut self.graph_layout,
                            &self.theme,
                        );
                    }
                    RightTab::Lint => {
                        ui.label(
                            RichText::new("Lint view — coming soon")
                                .color(self.theme.text_secondary)
                                .italics(),
                        );
                    }
                    RightTab::Pacing => {
                        ui.label(
                            RichText::new("Pacing view — coming soon")
                                .color(self.theme.text_secondary)
                                .italics(),
                        );
                    }
                }
            });
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process pending project load (set last frame)
        self.process_pending_project_load();

        // Handle "switch to welcome" request from status bar click (via ctx memory)
        let switch = ctx.memory_mut(|mem| {
            mem.data.get_temp::<bool>(egui::Id::new("switch_to_welcome")).unwrap_or(false)
        });
        if switch {
            ctx.memory_mut(|mem| mem.data.insert_temp(egui::Id::new("switch_to_welcome"), false));
            self.gui_state.app_mode = AppMode::Welcome;
        }

        // Handle "open settings" request from status bar gear icon (via ctx memory)
        let open_settings = ctx.memory_mut(|mem| {
            mem.data.get_temp::<bool>(egui::Id::new("open_settings")).unwrap_or(false)
        });
        if open_settings {
            ctx.memory_mut(|mem| mem.data.insert_temp(egui::Id::new("open_settings"), false));
            self.open_settings_dialog();
        }

        // Poll agent events (only meaningful when in Project mode)
        self.process_agent_events();

        // Handle keyboard shortcuts
        self.handle_shortcuts(ctx);

        match self.gui_state.app_mode {
            AppMode::Welcome => self.render_welcome_screen(ctx),
            AppMode::Project => self.render_project_screen(ctx),
        }

        // Render settings dialog on top of everything
        self.render_settings_dialog(ctx);

        // Keep polling when agent is active
        let poll_interval = match self.gui_state.agent_status {
            AgentStatus::Idle => Duration::from_millis(500),
            _ => Duration::from_millis(50),
        };
        ctx.request_repaint_after(poll_interval);
    }
}

fn build_snapshot(proj: &ProjectData) -> ProjectSnapshot {
    let story_text = proj.text_buffer.read_all();
    let scenes = proj.scene_map.list_scenes();

    let scene_titles: Vec<(String, Option<String>)> = scenes
        .iter()
        .map(|s| (s.id.clone(), s.title.clone()))
        .collect();

    // Build grouped scene list from FBM if available, otherwise fall back to single scene_map
    let all_scenes = if let Some(ref fbm) = proj.file_buffer_manager {
        fbm.entries()
            .iter()
            .map(|entry| FileSceneGroup {
                file_path: entry.file_path.clone(),
                scenes: entry
                    .scene_map
                    .list_scenes()
                    .iter()
                    .map(|s| (s.id.clone(), s.title.clone()))
                    .collect(),
            })
            .filter(|g| !g.scenes.is_empty())
            .collect()
    } else if !scene_titles.is_empty() {
        vec![FileSceneGroup {
            file_path: String::new(),
            scenes: scene_titles.clone(),
        }]
    } else {
        Vec::new()
    };

    let mut boundary_lines = std::collections::HashSet::new();
    let mut byte_offset = 0;
    for (line_idx, line) in story_text.lines().enumerate() {
        for scene in scenes {
            if byte_offset >= scene.start && byte_offset <= scene.start + line.len() {
                boundary_lines.insert(line_idx);
            }
        }
        byte_offset += line.len() + 1;
    }

    // Build per-file text data for file-aware canvas
    let mut file_texts = std::collections::HashMap::new();
    if let Some(ref fbm) = proj.file_buffer_manager {
        for entry in fbm.entries() {
            let text = entry.text_buffer.read_all();
            let file_scenes = entry.scene_map.list_scenes();
            let mut file_boundaries = std::collections::HashSet::new();
            let mut bo = 0;
            for (line_idx, line) in text.lines().enumerate() {
                for scene in file_scenes {
                    if bo >= scene.start && bo <= scene.start + line.len() {
                        file_boundaries.insert(line_idx);
                    }
                }
                bo += line.len() + 1;
            }
            let wc = entry.text_buffer.word_count();
            file_texts.insert(
                entry.file_path.clone(),
                FileTextData {
                    text,
                    boundary_lines: file_boundaries,
                    word_count: wc,
                },
            );
        }
    }

    let story_files = proj
        .manifest
        .as_ref()
        .map(|m| m.story_files.iter().map(|sf| sf.path.clone()).collect())
        .unwrap_or_default();

    let context_files = proj
        .manifest
        .as_ref()
        .map(|m| m.context_files.iter().map(|cf| cf.path.clone()).collect())
        .unwrap_or_default();

    // Extract graph data
    let inner = proj.graph.inner_graph();
    let graph_nodes: Vec<panels::graph_view::GraphNodeInfo> = inner
        .node_indices()
        .filter_map(|idx| {
            inner.node_weight(idx).map(|node| {
                let label = match node {
                    crate::concepts::narrative_graph::GraphNode::Character { name, .. } => {
                        name.clone()
                    }
                    crate::concepts::narrative_graph::GraphNode::Objective {
                        description, ..
                    } => {
                        if description.len() > 30 {
                            format!("{}...", &description[..27])
                        } else {
                            description.clone()
                        }
                    }
                    crate::concepts::narrative_graph::GraphNode::Scene { title, id, .. } => {
                        title.clone().unwrap_or_else(|| id.clone())
                    }
                    crate::concepts::narrative_graph::GraphNode::Conflict {
                        description, ..
                    } => {
                        if description.len() > 30 {
                            format!("{}...", &description[..27])
                        } else {
                            description.clone()
                        }
                    }
                };
                panels::graph_view::GraphNodeInfo {
                    id: node.node_id().to_string(),
                    label,
                    node_type: node.node_type_name().to_string(),
                }
            })
        })
        .collect();

    let graph_edges: Vec<panels::graph_view::GraphEdgeInfo> = inner
        .edge_indices()
        .filter_map(|eidx| {
            let (src_idx, dst_idx) = inner.edge_endpoints(eidx)?;
            let src_node = inner.node_weight(src_idx)?;
            let dst_node = inner.node_weight(dst_idx)?;
            let edge = inner.edge_weight(eidx)?;
            let label = match edge {
                crate::concepts::narrative_graph::GraphEdge::Pursues { .. } => "pursues",
                crate::concepts::narrative_graph::GraphEdge::DecomposesInto => "decomposes",
                crate::concepts::narrative_graph::GraphEdge::ConflictsWith => "conflicts",
                crate::concepts::narrative_graph::GraphEdge::PresentIn => "present_in",
                crate::concepts::narrative_graph::GraphEdge::Advances => "advances",
                crate::concepts::narrative_graph::GraphEdge::Blocks => "blocks",
                crate::concepts::narrative_graph::GraphEdge::Precedes => "precedes",
                crate::concepts::narrative_graph::GraphEdge::Transforms { .. } => "transforms",
            };
            Some(panels::graph_view::GraphEdgeInfo {
                source: src_node.node_id().to_string(),
                target: dst_node.node_id().to_string(),
                label: label.to_string(),
            })
        })
        .collect();

    ProjectSnapshot {
        story_text,
        all_scenes,
        scene_boundary_lines: boundary_lines,
        file_texts,
        story_files,
        context_files,
        graph_nodes,
        graph_edges,
    }
}

/// Load project data from a specific path.
fn load_project_from_path(
    start: &Path,
) -> anyhow::Result<(ProjectData, Provider, Skills, String, String, usize, usize, usize)> {
    let project_root = config::find_project_root(start)
        .ok_or_else(|| anyhow::anyhow!("No .laires/ directory found at {}", start.display()))?;

    let config = ProjectConfig::load(&project_root)?;
    let story_path = config::story_file_path(&project_root, &config.project.format);

    if !story_path.exists() {
        anyhow::bail!("Story file not found: {}", story_path.display());
    }

    let text_buffer = TextBuffer::from_file(story_path)?;
    let full_text = text_buffer.read_all();

    let parse_mode = match config.project.format.as_str() {
        "fountain" => ParseMode::Fountain,
        _ => ParseMode::Prose,
    };
    let mut scene_map = SceneMap::new(parse_mode);
    scene_map.full_reindex(&full_text, "");

    let graph_path = project_root.join(LAIRES_DIR).join("graph.json");
    let graph = if graph_path.exists() {
        NarrativeGraph::load(&graph_path).unwrap_or_default()
    } else {
        NarrativeGraph::new()
    };

    let overrides_path = project_root.join(LAIRES_DIR).join(OVERRIDES_FILE);
    let intent = if overrides_path.exists() {
        DeclaredIntent::load(&overrides_path).unwrap_or_default()
    } else {
        DeclaredIntent::new()
    };

    let perspectives_dir = project_root.join(LAIRES_DIR).join(PERSPECTIVES_CACHE_DIR);
    let perspectives_path = perspectives_dir.join("perspectives.json");
    let graph_hash = blake3::hash(graph.serialize_compact().as_bytes())
        .to_hex()
        .to_string();
    let perspectives = if perspectives_path.exists() {
        let mut p = CharacterPerspective::load(&perspectives_path).unwrap_or_default();
        p.invalidate_by_graph_hash(&graph_hash);
        p
    } else {
        CharacterPerspective::new()
    };

    let manifest = Manifest::load(&project_root).ok();

    // Build FileBufferManager from manifest if available
    let file_buffer_manager = manifest
        .as_ref()
        .and_then(|m| FileBufferManager::from_manifest(m, &project_root).ok());

    let provider = Provider::from_project_config(&config)?;
    let privacy = if provider.is_local() { "Local" } else { "Cloud" }.to_string();
    let model_name = provider.model_name().to_string();

    // Use FBM counts when available, otherwise fall back to single scene_map/text_buffer
    let scene_count = file_buffer_manager
        .as_ref()
        .map(|fbm| fbm.total_scene_count())
        .unwrap_or_else(|| scene_map.scene_count());
    let char_count = graph.get_characters().len();
    let word_count = file_buffer_manager
        .as_ref()
        .map(|fbm| fbm.total_word_count())
        .unwrap_or_else(|| text_buffer.word_count());

    let canvas = Canvas::new(24, 80);

    let mut skills = Skills::new();
    if !provider.is_local() {
        for restricted in &config.privacy.restricted_when_cloud {
            skills.set_permission(restricted, Permission::Disabled);
        }
    }

    let data = ProjectData {
        text_buffer,
        scene_map,
        graph,
        intent,
        perspectives,
        canvas,
        manifest,
        file_buffer_manager,
        project_root,
        config,
    };

    Ok((data, provider, skills, privacy, model_name, scene_count, char_count, word_count))
}

pub async fn run_gui(project_path: Option<PathBuf>) -> anyhow::Result<()> {
    let runtime_handle = tokio::runtime::Handle::current();

    // Load + prune recent projects
    let mut recent_projects = RecentProjects::load();
    recent_projects.prune();
    recent_projects.save();

    // Determine starting path: explicit arg > cwd detection > welcome screen
    let start_path = match project_path {
        Some(p) => Some(p),
        None => {
            let cwd = std::env::current_dir().ok();
            cwd.and_then(|d| config::find_project_root(&d))
        }
    };

    // Try to load the project if we have a start path
    let (domain, gui_tx, gui_rx, gui_state) = if let Some(path) = start_path {
        match load_project_from_path(&path) {
            Ok((data, provider, skills, privacy, model_name, scene_count, char_count, word_count)) => {
                let title = data.config.project.title.clone();
                let mut gs = GuiState::default();
                gs.app_mode = AppMode::Project;
                gs.project_title = title.clone();
                gs.privacy_label = privacy;
                gs.model_name = model_name.clone();
                gs.scene_count = scene_count;
                gs.char_count = char_count;
                gs.word_count = word_count;
                gs.context_window_max = state::context_window_for_model(&model_name);
                gs.chat_history = vec![ChatMessage {
                    role: ChatRole::System,
                    content: "Welcome to Laires. Ask me anything about your story.".to_string(),
                    tool_calls: Vec::new(),
                }];

                let domain = Arc::new(Mutex::new(data));

                // Create channels
                let (tx_to_agent, rx_from_gui) =
                    tokio::sync::mpsc::unbounded_channel::<GuiRequest>();
                let (tx_to_gui, rx_from_agent) = std::sync::mpsc::channel::<AgentEvent>();

                // Spawn agent on the current tokio runtime
                let agent_domain = domain.clone();
                tokio::spawn(async move {
                    agent::agent_loop(agent_domain, rx_from_gui, tx_to_gui, provider, skills).await;
                });

                // Update recents
                recent_projects.touch(path, title);
                recent_projects.save();

                (Some(domain), Some(tx_to_agent), Some(rx_from_agent), gs)
            }
            Err(e) => {
                eprintln!("Warning: Could not load project: {e}");
                let gs = GuiState::default(); // app_mode defaults to Welcome
                (None, None, None, gs)
            }
        }
    } else {
        // No project path — start with welcome screen
        let gs = GuiState::default();
        (None, None, None, gs)
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("Laires"),
        ..Default::default()
    };

    eframe::run_native(
        "Laires",
        options,
        Box::new(move |cc| {
            Ok(Box::new(GuiApp::new(
                cc,
                domain,
                gui_state,
                gui_tx,
                gui_rx,
                recent_projects,
                runtime_handle,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {e}"))?;

    Ok(())
}
