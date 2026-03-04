# Laires egui Desktop GUI — Implementation Plan

## Context

Laires currently has a ratatui TUI (`laires open`) with split-pane chat + canvas. The goal is to build a native desktop GUI using egui/eframe that compiles to a **single self-contained binary** — no JavaScript, no web tech, no external runtime dependencies. The GUI will be a new `laires gui` command coexisting with the TUI. The ambition is to push egui beyond its typical debug-tool aesthetic into a polished, purpose-built writing tool.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│  Sidebar (collapsible)  │  Chat Pane  │  Right Pane         │
│  - Scene list           │  - History  │  [Canvas|Graph|     │
│  - File browser         │  - Input    │   Lint|Pacing]      │
│                         │  - Tools    │                     │
├─────────────────────────┴─────────────┴─────────────────────┤
│  Status Bar: provider | model | counts | agent status       │
└─────────────────────────────────────────────────────────────┘
```

### Threading Model

- **Main thread**: eframe event loop, renders UI at ~60fps
- **Tokio runtime**: Created before `eframe::run_native()`, hosts the async agent task
- **Channels**: `std::sync::mpsc` for agent→GUI events (polled via `try_recv()` each frame), `tokio::sync::mpsc` for GUI→agent requests
- **Domain state**: `Arc<tokio::sync::Mutex<DomainState>>` shared between GUI and agent, with `try_lock()` in the render loop (same proven pattern as existing TUI at `tui.rs:399`)

### State Architecture

Two separate state structs:

**DomainState** (shared via `Arc<Mutex<>>` between GUI + agent):
- TextBuffer, SceneMap, NarrativeGraph, DeclaredIntent, CharacterPerspective, Canvas, Manifest, ProjectConfig

**GuiState** (GUI-thread only, never shared):
- Layout state: active right tab, sidebar visibility, sidebar width
- Chat state: history, input buffer, scroll position, agent status
- Graph state: selected node ID, graph rebuild flag
- Canvas state: scroll offset, selected scene
- Cached display values: provider label, model name, counts

### Agent Event System

```rust
// Agent → GUI (polled each frame via try_recv)
enum AgentEvent {
    Thinking,
    ToolCall { name, args_summary },
    ToolResult { name, result_summary },
    StreamChunk(String),
    Response(String),
    Error(String),
    Idle,
    StateChanged,
}

// GUI → Agent (sent on user action)
enum GuiRequest {
    Chat(String),
    Cancel,
}
```

## New Dependencies (Cargo.toml)

```toml
# GUI
eframe = { version = "0.33", optional = true, default-features = false, features = ["default_fonts", "glow"] }
egui_extras = { version = "0.33", optional = true }
egui_graphs = { version = "0.25", optional = true }
egui_commonmark = { version = "0.21", optional = true }
rfd = { version = "0.15", optional = true }

[features]
default = ["gui"]
gui = ["dep:eframe", "dep:egui_extras", "dep:egui_graphs", "dep:egui_commonmark", "dep:rfd"]
```

Using `glow` backend (not wgpu) — simpler, faster compile, fewer system deps on macOS.

## New File Structure

```
src/gui/
├── mod.rs              # GuiApp struct, eframe::App impl, run_gui(), top-level layout
├── state.rs            # DomainState, GuiState, AgentEvent/GuiRequest enums, channel types
├── theme.rs            # LairesTheme palette, configure_visuals(), configure_fonts()
├── agent.rs            # Async agent loop (extracted from tui.rs pattern)
├── panels/
│   ├── mod.rs          # Re-exports
│   ├── chat.rs         # Message history, input box, streaming display, tool call rendering
│   ├── canvas.rs       # Story text with line numbers + scene boundary highlighting
│   ├── sidebar.rs      # Scene list tab + file browser tab
│   ├── graph_view.rs   # egui_graphs integration, node coloring, click-to-select
│   ├── status_bar.rs   # Bottom bar: provider, model, counts, agent spinner
│   ├── inspector.rs    # Node/scene detail panel (shown on graph node click)
│   ├── lint_view.rs    # Lint results display
│   └── pacing_view.rs  # Pacing analysis table
└── widgets/
    ├── mod.rs
    ├── chat_message.rs # Styled message bubble (user/assistant/tool/error)
    ├── tool_call.rs    # Collapsible tool call with args + result
    └── scene_item.rs   # Scene list item with status indicator

