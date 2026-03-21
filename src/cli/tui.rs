use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use tokio::sync::{mpsc, Mutex};

use crate::concepts::canvas::Canvas;
use crate::concepts::character_perspective::CharacterPerspective;
use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::manifest::Manifest;
use crate::concepts::narrative_graph::NarrativeGraph;
use crate::concepts::provider::{ToolCall, ToolResult};
use crate::concepts::scene_map::SceneMap;
use crate::concepts::skills::{SkillContext, SkillSetContext};
use crate::concepts::text_buffer::TextBuffer;
use crate::config::{self, LAIRES_DIR, OVERRIDES_FILE, PERSPECTIVES_CACHE_DIR};
use crate::runtime::agent_session::{
    truncate_json, AgentSession, ChatTurnError, ChatTurnRequest, SessionEvent,
};
use crate::runtime::project_loader::load_project;
use crate::runtime::story_access::StoryAccess;

const SYSTEM_PROMPT: &str = r#"You are Laires, an intelligent narrative analysis agent for fiction writers. You have deep understanding of story structure, character arcs, objectives, conflicts, and pacing.

You are helping a writer analyze and develop their manuscript. You have access to a narrative graph that maps characters, objectives, conflicts, and scenes. You also have tools to search and read the story text, analyze from character perspectives, detect structural issues, and compare how different characters experience the same events.

When answering questions:
- Reference specific scenes, characters, and objectives from the graph
- Cite evidence from the text when making claims
- Be honest about confidence levels
- If the graph is incomplete or stale, mention it
- Offer structural insights, not just surface observations
- Use multiple tools when needed to build a complete picture

You can use tools to search the story, read scenes, query the graph, analyze character arcs, run perspective analysis, detect blind spots, lint for consistency issues, check pacing, and write/replace text in the canvas."#;

struct TuiToolRuntime {
    app: Arc<Mutex<App>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pane {
    Chat,
    Canvas,
}

#[derive(Debug, Clone)]
enum AgentStatus {
    Idle,
    Thinking,
    ToolCall(String),
    #[allow(dead_code)]
    Streaming,
}

#[derive(Debug, Clone)]
enum OverlayKind {
    Graph,
    Lint,
    Pacing,
    FileExplorer,
}

struct ChatPaneState {
    history: Vec<ChatMessage>,
    input_buffer: String,
    scroll_offset: usize,
}

#[derive(Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

struct App {
    text_buffer: TextBuffer,
    scene_map: SceneMap,
    graph: NarrativeGraph,
    intent: DeclaredIntent,
    perspectives: CharacterPerspective,
    canvas: Canvas,
    manifest: Option<Manifest>,
    file_buffer_manager: Option<crate::concepts::file_buffer_manager::FileBufferManager>,
    project_root: std::path::PathBuf,
    active_pane: Pane,
    agent_status: AgentStatus,
    overlay: Option<OverlayKind>,
    overlay_content: Vec<String>,
    chat: ChatPaneState,
    privacy: String,
    model_name: String,
    should_quit: bool,
    overlay_scroll: usize,
    status_expanded: bool,
}

pub async fn run_tui() -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let load_result = load_project(&project_root)?;
    let text_buffer = load_result.project.text_buffer;
    let scene_map = load_result.project.scene_map;
    let graph = load_result.project.graph;
    let intent = load_result.project.intent;
    let perspectives = load_result.project.perspectives;
    let manifest = load_result.project.manifest;
    let file_buffer_manager = load_result.project.file_buffer_manager;
    let config = load_result.project.config;
    let mut provider = load_result.provider;
    let mut skills = load_result.skills;
    let overrides_path = project_root.join(LAIRES_DIR).join(OVERRIDES_FILE);
    let perspectives_path = project_root
        .join(LAIRES_DIR)
        .join(PERSPECTIVES_CACHE_DIR)
        .join("perspectives.json");

    let privacy = if provider.is_local() { "Local" } else { "Cloud" }.to_string();
    let model_name = provider.model_name().to_string();
    let title = config.project.title.clone();
    let story = StoryAccess::new(&text_buffer, &scene_map, file_buffer_manager.as_ref());
    let scene_count = story.scene_count();
    let char_count = graph.get_characters().len();

    let canvas = Canvas::new(24, 80);

    let app = Arc::new(Mutex::new(App {
        text_buffer,
        scene_map,
        graph,
        intent,
        perspectives,
        canvas,
        manifest,
        file_buffer_manager,
        project_root,
        active_pane: Pane::Chat,
        agent_status: AgentStatus::Idle,
        overlay: None,
        overlay_content: Vec::new(),
        chat: ChatPaneState {
            history: vec![ChatMessage {
                role: "system".to_string(),
                content: format!(
                    "Laires TUI - {} | {} | {} scene(s) | {} character(s)",
                    title, model_name, scene_count, char_count
                ),
            }],
            input_buffer: String::new(),
            scroll_offset: 0,
        },
        privacy,
        model_name,
        should_quit: false,
        overlay_scroll: 0,
        status_expanded: false,
    }));

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Channel: main loop sends user input -> agent task
    let (req_tx, mut req_rx) = mpsc::unbounded_channel::<String>();

