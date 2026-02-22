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
use crate::concepts::narrative_graph::NarrativeGraph;
use crate::concepts::provider::{Message, Provider, Role, ToolResult};
use crate::concepts::scene_map::{ParseMode, SceneMap};
use crate::concepts::skills::{SkillContext, Skills};
use crate::concepts::text_buffer::TextBuffer;
use crate::config::{self, ProjectConfig, LAIRES_DIR, OVERRIDES_FILE, PERSPECTIVES_CACHE_DIR};
use crate::sync::divergence;

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

const MAX_TOOL_TURNS: usize = 10;

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
    active_pane: Pane,
    agent_status: AgentStatus,
    overlay: Option<OverlayKind>,
    overlay_content: Vec<String>,
    chat: ChatPaneState,
    privacy: String,
    model_name: String,
    should_quit: bool,
    llm_history: Vec<Message>,
}

pub async fn run_tui() -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

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
    scene_map.full_reindex(&full_text);

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

    let mut provider = Provider::from_project_config(&config)?;
    let mut skills = Skills::new();

    // Apply cloud restrictions
    if !provider.is_local() {
        for restricted in &config.privacy.restricted_when_cloud {
            skills.set_permission(restricted, crate::concepts::skills::Permission::Disabled);
        }
    }

    let privacy = if provider.is_local() { "Local" } else { "Cloud" }.to_string();
    let model_name = provider.model_name().to_string();
    let title = config.project.title.clone();
    let scene_count = scene_map.scene_count();
    let char_count = graph.get_characters().len();

    let canvas = Canvas::new(24, 80);

    let app = Arc::new(Mutex::new(App {
        text_buffer,
        scene_map,
        graph,
        intent,
        perspectives,
        canvas,
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
        llm_history: Vec::new(),
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
        while let Some(input) = req_rx.recv().await {
            // 1. Lock app to build context messages, then release
            let (mut messages, tool_schemas) = {
                let a = agent_app.lock().await;
                let graph_json = a.graph.serialize_compact();
                let pending = a.scene_map.get_pending();
                let staleness_note = if pending.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n[Note: {} scene(s) have unanalyzed changes.]",
                        pending.len()
                    )
                };
                let divs = divergence::detect_divergences(&a.graph, &a.intent);
                let div_note = if divs.is_empty() {
                    String::new()
                } else {
                    let div_json = serde_json::to_string(&divs).unwrap_or_default();
                    format!("\n\nActive divergences:\n{div_json}")
                };
                let context_msg = format!(
                    "Current narrative graph:\n```json\n{graph_json}\n```{staleness_note}{div_note}"
                );

                let mut msgs = vec![Message {
                    role: Role::User,
                    content: context_msg,
                    tool_calls: None,
                    tool_results: None,
                }];
                msgs.extend(a.llm_history.clone());
                msgs.push(Message {
                    role: Role::User,
                    content: input.clone(),
                    tool_calls: None,
                    tool_results: None,
                });

                let schemas = skills.tool_schemas();
                (msgs, schemas)
            };
            // Lock released here — UI can render while LLM thinks

            // 2. Multi-turn agent loop
            let mut turn_count = 0;
            loop {
                if turn_count >= MAX_TOOL_TURNS {
                    let mut a = agent_app.lock().await;
                    a.chat.history.push(ChatMessage {
                        role: "system".to_string(),
                        content: "[Max tool turns reached]".to_string(),
                    });
                    a.agent_status = AgentStatus::Idle;
                    break;
                }

                // LLM call — no lock held, UI stays responsive
                match provider
                    .complete(&messages, &tool_schemas, Some(SYSTEM_PROMPT))
                    .await
                {
                    Ok(response) => {
                        if response.tool_calls.is_empty() {
                            // Final text response
                            let text = response.content.unwrap_or_default();
                            let mut a = agent_app.lock().await;
                            a.agent_status = AgentStatus::Idle;
                            a.chat.history.push(ChatMessage {
                                role: "assistant".to_string(),
                                content: text.clone(),
                            });
                            // Update LLM history
                            a.llm_history.push(Message {
                                role: Role::User,
                                content: input.clone(),
                                tool_calls: None,
                                tool_results: None,
                            });
                            a.llm_history.push(Message {
                                role: Role::Assistant,
                                content: text,
                                tool_calls: None,
                                tool_results: None,
                            });
                            let hist_len = a.llm_history.len();
                            if hist_len > 20 {
                                a.llm_history.drain(..hist_len - 20);
                            }
                            break;
                        }

                        // Tool calls — lock held during execution
                        let mut tool_results = Vec::new();
                        {
                            let mut guard = agent_app.lock().await;
                            let a = &mut *guard;
                            for tc in &response.tool_calls {
                                a.agent_status =
                                    AgentStatus::ToolCall(tc.name.clone());

                                let result = {
                                    let mut ctx = SkillContext {
                                        text_buffer: &mut a.text_buffer,
                                        scene_map: &mut a.scene_map,
                                        graph: &a.graph,
                                        intent: Some(&mut a.intent),
                                        perspectives: Some(&mut a.perspectives),
                                        canvas: Some(&mut a.canvas),
                                    };
                                    skills
                                        .invoke(
                                            &tc.name,
                                            &tc.arguments,
                                            &mut ctx,
                                            Some(&mut provider),
                                        )
                                        .await
                                };

                                let truncated = truncate_json(&result, 100);
                                a.chat.history.push(ChatMessage {
                                    role: "tool".to_string(),
                                    content: format!("[{} -> {}]", tc.name, truncated),
                                });

                                tool_results.push(ToolResult {
                                    tool_call_id: tc.id.clone(),
                                    name: tc.name.clone(),
                                    result,
                                });
                            }

                            if a.text_buffer.is_dirty() {
                                if let Err(e) = a.text_buffer.save() {
                                    a.chat.history.push(ChatMessage {
                                        role: "error".to_string(),
                                        content: format!("Failed to save: {e}"),
                                    });
                                }
                            }
                        }
                        // Lock released

                        // Append to conversation for next turn
                        messages.push(Message {
                            role: Role::Assistant,
                            content: response.content.unwrap_or_default(),
                            tool_calls: Some(response.tool_calls),
                            tool_results: None,
                        });
                        messages.push(Message {
                            role: Role::User,
                            content: String::new(),
                            tool_calls: None,
                            tool_results: Some(tool_results),
                        });

                        turn_count += 1;
                    }
                    Err(e) => {
                        let mut a = agent_app.lock().await;
                        a.agent_status = AgentStatus::Idle;
                        a.chat.history.push(ChatMessage {
                            role: "error".to_string(),
                            content: format!("LLM error: {e}"),
                        });
                        break;
                    }
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
                        (_, KeyCode::Esc) => {
                            if a.overlay.is_some() {
                                a.overlay = None;
                                a.overlay_content.clear();
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
                                // Overlay scrolling (future enhancement)
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

    // Main layout: content + status bar
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(size);

    // Draw status bar
    draw_status_bar(f, app, main_chunks[1]);

    // If overlay is active, draw it instead of normal panes
    if let Some(ref overlay) = app.overlay {
        draw_overlay(f, app, overlay, main_chunks[0]);
        return;
    }

    // Split content area into chat (left) and canvas (right)
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[0]);

    draw_chat_pane(f, app, content_chunks[0]);
    draw_canvas_pane(f, app, content_chunks[1]);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let status_text = match &app.agent_status {
        AgentStatus::Idle => "Idle".to_string(),
        AgentStatus::Thinking => "Thinking...".to_string(),
        AgentStatus::ToolCall(name) => format!("Calling: {name}"),
        AgentStatus::Streaming => "Streaming...".to_string(),
    };

    let left = format!(" {} | {}", app.privacy, app.model_name);
    let center = format!(
        "{} scenes | {} chars",
        app.scene_map.scene_count(),
        app.graph.get_characters().len()
    );
    let right = format!("[{}] ", status_text);

    // Calculate padding
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
    let lines: Vec<Line> = full_text
        .lines()
        .enumerate()
        .skip(app.canvas.scroll_offset())
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
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem> = app
        .overlay_content
        .iter()
        .map(|line| ListItem::new(Line::from(line.as_str())))
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn toggle_overlay(app: &mut App, kind: OverlayKind) {
    if app.overlay.as_ref().map(|k| std::mem::discriminant(k)) == Some(std::mem::discriminant(&kind)) {
        app.overlay = None;
        app.overlay_content.clear();
        return;
    }

    let content = match kind {
        OverlayKind::Graph => {
            let summary = app.graph.summary();
            summary.lines().map(String::from).collect()
        }
        OverlayKind::Lint => {
            // Run story_lint inline
            let mut issues = Vec::new();
            let dead = app.graph.find_dead_scenes();
            for sid in &dead {
                issues.push(format!("WARN: Dead scene {sid}"));
            }
            let pending = app.scene_map.get_pending();
            if !pending.is_empty() {
                issues.push(format!("INFO: {} scene(s) pending analysis", pending.len()));
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
    };

    app.overlay = Some(kind);
    app.overlay_content = content;
}

fn truncate_json(value: &serde_json::Value, max_len: usize) -> String {
    let s = serde_json::to_string(value).unwrap_or_default();
    if s.len() > max_len {
        format!("{}...", &s[..max_len])
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_tui_status_bar_renders() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let text = "## Scene 1\n\nSome test content with enough words.";
        let text_buffer = TextBuffer::from_str(text, std::path::PathBuf::from("/tmp/test.md"));
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text);

        let app = App {
            text_buffer,
            scene_map,
            graph: NarrativeGraph::new(),
            intent: DeclaredIntent::new(),
            perspectives: CharacterPerspective::new(),
            canvas: Canvas::new(20, 80),
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
            llm_history: Vec::new(),
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
        scene_map.full_reindex(text);

        let mut app = App {
            text_buffer,
            scene_map,
            graph: NarrativeGraph::new(),
            intent: DeclaredIntent::new(),
            perspectives: CharacterPerspective::new(),
            canvas: Canvas::new(20, 80),
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
            llm_history: Vec::new(),
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
            llm_history: Vec::new(),
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
            llm_history: Vec::new(),
        };

        // Type characters
        app.chat.input_buffer.push('H');
        app.chat.input_buffer.push('i');
        assert_eq!(app.chat.input_buffer, "Hi");

        // Backspace
        app.chat.input_buffer.pop();
        assert_eq!(app.chat.input_buffer, "H");
    }
}