assets/fonts/           # Embedded font files (Source Serif 4 + JetBrains Mono)
```

## Modifications to Existing Files

| File | Change |
|------|--------|
| `Cargo.toml` | Add gui dependencies and feature flag |
| `src/main.rs` | Add `#[cfg(feature = "gui")] mod gui;` |
| `src/cli/mod.rs` | Add `Gui` command variant, dispatch to `gui::run_gui()` |
| `src/concepts/narrative_graph.rs` | Add `pub fn inner_graph(&self) -> &DiGraph<GraphNode, GraphEdge>` for egui_graphs access |

## Phased Build Order

### Phase A1: Window Shell

Create the eframe window with empty placeholder panes. Verify it compiles and opens.

**What to build:**
1. Add dependencies to `Cargo.toml` with feature flag
2. Create `src/gui/mod.rs` — `GuiApp` struct implementing `eframe::App`, `run_gui()` function
3. Create `src/gui/state.rs` — `GuiState` with defaults, `DomainState` struct, event/request enums
4. Create `src/gui/theme.rs` — `LairesTheme` struct with light theme colors, `configure_visuals()` function
5. Create `src/gui/panels/mod.rs` — placeholder re-exports
6. Modify `src/cli/mod.rs` — add `Gui` command variant
7. Modify `src/main.rs` — add `mod gui`

**Layout in `update()`:**
```
TopBottomPanel::bottom("status_bar")     — fixed 28px height
SidePanel::left("sidebar")              — ~200px, collapsible
CentralPanel                            — remaining space
  └── inner SidePanel::left("chat")     — ~50% width (chat pane)
  └── remaining                         — right pane (canvas/graph)
```

**Milestone:** `cargo run -- gui` opens a themed window with three empty panes and a status bar.

### Phase A2: Project Loading + Static Display

Load project state and render read-only content.

**What to build:**
1. `DomainState::load_from_project(project_root)` — mirrors `tui.rs:105-204` initialization:
   - `ProjectConfig::load()`, `TextBuffer::from_file()`, `SceneMap::new()` + `full_reindex()`
   - `NarrativeGraph::load()`, `DeclaredIntent::load()`, `CharacterPerspective::load()`
   - `Manifest::load()`, `Provider::from_project_config()`, `Skills::new()`
2. `src/gui/panels/sidebar.rs` — scene list from `SceneMap::list_scenes()`, file list from `Manifest`
3. `src/gui/panels/canvas.rs` — story text from `TextBuffer::read_all()` with:
   - Line numbers (dimmed gray)
   - Scene boundary lines highlighted (gold/bold)
   - `ScrollArea::vertical()` for scrolling
4. `src/gui/panels/status_bar.rs` — provider name, model, scene/character/word counts

**Scene list rendering:** Each scene as a clickable item showing title (or "Scene N"), word range, analysis status indicator.

**Canvas rendering:** For performance with large manuscripts, use line-height calculation to only render visible lines within the `ScrollArea` viewport.

**Milestone:** GUI displays real project data — scenes in sidebar, story text in canvas, provider info in status bar. Read-only.

### Phase A3: Chat + Agent Loop

Wire up the async agent task and implement interactive chat.

**What to build:**
1. `src/gui/agent.rs` — async agent loop extracted from `tui.rs:218-393`:
   - Takes `SharedDomain`, channels, `Provider`, `Skills`
   - On `GuiRequest::Chat(input)`: builds context (graph JSON + pending + divergences), calls `provider.complete()`, executes tool loop (max 10 turns), sends events back
   - System prompt and MAX_TOOL_TURNS constants (from `tui.rs:32-46`)
2. `src/gui/panels/chat.rs`:
   - `ScrollArea` with message history, `stick_to_bottom(true)`
   - Multi-line `TextEdit` input box at bottom
   - Enter sends message (when agent is Idle), Shift+Enter for newline
   - Agent status indicator (thinking spinner, tool call name)
