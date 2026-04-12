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

use crate::concepts::character_perspective::CharacterPerspective;
use crate::config::{self, LAIRES_DIR, ProjectConfig};
use crate::runtime::project_loader::{LoadedProject, ProjectLoadResult, load_project};
use crate::runtime::story_access::StoryAccess;

use state::{
    AgentEvent, AgentStatus, AppMode, ChatMessage, ChatRole, GuiRequest, GuiState, RightTab,
    SessionMode, StoryChangeReview, StoryChangeScope, ToolCallDisplay,
};
use theme::LairesTheme;
use welcome::{RecentProjects, WelcomeAction};

/// Infer the default session mode from project config and manifest.
///
/// Priority: explicit config > manifest heuristic (all editable = Workshop, else Consultant).
fn infer_session_mode(
    config: &ProjectConfig,
    manifest: Option<&crate::concepts::manifest::Manifest>,
) -> SessionMode {
    // Check explicit config
    if let Some(ref mode_str) = config.project.default_mode {
        match mode_str.to_lowercase().as_str() {
            "workshop" => return SessionMode::Workshop,
            "consultant" => return SessionMode::Consultant,
            _ => {} // fall through to inference
        }
    }
    // Infer from manifest: if all story files are editable, default Workshop
    if let Some(m) = manifest {
        if !m.story_files.is_empty() && m.story_files.iter().all(|f| f.editable) {
            return SessionMode::Workshop;
        }
    }
    SessionMode::Consultant
}

/// Loaded project data — shared between GUI and agent via Arc<Mutex<>>.
pub(crate) struct ProjectData {
    pub text_buffer: crate::concepts::text_buffer::TextBuffer,
    pub scene_map: crate::concepts::scene_map::SceneMap,
    pub graph: crate::concepts::narrative_graph::NarrativeGraph,
    pub intent: crate::concepts::declared_intent::DeclaredIntent,
    pub perspectives: CharacterPerspective,
    pub manifest: Option<crate::concepts::manifest::Manifest>,
    pub file_buffer_manager: Option<crate::concepts::file_buffer_manager::FileBufferManager>,
    pub project_root: PathBuf,
    pub config: ProjectConfig,
    /// Accumulated revision brief for Consultant mode.
    pub revision_brief: Option<crate::concepts::revision_brief::RevisionBrief>,
    /// Pending review scope from story edits that have not yet been re-analyzed.
    pub pending_change_scope: Option<StoryChangeScope>,
    /// Latest completed deterministic before/after review from an incremental edit pass.
    pub latest_change_review: Option<StoryChangeReview>,
}

/// State for the "New Project" / "Initialize" dialog.
struct NewProjectDialog {
    title: String,
    target_dir: PathBuf,
    /// Contextual message shown at the top of the dialog (varies by folder state).
    message: String,
    /// Label for the primary action button ("Create" or "Initialize").
    button_label: String,
}

/// Connection test status for the settings dialog.
#[derive(Debug, Clone)]
enum TestConnectionStatus {
    Testing,
    Success(String),
    Failure(String),
}

/// Cached settings for a single provider (used to restore when switching back).
#[derive(Clone)]
struct ProviderSettings {
    model: String,
    api_key_env: String,
    api_key: String,
    base_url: String,
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
    /// Timestamp of last canvas edit (for debounced save).
    canvas_last_edit: Option<std::time::Instant>,

    // --- Project picker state ---
    recent_projects: RecentProjects,
    runtime_handle: tokio::runtime::Handle,
    pending_project_path: Option<PathBuf>,
    load_error: Option<String>,
    new_project_dialog: Option<NewProjectDialog>,
    settings_dialog: Option<SettingsDialog>,
    /// Remembers settings per provider so switching back restores them across dialog sessions.
    provider_history: std::collections::HashMap<String, ProviderSettings>,
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
    pending_scene_ids: std::collections::HashSet<String>,
    pending_story_files: std::collections::HashSet<String>,
    pending_scene_count: usize,
    pending_change_scope: Option<StoryChangeScope>,
    latest_change_review: Option<StoryChangeReview>,
    /// Brief revision count and rendered markdown (for Brief panel).
    brief_revision_count: usize,
    brief_markdown: String,
    /// Whether the currently selected file supports Preview mode (.md / .fountain).
    preview_available: bool,
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
        let snapshot = domain
            .as_ref()
            .and_then(|d| d.try_lock().ok().map(|proj| build_snapshot(&proj)));