    // Spawn async agent task (owns Provider and Skills)
    let agent_app = app.clone();
    let agent_handle = tokio::spawn(async move {
        let mut session = AgentSession::new();
        let mut tool_runtime = TuiToolRuntime {
            app: agent_app.clone(),
        };
        while let Some(input) = req_rx.recv().await {
            let prepared = {
                let a = agent_app.lock().await;
                session.prepare_chat_turn(ChatTurnRequest {
                    user_input: &input,
                    system_prompt: SYSTEM_PROMPT,
                    graph: &a.graph,
                    intent: &a.intent,
                    text_buffer: &a.text_buffer,
                    scene_map: &a.scene_map,
                    file_buffer_manager: a.file_buffer_manager.as_ref(),
                    skills: &skills,
                    skill_context: SkillSetContext::Chat,
                })
            };
            match session
                .run_prepared_turn(
                    &mut provider,
                    &mut skills,
                    &mut tool_runtime,
                    &input,
                    prepared,
                    |tool_calls, provider, skills, runtime| {
                        Box::pin(execute_tui_tool_calls(
                            runtime,
                            tool_calls,
                            provider,
                            skills,
                        ))
                    },
                    |event| match event {
                        SessionEvent::ContextPrepared { .. } => {}
                        SessionEvent::Usage { .. } => {}
                    },
                )
                .await
            {
                Ok(turn) => {
                    let mut a = agent_app.lock().await;
                    a.agent_status = AgentStatus::Idle;
                    a.chat.history.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: turn.final_text,
                    });
                }
                Err(ChatTurnError::MaxToolTurnsReached) => {
                    let mut a = agent_app.lock().await;
                    a.agent_status = AgentStatus::Idle;
                    a.chat.history.push(ChatMessage {
                        role: "system".to_string(),
                        content: "[Max tool turns reached]".to_string(),
                    });
                }
                Err(ChatTurnError::EmptyResponse) => {
                    let mut a = agent_app.lock().await;
                    a.agent_status = AgentStatus::Idle;
                    a.chat.history.push(ChatMessage {
                        role: "error".to_string(),
                        content: "LLM returned an empty response.".to_string(),
                    });
                }
                Err(ChatTurnError::Provider { source, .. }) => {
                    let mut a = agent_app.lock().await;
                    a.agent_status = AgentStatus::Idle;
                    a.chat.history.push(ChatMessage {
                        role: "error".to_string(),
                        content: format!("LLM error: {source}"),
                    });
                }
            }
        }
    });

    // Main event loop — uses try_lock() so it never blocks on the agent
    let mut should_quit = false;
    loop {
        // Render (skip frame if agent holds the lock)
        if let Ok(a) = app.try_lock() {
            terminal.draw(|f| draw_ui(f, &a))?;
        }

        // Poll keyboard events (50ms timeout keeps UI responsive)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if let Ok(mut a) = app.try_lock() {
                    match (key.modifiers, key.code) {
                        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                            a.should_quit = true;
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('g')) => {
                            toggle_overlay(&mut a, OverlayKind::Graph);
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                            toggle_overlay(&mut a, OverlayKind::Lint);
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('p')) => {
                            toggle_overlay(&mut a, OverlayKind::Pacing);
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                            toggle_overlay(&mut a, OverlayKind::FileExplorer);
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('/')) => {
                            a.status_expanded = !a.status_expanded;
                        }
                        (_, KeyCode::Esc) => {
                            if a.overlay.is_some() {
                                a.overlay = None;
                                a.overlay_content.clear();
                                a.overlay_scroll = 0;
                            }
                        }
                        (_, KeyCode::Tab) => {
                            a.active_pane = match a.active_pane {
                                Pane::Chat => Pane::Canvas,
                                Pane::Canvas => Pane::Chat,
                            };
                        }
                        _ => {
                            if a.overlay.is_some() {
                                match key.code {
                                    KeyCode::Up => {
                                        a.overlay_scroll =
                                            a.overlay_scroll.saturating_sub(1);
                                    }
                                    KeyCode::Down => {
                                        let max = a.overlay_content.len().saturating_sub(1);
                                        a.overlay_scroll =
                                            a.overlay_scroll.saturating_add(1).min(max);
                                    }
                                    KeyCode::PageUp => {
                                        a.overlay_scroll =
                                            a.overlay_scroll.saturating_sub(10);
                                    }
                                    KeyCode::PageDown => {
                                        let max = a.overlay_content.len().saturating_sub(1);
                                        a.overlay_scroll =
                                            a.overlay_scroll.saturating_add(10).min(max);
                                    }
                                    _ => {}
                                }
                            } else if a.active_pane == Pane::Chat {
                                match key.code {
                                    KeyCode::Char(c) => {
                                        a.chat.input_buffer.push(c);
                                    }
                                    KeyCode::Backspace => {
                                        a.chat.input_buffer.pop();
                                    }
                                    KeyCode::Enter => {
                                        if !a.chat.input_buffer.trim().is_empty()
                                            && matches!(a.agent_status, AgentStatus::Idle)
                                        {
                                            let input: String =
                                                a.chat.input_buffer.drain(..).collect();
                                            let input = input.trim().to_string();
                                            if input == "quit"
                                                || input == "exit"
                                                || input == "/q"
                                            {
                                                a.should_quit = true;
                                            } else {
                                                a.chat.history.push(ChatMessage {
                                                    role: "user".to_string(),
                                                    content: input.clone(),
                                                });
                                                a.agent_status = AgentStatus::Thinking;
                                                let _ = req_tx.send(input);
                                            }
                                        }
                                    }
                                    KeyCode::Up => {
                                        a.chat.scroll_offset =
                                            a.chat.scroll_offset.saturating_add(1);
                                    }
                                    KeyCode::Down => {
                                        a.chat.scroll_offset =
                                            a.chat.scroll_offset.saturating_sub(1);
                                    }
                                    _ => {}
                                }
                            } else {
                                // Canvas pane navigation
                                match key.code {
                                    KeyCode::Up => a.canvas.scroll(-1),
                                    KeyCode::Down => a.canvas.scroll(1),
                                    KeyCode::PageUp => a.canvas.scroll(-10),
                                    KeyCode::PageDown => a.canvas.scroll(10),
                                    _ => {}
                                }
                            }
                        }
                    }

                    should_quit = a.should_quit;
                }
            }
        }

        if should_quit {
            break;
        }
    }

    // Signal agent task to stop and wait for it
    drop(req_tx);
    let _ = agent_handle.await;

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Persist state
    let a = app.lock().await;
    if let Err(e) = a.intent.save(&overrides_path) {
        eprintln!("Warning: failed to save overrides: {e}");
    }
    if let Err(e) = a.perspectives.save(&perspectives_path) {
        eprintln!("Warning: failed to save perspectives: {e}");
    }

    println!("Goodbye!");
    Ok(())
}