3. `src/gui/widgets/chat_message.rs`:
   - User messages: green-tinted, right-aligned or prefixed with ">"
   - Assistant messages: default text, rendered as markdown via `egui_commonmark`
   - Error messages: red tint
4. `src/gui/widgets/tool_call.rs`:
   - `CollapsingHeader` showing tool name, collapsed by default
   - Expands to show args summary + result summary in monospace

**Async wiring in `run_gui()`:**
```rust
let rt = tokio::runtime::Runtime::new()?;
let (gui_tx, agent_rx) = tokio::sync::mpsc::unbounded_channel();
let (agent_tx, gui_rx) = std::sync::mpsc::channel();
rt.spawn(agent::agent_loop(domain.clone(), agent_rx, agent_tx, provider, skills));
eframe::run_native("Laires", options, Box::new(move |cc| {
    Ok(Box::new(GuiApp::new(cc, domain, gui_tx, gui_rx, gui_state)))
}));
```

**In `GuiApp::update()`:** drain `gui_rx` via `try_recv()` at the start of each frame. Call `ctx.request_repaint_after(Duration::from_millis(100))` to keep polling while agent is active.

**Milestone:** Full interactive chat with LLM agent. Tool calls displayed as collapsible entries. Agent status shown in status bar.

### Phase A4: Interactive Graph

Build the graph visualization as a first-class view.

**What to build:**
1. Add to `src/concepts/narrative_graph.rs`:
   ```rust
   pub fn inner_graph(&self) -> &DiGraph<GraphNode, GraphEdge> { &self.graph }
   pub fn index_for_node(&self, id: &str) -> Option<NodeIndex> { self.index_map.get(id).copied() }
   ```
2. `src/gui/panels/graph_view.rs`:
   - Convert `NarrativeGraph` → `egui_graphs::Graph` with display wrappers
   - Node coloring by type: Characters `#7AA2F7`, Objectives `#9ECE6A`, Scenes `#E0AF68`, Conflicts `#F7768E`
   - Force-directed layout (Fruchterman-Reingold via egui_graphs)
   - Enable: zoom/pan, node dragging, node clicking, edge clicking
   - On node click: set `gui_state.selected_node_id`
   - Labels always visible
3. Right pane tab bar: `[Canvas] [Graph]` — selectable tabs at the top of the right pane

**Graph rebuild strategy:** Only rebuild the egui_graphs representation when `AgentEvent::StateChanged` is received (meaning the agent modified the graph). Cache the graph layout between frames.

**If egui_graphs has petgraph version incompatibility** (risk: it may depend on petgraph <0.7): fall back to custom rendering using egui's `Painter` API — draw circles for nodes, lines for edges, implement basic force-directed layout (~100 lines).

**Milestone:** Interactive force-directed graph view. Toggle between Canvas and Graph in right pane. Click nodes to select them.

### Phase A5: Keyboard Shortcuts + Fonts

Polish the interaction model and typography.

**What to build:**
1. Download and embed fonts:
   - Source Serif 4 Regular (SIL OFL) — for prose display
   - JetBrains Mono Regular (Apache 2.0) — for monospace/data
   - Place in `assets/fonts/`, embed via `include_bytes!()`
2. In `src/gui/theme.rs` — `configure_fonts()`:
   - Set Source Serif 4 as the `Proportional` family
   - Set JetBrains Mono as the `Monospace` family
   - Configure text style sizes (Body: 14px, Heading: 20px, Small: 11px)
3. Keyboard shortcuts in `GuiApp::update()`:
   - `Ctrl+G`: Switch right pane to Graph
   - `Ctrl+L`: Switch right pane to Lint
   - `Ctrl+P`: Switch right pane to Pacing
   - `Ctrl+B`: Toggle sidebar visibility
   - `Ctrl+/`: Toggle status bar expansion
   - `Escape`: Return right pane to Canvas

**Milestone:** Polished typography with literary serif font for prose. All keyboard shortcuts functional.

### Phase B: Polish (post-MVP)

