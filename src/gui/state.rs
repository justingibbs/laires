use std::sync::Arc;
use tokio::sync::Mutex;

use crate::concepts::canvas::Canvas;
use crate::concepts::character_perspective::CharacterPerspective;
use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::manifest::Manifest;
use crate::concepts::narrative_graph::NarrativeGraph;
use crate::concepts::scene_map::SceneMap;
use crate::concepts::text_buffer::TextBuffer;
use crate::config::ProjectConfig;

/// Domain state shared between GUI and agent task.
pub struct DomainState {
    pub text_buffer: TextBuffer,
    pub scene_map: SceneMap,
    pub graph: NarrativeGraph,
    pub intent: DeclaredIntent,
    pub perspectives: CharacterPerspective,
    pub canvas: Canvas,
    pub manifest: Option<Manifest>,
    pub project_root: std::path::PathBuf,
    pub config: ProjectConfig,
}

pub type SharedDomain = Arc<Mutex<DomainState>>;

/// Messages from agent -> GUI
pub enum AgentEvent {
    Thinking,
    ToolCall { name: String, args_summary: String },
    ToolResult { name: String, result_summary: String },
    StreamChunk(String),
    Response(String),
    Error(String),
    Idle,
    StateChanged,
    ConnectionTestResult(bool, String),
}

/// Messages from GUI -> agent
pub enum GuiRequest {
    Chat(String),
    Scan,
    UpdateProvider(ProjectConfig),
    TestConnection,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Canvas,
    Graph,
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
    Streaming,
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
            project_title: String::new(),
            active_right_tab: RightTab::Canvas,
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
            sidebar_tab: SidebarTab::Scenes,
            scan_requested: false,
            privacy_label: String::new(),
            model_name: String::new(),
            scene_count: 0,
            char_count: 0,
            word_count: 0,
        }
    }
}
