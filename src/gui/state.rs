use crate::config::ProjectConfig;

/// The two operating modes for a Laires session.
///
/// - **Consultant**: Read-only analysis. The agent never modifies files.
///   Canvas write tools are disabled. Output is structured as a revision brief.
/// - **Workshop**: Live editing of `.md` and `.fountain` files via canvas tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionMode {
    Consultant,
    Workshop,
}

impl std::fmt::Display for SessionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionMode::Consultant => write!(f, "Consultant"),
            SessionMode::Workshop => write!(f, "Workshop"),
        }
    }
}

/// Messages from agent -> GUI
pub enum AgentEvent {
    Thinking,
    ToolCall { name: String, args_summary: String },
    ToolResult { name: String, result_summary: String },
    Response(String),
    Error(String),
    Idle,
    StateChanged,
    ConnectionTestResult(bool, String),
    UsageReport {
        prompt_tokens: u64,
        completion_tokens: u64,
        context_estimate: String,
    },
}

/// Messages from GUI -> agent
pub enum GuiRequest {
    Chat(String),
    Scan,
    UpdateProvider(ProjectConfig),
    TestConnection,
    NewSession,
    CompactContext,
    SwitchMode(SessionMode),
    /// Direct canvas edit: the user typed in the canvas (Workshop mode).
    CanvasTextChanged {
        file: Option<String>,
        text: String,
    },
}

/// Which screen the GUI is displaying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Welcome,
    Project,
}

/// GUI-only state (not shared with agent)
pub struct GuiState {
    // App mode
    pub app_mode: AppMode,
    pub session_mode: SessionMode,
    pub project_title: String,

    // Layout
    pub active_right_tab: RightTab,
    pub sidebar_visible: bool,

    // Chat
    pub chat_history: Vec<ChatMessage>,
    pub chat_input: String,
    pub agent_status: AgentStatus,

    // Graph
    pub selected_node_id: Option<String>,
    pub graph_needs_rebuild: bool,

    // Canvas
    pub selected_scene_id: Option<String>,
    pub selected_file: Option<String>,
    /// Live editable text for the canvas (Workshop mode).
    pub canvas_edit_text: String,
    /// Which file the canvas_edit_text belongs to (None = primary buffer).
    pub canvas_edit_file: Option<String>,
    /// Whether the canvas edit text has been modified since last sync.
    pub canvas_dirty: bool,

    // Analysis sidebar — entity highlight toggles
    pub highlight_characters: bool,
    pub highlight_locations: bool,
    pub highlight_objects: bool,
    pub analysis_sidebar_visible: bool,

    // Sidebar
    pub sidebar_tab: SidebarTab,

    // Scan
    pub scan_requested: bool,

    // Cached display values
    pub privacy_label: String,
    pub model_name: String,
    pub scene_count: usize,
    pub char_count: usize,
    pub word_count: usize,

    // Token usage from last LLM request
    pub last_usage: Option<String>,

    // Context window tracking
    pub context_window_percent: f32,
    pub context_window_max: u64,

    // Session management
    pub new_session_requested: bool,
    pub compact_context_requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Dashboard,
    Canvas,
    Graph,
    Brief,
    Lint,
    Pacing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Scenes,
    Files,
}

#[derive(Debug, Clone)]
pub enum AgentStatus {
    Idle,
    Thinking,
    ToolCall(String),
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub tool_calls: Vec<ToolCallDisplay>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Error,
}

#[derive(Debug, Clone)]
pub struct ToolCallDisplay {
    pub name: String,
    pub args_summary: String,
    pub result_summary: String,
}

impl Default for GuiState {
    fn default() -> Self {
        Self {
            app_mode: AppMode::Welcome,
            session_mode: SessionMode::Consultant,
            project_title: String::new(),
            active_right_tab: RightTab::Dashboard,
            sidebar_visible: true,
            chat_history: vec![ChatMessage {
                role: ChatRole::System,
                content: "Welcome to Laires. Ask me anything about your story.".into(),
                tool_calls: Vec::new(),
            }],
            chat_input: String::new(),
            agent_status: AgentStatus::Idle,
            selected_node_id: None,
            graph_needs_rebuild: true,
            selected_scene_id: None,
            selected_file: None,
            canvas_edit_text: String::new(),
            canvas_edit_file: None,
            canvas_dirty: false,
            highlight_characters: true,
            highlight_locations: true,
            highlight_objects: false,
            analysis_sidebar_visible: true,
            sidebar_tab: SidebarTab::Scenes,
            scan_requested: false,
            privacy_label: String::new(),
            model_name: String::new(),
            scene_count: 0,
            char_count: 0,
            word_count: 0,
            last_usage: None,
            context_window_percent: 0.0,
            context_window_max: 128_000,
            new_session_requested: false,
            compact_context_requested: false,
        }
    }
}

/// Estimate the context window size (in tokens) for known model families.
pub fn context_window_for_model(model: &str) -> u64 {
    let m = model.to_lowercase();
    if m.contains("gemini-2") {
        1_048_576
    } else if m.contains("gemini-1.5-pro") {
        2_097_152
    } else if m.contains("gemini-1.5") {
        1_048_576
    } else if m.contains("claude") {
        200_000
    } else if m.contains("gpt-4o") || m.contains("gpt-4-turbo") || m.contains("o1") || m.contains("o3") {
        128_000
    } else if m.contains("gpt-3.5") {
        16_385
    } else if m.contains("llama") {
        128_000
    } else if m.contains("mistral") {
        32_768
    } else {
        128_000
    }
}