fn draw_ui(f: &mut Frame, app: &App) {
    let size = f.area();

    // Main layout: content + status bar (expanded = 4 lines, minimal = 1)
    let status_height = if app.status_expanded { 4 } else { 1 };
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(status_height)])
        .split(size);

    // Draw status bar
    draw_status_bar(f, app, main_chunks[1]);

    // Split content area into chat (left) and canvas/overlay (right)
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[0]);

    draw_chat_pane(f, app, content_chunks[0]);

    if let Some(ref overlay) = app.overlay {
        draw_overlay(f, app, overlay, content_chunks[1]);
    } else {
        draw_canvas_pane(f, app, content_chunks[1]);
    }
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let status_text = match &app.agent_status {
        AgentStatus::Idle => "Idle".to_string(),
        AgentStatus::Thinking => "Thinking...".to_string(),
        AgentStatus::ToolCall(name) => format!("Calling: {name}"),
        AgentStatus::Streaming => "Streaming...".to_string(),
    };
    let story = StoryAccess::new(&app.text_buffer, &app.scene_map, app.file_buffer_manager.as_ref());

    if app.status_expanded {
        // Expanded: multi-line status
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);

        // Row 0: separator
        let sep = Paragraph::new(Line::from(
            "─".repeat(area.width as usize),
        ))
        .style(Style::default().fg(Color::DarkGray).bg(Color::DarkGray));
        f.render_widget(sep, rows[0]);

        // Row 1: provider + status
        let line1 = Line::from(vec![
            Span::styled(
                format!(" {} | {} ", app.privacy, app.model_name),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("[{}]", status_text),
                Style::default().fg(Color::Yellow),
            ),
        ]);
        let bar1 = Paragraph::new(line1).style(Style::default().bg(Color::DarkGray));
        f.render_widget(bar1, rows[1]);

        // Row 2: graph stats
        let scene_count = story.scene_count();
        let char_count = app.graph.get_characters().len();
        let obj_count = app.graph.get_objectives().len();
        let conflict_count = app.graph.get_conflicts().len();
        let line2 = Line::from(vec![
            Span::styled(
                format!(
                    " Scenes: {} | Characters: {} | Objectives: {} | Conflicts: {}",
                    scene_count, char_count, obj_count, conflict_count
                ),
                Style::default().fg(Color::White),
            ),
        ]);
        let bar2 = Paragraph::new(line2).style(Style::default().bg(Color::DarkGray));
        f.render_widget(bar2, rows[2]);

        // Row 3: word count + pending + files
        let word_count = story.word_count();
        let pending_count = story.pending_scene_count();
        let file_count = app
            .manifest
            .as_ref()
            .map(|m| m.story_files.len())
            .unwrap_or(0);
        let line3 = Line::from(vec![
            Span::styled(
                format!(
                    " Words: {} | Pending: {} | Files: {} | Ctrl+/ to collapse",
                    word_count, pending_count, file_count
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        let bar3 = Paragraph::new(line3).style(Style::default().bg(Color::DarkGray));
        f.render_widget(bar3, rows[3]);
    } else {
        // Minimal: single-line status bar (original)
        let left = format!(" {} | {}", app.privacy, app.model_name);
        let center = format!(
            "{} scenes | {} chars",
            story.scene_count(),
            app.graph.get_characters().len()
        );
        let right = format!("[{}] ", status_text);

        let total = area.width as usize;
        let used = left.len() + center.len() + right.len();
        let pad_left = if total > used {
            ((total - used) / 2).saturating_sub(left.len()).max(1)
        } else {
            1
        };
        let pad_right = total
            .saturating_sub(left.len())
            .saturating_sub(pad_left)
            .saturating_sub(center.len())
            .saturating_sub(right.len())
            .max(1);

        let status_line = Line::from(vec![
            Span::styled(left, Style::default().fg(Color::Cyan)),
            Span::raw(" ".repeat(pad_left.max(1))),
            Span::styled(center, Style::default().fg(Color::White)),
            Span::raw(" ".repeat(pad_right.max(1))),
            Span::styled(right, Style::default().fg(Color::Yellow)),
        ]);

        let bar = Paragraph::new(status_line)
            .style(Style::default().bg(Color::DarkGray));
        f.render_widget(bar, area);
    }
}

fn draw_chat_pane(f: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.active_pane == Pane::Chat {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" Chat ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner into history and input
    let chat_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(inner);

    // History
    let items: Vec<ListItem> = app
        .chat
        .history
        .iter()
        .flat_map(|msg| {
            let (prefix, style) = match msg.role.as_str() {
                "user" => ("> ", Style::default().fg(Color::Green)),
                "assistant" => ("  ", Style::default().fg(Color::White)),
                "tool" => ("  ", Style::default().fg(Color::DarkGray)),
                "error" => ("! ", Style::default().fg(Color::Red)),
                "system" => ("  ", Style::default().fg(Color::Cyan).add_modifier(Modifier::ITALIC)),
                _ => ("  ", Style::default()),
            };

            // Wrap long messages into multiple lines
            let max_width = chat_chunks[0].width.saturating_sub(2) as usize;
            let mut lines = Vec::new();
            for (i, line) in msg.content.lines().enumerate() {
                let display = if i == 0 {
                    format!("{prefix}{line}")
                } else {
                    format!("  {line}")
                };
                // Simple wrapping
                if max_width > 0 && display.len() > max_width {
                    for chunk in display.as_bytes().chunks(max_width) {
                        lines.push(ListItem::new(Line::from(Span::styled(
                            String::from_utf8_lossy(chunk).to_string(),
                            style,
                        ))));
                    }
                } else {
                    lines.push(ListItem::new(Line::from(Span::styled(display, style))));
                }
            }
            lines
        })
        .collect();

    let visible_height = chat_chunks[0].height as usize;
    let total = items.len();
    let offset = if total > visible_height {
        total - visible_height
    } else {
        0
    };

    let visible_items: Vec<ListItem> = items.into_iter().skip(offset).collect();
    let history_list = List::new(visible_items);
    f.render_widget(history_list, chat_chunks[0]);

    // Input bar
    let input_block = Block::default()
        .title(" Input ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let input = Paragraph::new(app.chat.input_buffer.as_str())
        .block(input_block)
        .wrap(Wrap { trim: false });
    f.render_widget(input, chat_chunks[1]);
}

fn draw_canvas_pane(f: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.active_pane == Pane::Canvas {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" Canvas ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Render text content with line numbers
    let full_text = app.text_buffer.read_all();
    let visible_start = app
        .canvas
        .scroll_offset()
        .min(full_text.lines().count().saturating_sub(inner.height as usize));
    let lines: Vec<Line> = full_text
        .lines()
        .enumerate()
        .skip(visible_start)
        .take(inner.height as usize)
        .map(|(i, line_text)| {
            let line_num = format!("{:4} ", i + 1);

            // Check if this is a scene boundary
            let is_boundary = line_text.starts_with("## ")
                || line_text.starts_with("# ")
                || line_text.starts_with("---")
                || line_text.starts_with("INT.")
                || line_text.starts_with("EXT.");

            if is_boundary {
                Line::from(vec![
                    Span::styled(line_num, Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        line_text.to_string(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(line_num, Style::default().fg(Color::DarkGray)),
                    Span::raw(line_text),
                ])
            }
        })
        .collect();

    let canvas_content = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(canvas_content, inner);
}

fn draw_overlay(f: &mut Frame, app: &App, kind: &OverlayKind, area: Rect) {
    let title = match kind {
        OverlayKind::Graph => " Narrative Graph ",
        OverlayKind::Lint => " Lint Results ",
        OverlayKind::Pacing => " Pacing Analysis ",
        OverlayKind::FileExplorer => " File Explorer ",
    };

    let total = app.overlay_content.len();
    let scroll_info = if total > 0 {
        format!(
            " {title}({}/{}) [Esc] ",
            app.overlay_scroll + 1,
            total
        )
    } else {
        format!(" {title}[Esc] ")
    };

    let block = Block::default()
        .title(scroll_info)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible_height = inner.height as usize;
    let items: Vec<ListItem> = app
        .overlay_content
        .iter()
        .skip(app.overlay_scroll)
        .take(visible_height)
        .map(|line| ListItem::new(Line::from(line.as_str())))
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn toggle_overlay(app: &mut App, kind: OverlayKind) {
    if app.overlay.as_ref().map(|k| std::mem::discriminant(k)) == Some(std::mem::discriminant(&kind)) {
        app.overlay = None;
        app.overlay_content.clear();
        app.overlay_scroll = 0;
        return;
    }

    let content = match kind {
        OverlayKind::Graph => {
            let summary = app.graph.summary();
            summary.lines().map(String::from).collect()
        }
        OverlayKind::Lint => {
            let mut issues = Vec::new();
            let dead = app.graph.find_dead_scenes();
            for sid in &dead {
                issues.push(format!("WARN: Dead scene {sid}"));
            }
            let pending_count = StoryAccess::new(
                &app.text_buffer,
                &app.scene_map,
                app.file_buffer_manager.as_ref(),
            )
            .pending_scene_count();
            if pending_count > 0 {
                issues.push(format!("INFO: {} scene(s) pending analysis", pending_count));
            }
            if issues.is_empty() {
                issues.push("No issues found.".to_string());
            }
            issues
        }
        OverlayKind::Pacing => {
            let mut lines = Vec::new();
            lines.push(format!(
                "{:<5} {:<30} {:>8} {:>8}",
                "#", "Title", "Words", "Density"
            ));
            lines.push("-".repeat(55));
            for (i, scene) in app.scene_map.list_scenes().iter().enumerate() {
                let text = app
                    .text_buffer
                    .read(scene.byte_range())
                    .unwrap_or_default();
                let wc = text.split_whitespace().count();
                let analysis = app.graph.get_scene_analysis(&scene.id);
                let density = analysis
                    .map(|a| a.objectives_advanced.len() + a.objectives_blocked.len())
                    .unwrap_or(0);
                let title = scene.title.as_deref().unwrap_or("(untitled)");
                lines.push(format!(
                    "{:<5} {:<30} {:>8} {:>8}",
                    i + 1,
                    if title.len() > 30 {
                        &title[..30]
                    } else {
                        title
                    },
                    wc,
                    density
                ));
            }
            lines
        }
        OverlayKind::FileExplorer => {
            build_file_explorer_content(app)
        }
    };

    app.overlay = Some(kind);
    app.overlay_content = content;
    app.overlay_scroll = 0;
}

fn build_file_explorer_content(app: &App) -> Vec<String> {
    let mut lines = Vec::new();

    match &app.manifest {
        Some(manifest) => {
            // Header
            lines.push(format!(
                "{:<40} {:<12} {:>8} {:>8}",
                "File", "Role", "Words", "Scenes"
            ));
            lines.push("-".repeat(72));

            // Story files
            for sf in &manifest.story_files {
                let (word_count, scene_count) = app
                    .file_buffer_manager
                    .as_ref()
                    .and_then(|fbm| fbm.get_entry(&sf.path))
                    .map(|entry| {
                        (
                            entry.text_buffer.word_count(),
                            entry.scene_map.scene_count(),
                        )
                    })
                    .unwrap_or_else(|| {
                        (
                            app.text_buffer.word_count(),
                            app.scene_map.scene_count(),
                        )
                    });
                let path = if sf.path.len() > 38 {
                    format!("..{}", &sf.path[sf.path.len() - 36..])
                } else {
                    sf.path.clone()
                };
                lines.push(format!(
                    "{:<40} {:<12} {:>8} {:>8}",
                    path,
                    format!("story/{}", sf.format),
                    word_count,
                    scene_count
                ));
            }

            // Context files
            if !manifest.context_files.is_empty() {
                lines.push(String::new());
                lines.push("Context files:".to_string());
                for cf in &manifest.context_files {
                    let path = if cf.path.len() > 38 {
                        format!("..{}", &cf.path[cf.path.len() - 36..])
                    } else {
                        cf.path.clone()
                    };
                    lines.push(format!(
                        "{:<40} {:<12}",
                        path,
                        cf.role.to_string()
                    ));
                }
            }

            // Excluded files
            if !manifest.excluded.is_empty() {
                lines.push(String::new());
                lines.push("Excluded:".to_string());
                for ef in &manifest.excluded {
                    lines.push(format!("  {} -- {}", ef.path, ef.reason));
                }
            }

            // Summary
            lines.push(String::new());
            lines.push(format!(
                "Total: {} story, {} context, {} excluded",
                manifest.story_files.len(),
                manifest.context_files.len(),
                manifest.excluded.len()
            ));
        }
        None => {
            lines.push("No manifest loaded.".to_string());
            lines.push("Run `laires scan` to discover and classify files.".to_string());
        }
    }

    lines
}

async fn execute_tui_tool_calls(
    runtime: &mut TuiToolRuntime,
    tool_calls: &[ToolCall],
    provider: &mut crate::concepts::provider::Provider,
    skills: &mut crate::concepts::skills::Skills,
) -> Vec<ToolResult> {
    let mut tool_results = Vec::new();
    let mut guard = runtime.app.lock().await;
    let a = &mut *guard;

    for tc in tool_calls {
        a.agent_status = AgentStatus::ToolCall(tc.name.clone());

        let result = {
            let mut ctx = SkillContext {
                text_buffer: &mut a.text_buffer,
                scene_map: &mut a.scene_map,
                file_buffer_manager: a.file_buffer_manager.as_mut(),
                graph: &a.graph,
                intent: Some(&mut a.intent),
                perspectives: Some(&mut a.perspectives),
                manifest: a.manifest.as_ref(),
                project_root: Some(&a.project_root),
                revision_brief: None,
            };
            skills
                .invoke(&tc.name, &tc.arguments, &mut ctx, Some(provider))
                .await
        };

        a.chat.history.push(ChatMessage {
            role: "tool".to_string(),
            content: format!("[{} -> {}]", tc.name, truncate_json(&result, 100)),
        });

        tool_results.push(ToolResult {
            tool_call_id: tc.id.clone(),
            name: tc.name.clone(),
            result,
        });
    }

    if let Some(fbm) = a.file_buffer_manager.as_mut() {
        if let Err(e) = fbm.save_dirty() {
            a.chat.history.push(ChatMessage {
                role: "error".to_string(),
                content: format!("Failed to save: {e}"),
            });
        }
    }
    if a.text_buffer.is_dirty() {
        if let Err(e) = a.text_buffer.save() {
            a.chat.history.push(ChatMessage {
                role: "error".to_string(),
                content: format!("Failed to save: {e}"),
            });
        }
    }

    tool_results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::scene_map::ParseMode;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_tui_status_bar_renders() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let text = "## Scene 1\n\nSome test content with enough words.";
        let text_buffer = TextBuffer::from_str(text, std::path::PathBuf::from("/tmp/test.md"));
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text, "");

        let app = App {
            text_buffer,
            scene_map,
            graph: NarrativeGraph::new(),
            intent: DeclaredIntent::new(),
            perspectives: CharacterPerspective::new(),
            canvas: Canvas::new(20, 80),
            manifest: None,
            file_buffer_manager: None,
            project_root: std::path::PathBuf::from("/tmp"),
            active_pane: Pane::Chat,
            agent_status: AgentStatus::Idle,
            overlay: None,
            overlay_content: Vec::new(),
            chat: ChatPaneState {
                history: vec![],
                input_buffer: String::new(),
                scroll_offset: 0,
            },
            privacy: "Local".to_string(),
            model_name: "test-model".to_string(),
            should_quit: false,
            overlay_scroll: 0,
            status_expanded: false,
        };

        terminal
            .draw(|f| draw_ui(f, &app))
            .unwrap();

        // Verify terminal rendered without panic
        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("Local"), "Status bar should show privacy indicator");
        assert!(content.contains("test-model"), "Status bar should show model name");
    }

    #[test]
    fn test_tui_overlay_toggle() {
        let text = "Test content";
        let text_buffer = TextBuffer::from_str(text, std::path::PathBuf::from("/tmp/test.md"));
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text, "");

        let mut app = App {
            text_buffer,
            scene_map,
            graph: NarrativeGraph::new(),
            intent: DeclaredIntent::new(),
            perspectives: CharacterPerspective::new(),
            canvas: Canvas::new(20, 80),
            manifest: None,
            file_buffer_manager: None,
            project_root: std::path::PathBuf::from("/tmp"),
            active_pane: Pane::Chat,
            agent_status: AgentStatus::Idle,
            overlay: None,
            overlay_content: Vec::new(),
            chat: ChatPaneState {
                history: vec![],
                input_buffer: String::new(),
                scroll_offset: 0,
            },
            privacy: "Local".to_string(),
            model_name: "test".to_string(),
            should_quit: false,
            overlay_scroll: 0,
            status_expanded: false,
        };

        // Toggle lint overlay on
        toggle_overlay(&mut app, OverlayKind::Lint);
        assert!(app.overlay.is_some());
        assert!(!app.overlay_content.is_empty());

        // Toggle same overlay off
        toggle_overlay(&mut app, OverlayKind::Lint);
        assert!(app.overlay.is_none());
    }

    #[test]
    fn test_tui_pane_switching() {
        let mut app = App {
            text_buffer: TextBuffer::from_str("test", std::path::PathBuf::from("/tmp/test.md")),
            scene_map: SceneMap::new(ParseMode::Prose),
            graph: NarrativeGraph::new(),
            intent: DeclaredIntent::new(),
            perspectives: CharacterPerspective::new(),
            canvas: Canvas::new(20, 80),
            manifest: None,
            file_buffer_manager: None,
            project_root: std::path::PathBuf::from("/tmp"),
            active_pane: Pane::Chat,
            agent_status: AgentStatus::Idle,
            overlay: None,
            overlay_content: Vec::new(),
            chat: ChatPaneState {
                history: vec![],
                input_buffer: String::new(),
                scroll_offset: 0,
            },
            privacy: "Local".to_string(),
            model_name: "test".to_string(),
            should_quit: false,
            overlay_scroll: 0,
            status_expanded: false,
        };

        assert_eq!(app.active_pane, Pane::Chat);
        app.active_pane = match app.active_pane {
            Pane::Chat => Pane::Canvas,
            Pane::Canvas => Pane::Chat,
        };
        assert_eq!(app.active_pane, Pane::Canvas);
    }

    #[test]
    fn test_tui_chat_input() {
        let mut app = App {
            text_buffer: TextBuffer::from_str("test", std::path::PathBuf::from("/tmp/test.md")),
            scene_map: SceneMap::new(ParseMode::Prose),
            graph: NarrativeGraph::new(),
            intent: DeclaredIntent::new(),
            perspectives: CharacterPerspective::new(),
            canvas: Canvas::new(20, 80),
            manifest: None,
            file_buffer_manager: None,
            project_root: std::path::PathBuf::from("/tmp"),
            active_pane: Pane::Chat,
            agent_status: AgentStatus::Idle,
            overlay: None,
            overlay_content: Vec::new(),
            chat: ChatPaneState {
                history: vec![],
                input_buffer: String::new(),
                scroll_offset: 0,
            },
            privacy: "Local".to_string(),
            model_name: "test".to_string(),
            should_quit: false,
            overlay_scroll: 0,
            status_expanded: false,
        };

        // Type characters
        app.chat.input_buffer.push('H');
        app.chat.input_buffer.push('i');
        assert_eq!(app.chat.input_buffer, "Hi");

        // Backspace
        app.chat.input_buffer.pop();
        assert_eq!(app.chat.input_buffer, "H");
    }

    fn make_test_app() -> App {
        let text = "## Scene 1\n\nSome test content.\n\n---\n\n## Scene 2\n\nMore content here.";
        let text_buffer = TextBuffer::from_str(text, std::path::PathBuf::from("/tmp/test.md"));
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text, "");

        App {
            text_buffer,
            scene_map,
            graph: NarrativeGraph::new(),
            intent: DeclaredIntent::new(),
            perspectives: CharacterPerspective::new(),
            canvas: Canvas::new(20, 80),
            manifest: None,
            file_buffer_manager: None,
            project_root: std::path::PathBuf::from("/tmp"),
            active_pane: Pane::Chat,
            agent_status: AgentStatus::Idle,
            overlay: None,
            overlay_content: Vec::new(),
            chat: ChatPaneState {
                history: vec![],
                input_buffer: String::new(),
                scroll_offset: 0,
            },
            privacy: "Local".to_string(),
            model_name: "test".to_string(),
            should_quit: false,
            overlay_scroll: 0,
            status_expanded: false,
        }
    }

    #[test]
    fn test_overlay_all_modes_toggle() {
        let mut app = make_test_app();

        // Test each overlay mode toggles on and off
        for kind in [
            OverlayKind::Graph,
            OverlayKind::Lint,
            OverlayKind::Pacing,
            OverlayKind::FileExplorer,
        ] {
            toggle_overlay(&mut app, kind.clone());
            assert!(app.overlay.is_some(), "Overlay should be active");
            assert!(!app.overlay_content.is_empty(), "Overlay content should be populated");
            assert_eq!(app.overlay_scroll, 0, "Scroll should reset on toggle");

            // Toggle off
            toggle_overlay(&mut app, kind);
            assert!(app.overlay.is_none(), "Overlay should be dismissed");
            assert!(app.overlay_content.is_empty(), "Content should be cleared");
            assert_eq!(app.overlay_scroll, 0);
        }
    }

    #[test]
    fn test_overlay_switch_between_modes() {
        let mut app = make_test_app();

        // Activate graph overlay
        toggle_overlay(&mut app, OverlayKind::Graph);
        assert!(matches!(app.overlay, Some(OverlayKind::Graph)));

        // Switch to lint without closing graph first
        toggle_overlay(&mut app, OverlayKind::Lint);
        assert!(matches!(app.overlay, Some(OverlayKind::Lint)));

        // Switch to file explorer
        toggle_overlay(&mut app, OverlayKind::FileExplorer);
        assert!(matches!(app.overlay, Some(OverlayKind::FileExplorer)));
    }

    #[test]
    fn test_overlay_scroll() {
        let mut app = make_test_app();

        // Set up overlay with enough content to scroll
        app.overlay = Some(OverlayKind::Lint);
        app.overlay_content = (0..50).map(|i| format!("Line {i}")).collect();
        app.overlay_scroll = 0;

        // Scroll down
        app.overlay_scroll = app.overlay_scroll.saturating_add(1).min(49);
        assert_eq!(app.overlay_scroll, 1);

        // Scroll down by page
        app.overlay_scroll = app.overlay_scroll.saturating_add(10).min(49);
        assert_eq!(app.overlay_scroll, 11);

        // Scroll up
        app.overlay_scroll = app.overlay_scroll.saturating_sub(1);
        assert_eq!(app.overlay_scroll, 10);

        // Scroll up by page
        app.overlay_scroll = app.overlay_scroll.saturating_sub(10);
        assert_eq!(app.overlay_scroll, 0);

        // Can't scroll past beginning
        app.overlay_scroll = app.overlay_scroll.saturating_sub(5);
        assert_eq!(app.overlay_scroll, 0);
    }

    #[test]
    fn test_file_explorer_no_manifest() {
        let mut app = make_test_app();
        toggle_overlay(&mut app, OverlayKind::FileExplorer);

        assert!(app.overlay_content.iter().any(|l| l.contains("No manifest")));
    }

    #[test]
    fn test_file_explorer_with_manifest() {
        let mut app = make_test_app();
        app.manifest = Some(Manifest {
            meta: crate::concepts::manifest::ManifestMeta {
                last_scan: "2026-01-01".to_string(),
                classification_model: "test".to_string(),
            },
            story_files: vec![crate::concepts::manifest::StoryFile {
                path: "chapter-1.md".to_string(),
                format: "prose".to_string(),
                order: 1,
                content_hash: "abc".to_string(),
                editable: true,
            }],
            context_files: vec![crate::concepts::manifest::ContextFile {
                path: "outline.md".to_string(),
                role: crate::concepts::manifest::FileRole::Outline,
                content_hash: "def".to_string(),
            }],
            excluded: vec![crate::concepts::manifest::ExcludedFile {
                path: "old.md".to_string(),
                reason: "Outdated".to_string(),
            }],
        });

        toggle_overlay(&mut app, OverlayKind::FileExplorer);

        let joined = app.overlay_content.join("\n");
        assert!(joined.contains("chapter-1.md"), "Should list story file");
        assert!(joined.contains("outline.md"), "Should list context file");
        assert!(joined.contains("old.md"), "Should list excluded file");
        assert!(joined.contains("Total:"), "Should show summary");
    }

    #[test]
    fn test_status_toggle() {
        let mut app = make_test_app();
        assert!(!app.status_expanded);

        app.status_expanded = !app.status_expanded;
        assert!(app.status_expanded);

        app.status_expanded = !app.status_expanded;
        assert!(!app.status_expanded);
    }

    #[test]
    fn test_overlay_renders_on_right_pane() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = make_test_app();
        toggle_overlay(&mut app, OverlayKind::Graph);

        terminal
            .draw(|f| draw_ui(f, &app))
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        // Chat pane should still be visible
        assert!(content.contains("Chat"), "Chat pane should be visible with overlay");
        // Overlay title should appear
        assert!(content.contains("Narrative Graph"), "Overlay title should be visible");
    }

    #[test]
    fn test_expanded_status_bar_renders() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = make_test_app();
        app.status_expanded = true;

        terminal
            .draw(|f| draw_ui(f, &app))
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("Scenes:"), "Expanded status should show scene count");
        assert!(content.contains("Words:"), "Expanded status should show word count");
    }

    #[test]
    fn test_esc_dismisses_overlay_and_resets_scroll() {
        let mut app = make_test_app();
        toggle_overlay(&mut app, OverlayKind::Pacing);
        app.overlay_scroll = 5;

        // Simulate Esc
        app.overlay = None;
        app.overlay_content.clear();
        app.overlay_scroll = 0;

        assert!(app.overlay.is_none());
        assert!(app.overlay_content.is_empty());
        assert_eq!(app.overlay_scroll, 0);
    }

    #[test]
    fn test_pacing_overlay_content() {
        let mut app = make_test_app();
        toggle_overlay(&mut app, OverlayKind::Pacing);

        // Should have header + separator + 2 scenes = 4 lines
        assert!(app.overlay_content.len() >= 4, "Pacing should have header + scenes");
        assert!(app.overlay_content[0].contains("Title"), "Should have column headers");
        assert!(app.overlay_content[1].contains("-"), "Should have separator line");
    }

    #[test]
    fn test_graph_overlay_content() {
        let mut app = make_test_app();
        toggle_overlay(&mut app, OverlayKind::Graph);

        let joined = app.overlay_content.join("\n");
        assert!(joined.contains("Narrative Graph"), "Should contain graph summary header");
    }
}