        let review_pending = snapshot
            .as_ref()
            .map(|snap| snap.pending_scene_count > 0)
            .unwrap_or(false);
        let review_status_text = build_review_status_text(snapshot.as_ref());

        Self {
            gui_state,
            theme,
            domain,
            snapshot,
            graph_layout: panels::graph_view::GraphLayoutState::default(),
            gui_tx,
            gui_rx,
            pending_tool_calls: Vec::new(),
            canvas_last_edit: None,
            recent_projects,
            runtime_handle,
            pending_project_path: None,
            load_error: None,
            new_project_dialog: None,
            settings_dialog: None,
            provider_history: std::collections::HashMap::new(),
        }
        .with_review_state(review_pending, review_status_text)
    }

    fn with_review_state(
        mut self,
        review_pending: bool,
        review_status_text: Option<String>,
    ) -> Self {
        self.gui_state.review_pending = review_pending;
        self.gui_state.review_status_text = review_status_text;
        self
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
                AgentEvent::ToolResult {
                    name,
                    result_summary,
                } => {
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
                            let story = StoryAccess::new(
                                &proj.text_buffer,
                                &proj.scene_map,
                                proj.file_buffer_manager.as_ref(),
                            );
                            self.gui_state.scene_count = story.scene_count();
                            self.gui_state.char_count = proj.graph.get_characters().len();
                            self.gui_state.word_count = story.word_count();
                            self.gui_state.graph_needs_rebuild = true;
                            self.gui_state.review_pending = self
                                .snapshot
                                .as_ref()
                                .map(|snap| snap.pending_scene_count > 0)
                                .unwrap_or(false);
                            self.gui_state.review_status_text =
                                build_review_status_text(self.snapshot.as_ref());
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

    /// Flush dirty canvas edits to the agent loop (debounced: 1s after last keystroke).
    fn flush_canvas_edits(&mut self) {
        let Some(last) = self.canvas_last_edit else {
            return;
        };
        if last.elapsed() < Duration::from_secs(1) {
            return;
        }
        if let Some(tx) = &self.gui_tx {
            let _ = tx.send(GuiRequest::CanvasTextChanged {
                file: self.gui_state.canvas_edit_file.clone(),
                text: self.gui_state.canvas_edit_text.clone(),
            });
        }
        self.canvas_last_edit = None;
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
            // Cmd/Ctrl+F → Focus search bar
            if input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL | egui::Modifiers::MAC_CMD,
                egui::Key::F,
            )) {
                self.gui_state.search_active = true;
                ctx.memory_mut(|mem| {
                    mem.request_focus(egui::Id::new(
                        panels::status_bar::SEARCH_INPUT_ID,
                    ));
                });
            }
        });
    }

    // -----------------------------------------------------------------------
    // Project loading / switching
    // -----------------------------------------------------------------------

    /// Open a native folder picker dialog. Handles three cases:
    /// 1. Folder has `.laires/` — load directly as existing project.
    /// 2. Folder has files but no `.laires/` — offer to initialize.
    /// 3. Empty folder — offer to create a new Laires project.
    fn trigger_open_project(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            let folder_name = folder
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".to_string());

            if folder.join(LAIRES_DIR).is_dir() {
                // Already a Laires project — load it
                self.pending_project_path = Some(folder);
            } else {
                let is_empty = std::fs::read_dir(&folder)
                    .map(|mut entries| entries.next().is_none())
                    .unwrap_or(true);

                if is_empty {
                    self.new_project_dialog = Some(NewProjectDialog {
                        title: folder_name,
                        target_dir: folder,
                        message: "Create a new Laires project in this folder. \
                                  Laires will set up your project structure and a starter story file."
                            .to_string(),
                        button_label: "Create".to_string(),
                    });
                } else {
                    self.new_project_dialog = Some(NewProjectDialog {
                        title: folder_name,
                        target_dir: folder,
                        message: "Would you like to initialize this folder as a Laires project? \
                                  Laires will discover and analyze your existing files."
                            .to_string(),
                        button_label: "Initialize".to_string(),
                    });
                }
            }
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

    /// Sensible default model for each provider.
    fn default_model_for_provider(provider: &str) -> &'static str {
        match provider {
            "anthropic" => "claude-sonnet-4-6",
            "openai" => "gpt-4o",
            "gemini" => "gemini-2.5-flash",
            "local" => "llama3.2",
            "pydantic-gateway" => "gpt-4o",
            _ => "",
        }
    }

    /// Populate the settings dialog from the current config.
    fn open_settings_dialog(&mut self) {
        let Some(domain) = &self.domain else { return };
        let Ok(proj) = domain.try_lock() else { return };

        let api_key_env = proj.config.llm.api_key_env.clone().unwrap_or_default();

        // Read the current env var value
        let api_key = if !api_key_env.is_empty() {
            std::env::var(&api_key_env).unwrap_or_default()
        } else {
            String::new()
        };

        // Always update the current provider's settings in history so switching
        // away and back restores them (including the actual model name).
        let current_provider = proj.config.llm.provider.clone();
        self.provider_history.insert(
            current_provider.clone(),
            ProviderSettings {
                model: proj.config.llm.model.clone(),
                api_key_env: api_key_env.clone(),
                api_key: api_key.clone(),
                base_url: proj.config.llm.base_url.clone().unwrap_or_default(),
            },
        );

        self.settings_dialog = Some(SettingsDialog {
            provider: current_provider,
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
        // Track provider change so we can update history after the closure
        let mut provider_changed_from: Option<String> = None;

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
                    if dialog.provider != prev_provider {
                        provider_changed_from = Some(prev_provider);
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
                    ui.add(egui::TextEdit::singleline(&mut dialog.api_key).password(true));
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

        // Handle provider switch: save old settings to history, restore or default new ones.
        // Done here (outside the egui closure) so we can access self.provider_history.
        if let Some(old_provider) = provider_changed_from {
            if let Some(dialog) = &mut self.settings_dialog {
                // Save the old provider's settings
                self.provider_history.insert(
                    old_provider,
                    ProviderSettings {
                        model: dialog.model.clone(),
                        api_key_env: dialog.api_key_env.clone(),
                        api_key: dialog.api_key.clone(),
                        base_url: dialog.base_url.clone(),
                    },
                );

                // Restore saved settings or fill defaults
                if let Some(saved) = self.provider_history.get(&dialog.provider) {
                    dialog.model = saved.model.clone();
                    dialog.api_key_env = saved.api_key_env.clone();
                    dialog.api_key = saved.api_key.clone();
                    dialog.base_url = saved.base_url.clone();
                } else {
                    let (env, url) = Self::provider_defaults(&dialog.provider);
                    dialog.api_key_env = env.to_string();
                    dialog.base_url = url.to_string();
                    dialog.api_key = if !env.is_empty() {
                        std::env::var(env).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    dialog.model =
                        Self::default_model_for_provider(&dialog.provider).to_string();
                }
            }
        }

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
            // Build a config from the dialog so the agent tests the NEW settings
            if let (Some(d), Some(domain)) = (&self.settings_dialog, &self.domain) {
                if let Ok(proj) = domain.try_lock() {
                    let mut test_cfg = proj.config.clone();
                    test_cfg.llm.provider = d.provider.clone();
                    test_cfg.llm.model = d.model.clone();
                    test_cfg.llm.api_key_env = if d.api_key_env.is_empty() {
                        None
                    } else {
                        Some(d.api_key_env.clone())
                    };
                    test_cfg.llm.base_url = if d.base_url.is_empty() {
                        None
                    } else {
                        Some(d.base_url.clone())
                    };
                    if let Some(tx) = &self.gui_tx {
                        let _ = tx.send(GuiRequest::TestConnection(test_cfg));
                    }
                }
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
            Ok(load_result) => {
                let data = into_gui_project_data(load_result.project);
                let provider = load_result.provider;
                let skills = load_result.skills;
                let summary = load_result.summary;
                let title = data.config.project.title.clone();
                let mode = infer_session_mode(&data.config, data.manifest.as_ref());
                let domain = Arc::new(Mutex::new(data));

                // Create new channels
                let (tx_to_agent, rx_from_gui) =
                    tokio::sync::mpsc::unbounded_channel::<GuiRequest>();
                let (tx_to_gui, rx_from_agent) = std::sync::mpsc::channel::<AgentEvent>();

                // Spawn new agent
                let agent_domain = domain.clone();
                self.runtime_handle.spawn(async move {
                    agent::agent_loop(agent_domain, rx_from_gui, tx_to_gui, provider, skills, mode)
                        .await;
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
                let ctx_window_max = state::context_window_for_model(&summary.model_name);
                self.gui_state = GuiState {
                    app_mode: AppMode::Project,
                    session_mode: mode,
                    project_title: title.clone(),
                    privacy_label: summary.privacy_label,
                    model_name: summary.model_name,
                    scene_count: summary.scene_count,
                    char_count: summary.char_count,
                    word_count: summary.word_count,
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
        let mut init_dir = PathBuf::new();
        let mut close_dialog = false;

        if let Some(dialog) = &mut self.new_project_dialog {
            let mut open = true;
            egui::Window::new("Open Project")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(&dialog.message)
                            .size(13.0)
                            .color(self.theme.text_secondary),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Title:");
                        ui.text_edit_singleline(&mut dialog.title);
                    });
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("Location: {}", dialog.target_dir.display()))
                            .small()
                            .color(self.theme.text_secondary),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(&dialog.button_label).clicked()
                            && !dialog.title.trim().is_empty()
                        {
                            should_init = true;
                            init_title = dialog.title.trim().to_string();
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
            match crate::cli::init::init_at(&init_dir, &init_title, false) {
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
                    WelcomeAction::OpenRecent(path) => {
                        self.pending_project_path = Some(path);
                    }
                }
            });
    }

    /// Render the project screen (AppMode::Project) — the existing 3-pane layout.
    fn render_project_screen(&mut self, ctx: &egui::Context) {
        // Top navigation bar
        panels::status_bar::render(ctx, &mut self.gui_state, &self.snapshot, &self.theme);

        // Check if scan was requested via top bar button
        let top_bar_scan = ctx.memory_mut(|mem| {
            mem.data
                .get_temp::<bool>(egui::Id::new("scan_requested"))
                .unwrap_or(false)
        });
        if top_bar_scan {
            ctx.memory_mut(|mem| {
                mem.data.insert_temp(egui::Id::new("scan_requested"), false);
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
                ui.set_max_width(ui.available_width());

                // Tab bar (mode-aware)
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

                    // Consultant mode: show Brief tab
                    if self.gui_state.session_mode == SessionMode::Consultant {
                        let brief_label = if let Some(snap) = &self.snapshot {
                            if snap.brief_revision_count > 0 {
                                format!("Brief ({})", snap.brief_revision_count)
                            } else {
                                "Brief".to_string()
                            }
                        } else {
                            "Brief".to_string()
                        };
                        ui.selectable_value(
                            &mut self.gui_state.active_right_tab,
                            RightTab::Brief,
                            brief_label,
                        );
                    }

                    // Workshop mode: show Lint and Pacing tabs
                    if self.gui_state.session_mode == SessionMode::Workshop {
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
                    }
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
                            // Capture available height before entering horizontal layout,
                            // because ui.horizontal() doesn't propagate parent height.
                            let avail_h = ui.available_height();

                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(canvas_w, avail_h),
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
                                    egui::vec2(sidebar_w, avail_h),
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
                        if self.gui_state.selected_node_id.is_some() && self.snapshot.is_some() {
                            // Split: graph on left, inspector on right
                            let inspector_w = 280.0_f32;
                            let graph_w = (ui.available_width() - inspector_w - 12.0).max(200.0);
                            let avail_h = ui.available_height();

                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(graph_w, avail_h),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| {
                                        panels::graph_view::render(
                                            ui,
                                            &mut self.gui_state,
                                            &self.snapshot,
                                            &mut self.graph_layout,
                                            &self.theme,
                                        );
                                    },
                                );

                                ui.add_space(4.0);

                                // Inspector in a card frame
                                ui.allocate_ui_with_layout(
                                    egui::vec2(inspector_w, avail_h),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| {
                                        self.theme.card_frame().show(ui, |ui| {
                                            panels::inspector::render(
                                                ui,
                                                &mut self.gui_state,
                                                self.snapshot.as_ref().unwrap(),
                                                &self.theme,
                                            );
                                        });
                                    },
                                );
                            });
                        } else {
                            panels::graph_view::render(
                                ui,
                                &mut self.gui_state,
                                &self.snapshot,
                                &mut self.graph_layout,
                                &self.theme,
                            );
                        }
                    }
                    RightTab::Brief => {
                        panels::brief::render(ui, &self.gui_state, &self.snapshot, &self.theme);
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
            mem.data
                .get_temp::<bool>(egui::Id::new("switch_to_welcome"))
                .unwrap_or(false)
        });
        if switch {
            ctx.memory_mut(|mem| {
                mem.data
                    .insert_temp(egui::Id::new("switch_to_welcome"), false)
            });
            self.gui_state.app_mode = AppMode::Welcome;
        }

        // Handle "open settings" request from status bar gear icon (via ctx memory)
        let open_settings = ctx.memory_mut(|mem| {
            mem.data
                .get_temp::<bool>(egui::Id::new("open_settings"))
                .unwrap_or(false)
        });
        if open_settings {
            ctx.memory_mut(|mem| mem.data.insert_temp(egui::Id::new("open_settings"), false));
            self.open_settings_dialog();
        }

        // Handle mode switch from status bar badge (via ctx memory bool flag)
        let mode_switch_requested = ctx.memory_mut(|mem| {
            mem.data
                .get_temp::<bool>(egui::Id::new("switch_mode"))
                .unwrap_or(false)
        });
        if mode_switch_requested {
            ctx.memory_mut(|mem| mem.data.insert_temp(egui::Id::new("switch_mode"), false));
            let new_mode = match self.gui_state.session_mode {
                SessionMode::Consultant => SessionMode::Workshop,
                SessionMode::Workshop => SessionMode::Consultant,
            };
            self.gui_state.session_mode = new_mode;
            // Notify agent of mode change
            if let Some(tx) = &self.gui_tx {
                let _ = tx.send(GuiRequest::SwitchMode(new_mode));
            }
            // Add system message to chat
            self.gui_state.chat_history.push(state::ChatMessage {
                role: state::ChatRole::System,
                content: format!("Switched to **{}** mode.", new_mode),
                tool_calls: Vec::new(),
            });
        }

        // Poll agent events (only meaningful when in Project mode)
        self.process_agent_events();

        // Track canvas dirty timestamp and flush debounced edits.
        // canvas_dirty is set each frame the TextEdit reports a change.
        // We consume it here and (re)start the debounce timer.
        if self.gui_state.canvas_dirty {
            self.canvas_last_edit = Some(std::time::Instant::now());
            self.gui_state.canvas_dirty = false;
        }
        self.flush_canvas_edits();

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

    let pending_change_scope = proj.pending_change_scope.clone();
    let pending_scene_ids: std::collections::HashSet<String> = pending_change_scope
        .as_ref()
        .map(|scope| scope.changed_scene_ids.iter().cloned().collect())
        .unwrap_or_default();
    let pending_story_files: std::collections::HashSet<String> = pending_change_scope
        .as_ref()
        .map(|scope| scope.changed_files.iter().cloned().collect())
        .unwrap_or_default();
    let pending_scene_count = pending_scene_ids.len();

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
    let graph_nodes: Vec<panels::graph_view::SnapshotNode> = inner
        .node_indices()
        .filter_map(|idx| {
            inner.node_weight(idx).map(|node| {
                use crate::concepts::narrative_graph::GraphNode;
                use panels::graph_view::NodeDetail;

                let (label, detail) = match node {
                    GraphNode::Character {
                        name,
                        aliases,
                        description,
                        ..
                    } => (
                        name.clone(),
                        NodeDetail::Character {
                            name: name.clone(),
                            aliases: aliases.clone(),
                            description: description.clone(),
                        },
                    ),
                    GraphNode::Objective {
                        character_id,
                        scope,
                        description,
                        evidence,
                        confidence,
                        status,
                        ..
                    } => {
                        let label = if description.len() > 30 {
                            format!("{}...", &description[..27])
                        } else {
                            description.clone()
                        };
                        (
                            label,
                            NodeDetail::Objective {
                                character_id: character_id.clone(),
                                scope: format!("{:?}", scope),
                                description: description.clone(),
                                evidence: evidence.clone(),
                                confidence: *confidence,
                                status: format!("{:?}", status),
                            },
                        )
                    }
                    GraphNode::Scene {
                        id,
                        title,
                        summary,
                        characters_present,
                        location,
                        time,
                        file_path,
                        ..
                    } => {
                        let label = title.clone().unwrap_or_else(|| id.clone());
                        (
                            label,
                            NodeDetail::Scene {
                                title: title.clone(),
                                summary: summary.clone(),
                                characters_present: characters_present.clone(),
                                location: location.clone(),
                                time: time.clone(),
                                file_path: file_path.clone(),
                            },
                        )
                    }
                    GraphNode::Conflict {
                        description,
                        objectives,
                        ..
                    } => {
                        let label = if description.len() > 30 {
                            format!("{}...", &description[..27])
                        } else {
                            description.clone()
                        };
                        (
                            label,
                            NodeDetail::Conflict {
                                description: description.clone(),
                                objectives: objectives.clone(),
                            },
                        )
                    }
                };
                panels::graph_view::SnapshotNode {
                    id: node.node_id().to_string(),
                    label,
                    node_type: node.node_type_name().to_string(),
                    detail,
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
        pending_scene_count,
        pending_scene_ids,
        pending_story_files,
        pending_change_scope,
        latest_change_review: proj.latest_change_review.clone(),
        brief_revision_count: proj
            .revision_brief
            .as_ref()
            .map(|b| b.revision_count())
            .unwrap_or(0),
        brief_markdown: proj
            .revision_brief
            .as_ref()
            .filter(|b| !b.is_empty())
            .map(|b| b.to_markdown())
            .unwrap_or_default(),
        preview_available: true, // determined per-file in canvas render
    }
}

fn into_gui_project_data(project: LoadedProject) -> ProjectData {
    let title = project.config.project.title.clone();
    ProjectData {
        text_buffer: project.text_buffer,
        scene_map: project.scene_map,
        graph: project.graph,
        intent: project.intent,
        perspectives: project.perspectives,
        manifest: project.manifest,
        file_buffer_manager: project.file_buffer_manager,
        project_root: project.project_root,
        config: project.config,
        revision_brief: Some(crate::concepts::revision_brief::RevisionBrief::new(&title)),
        pending_change_scope: None,
        latest_change_review: None,
    }
}

fn build_review_status_text(snapshot: Option<&ProjectSnapshot>) -> Option<String> {
    let snap = snapshot?;
    if snap.pending_scene_count == 0 {
        return None;
    }

    if let Some(scope) = &snap.pending_change_scope {
        Some(scope.summary())
    } else {
        Some(format!(
            "Review pending for {} {}",
            snap.pending_scene_count,
            if snap.pending_scene_count == 1 {
                "scene"
            } else {
                "scenes"
            }
        ))
    }
}

/// Load project data from a specific path.
fn load_project_from_path(start: &Path) -> anyhow::Result<ProjectLoadResult> {
    let project_root = config::find_project_root(start)
        .ok_or_else(|| anyhow::anyhow!("No .laires/ directory found at {}", start.display()))?;
    // Load .env files so API keys are available in the process environment.
    config::load_env_for_project(&project_root);
    load_project(&project_root)
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
            Ok(load_result) => {
                let data = into_gui_project_data(load_result.project);
                let provider = load_result.provider;
                let skills = load_result.skills;
                let summary = load_result.summary;
                let title = data.config.project.title.clone();
                let mode = infer_session_mode(&data.config, data.manifest.as_ref());
                let mut gs = GuiState::default();
                gs.app_mode = AppMode::Project;
                gs.session_mode = mode;
                gs.project_title = title.clone();
                gs.privacy_label = summary.privacy_label;
                gs.model_name = summary.model_name.clone();
                gs.scene_count = summary.scene_count;
                gs.char_count = summary.char_count;
                gs.word_count = summary.word_count;
                gs.context_window_max = state::context_window_for_model(&summary.model_name);
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
                    agent::agent_loop(agent_domain, rx_from_gui, tx_to_gui, provider, skills, mode)
                        .await;
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