These features are built after the core is working:

- **Inspector panel** (`panels/inspector.rs`): Node detail view when clicking a graph node — shows all fields from `GraphNode` enum variants, related edges, connected nodes
- **Lint view** (`panels/lint_view.rs`): Dead scenes, orphan intents, pending analysis warnings — logic from `tui.rs:873-908`
- **Pacing view** (`panels/pacing_view.rs`): Scene table with word count + objective density — logic from `tui.rs:909-941`
- **Character perspective panel**: Show perspective analysis results from `CharacterPerspective`
- **Divergence indicators**: Visual markers in graph view and sidebar when `detect_divergences()` finds mismatches
- **Scene click → canvas scroll**: Clicking a scene in sidebar scrolls canvas to that scene
- **Virtual scrolling**: For manuscripts >10k lines, only render visible line range
- **File explorer sidebar tab**: Show manifest files with classification status
- **Scene search**: Quick search/filter in the sidebar scene list

## Theme Details

### Light Theme — Clean Literary Palette

```
Backgrounds:     #FFFFFF (primary), #F8F9FA (secondary/panels), #F1F3F5 (sidebar), #E9ECEF (inputs)
Text:            #212529 (primary), #868E96 (dimmed), #364FC7 (accent/links)
Prose:           #343A40 (body text), #C92A2A (scene boundaries — deep red)
Line numbers:    #CED4DA (very dim)

Graph nodes:
  Characters:    #364FC7 (indigo)
  Objectives:    #2B8A3E (green)
  Scenes:        #E67700 (amber)
  Conflicts:     #C92A2A (red)

Chat roles:
  User:          #2B8A3E (green)
  Assistant:     #212529 (default)
  Tool calls:    #868E96 (dimmed)
  Errors:        #C92A2A (red)

Accents:         #364FC7 (primary indigo), #7048E8 (purple, secondary)
Borders:         #DEE2E6 (subtle)
```

### Visuals Configuration

- Window rounding: 8px
- Widget rounding: 4px
- Selection highlight: `#364FC7` at 15% opacity
- Panel separators: 1px `#DEE2E6`
- Scrollbar: thin, auto-hide

### Fonts

- **Proportional (prose)**: Source Serif 4 Regular — literary, readable at body size
- **Monospace (data/code)**: JetBrains Mono Regular — clean, good for tool call output
- Both are open-source, embedded via `include_bytes!()` for self-contained binary

## Key Design Decisions

| Decision | Choice | Why |
|----------|--------|-----|
| Render backend | glow | Simpler than wgpu, good macOS compat, faster compile |
| Graph viz | egui_graphs | Built on petgraph (already used), force-directed layout included |
| Chat markdown | egui_commonmark | Agent responses contain markdown |
| Async bridge | std::sync::mpsc (agent→GUI), tokio::sync::mpsc (GUI→agent) | GUI thread can't `.await`; `try_recv()` is non-blocking |
| Domain lock | `tokio::sync::Mutex` + `try_lock()` in GUI | Proven pattern from existing TUI |
| Fonts | Embedded via `include_bytes!()` | Self-contained binary requirement |
| Layout | SidePanel + CentralPanel manual split | Simpler than egui_tiles for v1 |
| CLI integration | New `laires gui` command | Coexists with existing `laires open` TUI |

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| egui_graphs requires petgraph <0.7 | Fall back to custom Painter API drawing (~100 LOC for basic graph) |
| Large manuscript performance | Virtual scrolling — only render visible lines |
| Font file size bloating binary | Use Regular weight only (~200KB each), strip unused glyphs if needed |
| tokio runtime + eframe interaction | Create Runtime before `run_native()`, use `rt.spawn()` not `tokio::spawn()` |

## Verification

1. `cargo build --features gui` compiles cleanly
2. `cargo build --no-default-features` still compiles CLI-only (no gui deps)
3. `cargo test` — all 137 existing tests pass
4. `cargo run -- gui` in a Laires project directory: opens GUI, loads project data, chat works
5. Graph view renders narrative graph with colored, interactive nodes
6. Single binary (`target/release/laires`) runs with no external dependencies
