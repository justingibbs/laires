use std::io::{self, BufRead, Write};

use crate::concepts::character_perspective::CharacterPerspective;
use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::narrative_graph::NarrativeGraph;
use crate::concepts::provider::{Message, Provider, Role, ToolResult};
use crate::concepts::scene_map::{ParseMode, SceneMap};
use crate::concepts::skills::{SkillContext, Skills};
use crate::concepts::text_buffer::TextBuffer;
use crate::config::{
    self, ProjectConfig, CHAT_HISTORY_FILE, LAIRES_DIR, OVERRIDES_FILE, PERSPECTIVES_CACHE_DIR,
};
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

You can use tools to search the story, read scenes, query the graph, analyze character arcs, run perspective analysis, detect blind spots, lint for consistency issues, and check pacing."#;

/// Maximum tool-use turns per user message to prevent runaway loops
const MAX_TOOL_TURNS: usize = 10;

pub async fn run(new_session: bool) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let config = ProjectConfig::load(&project_root)?;
    let story_path = config::story_file_path(&project_root, &config.project.format);

    if !story_path.exists() {
        anyhow::bail!("Story file not found: {}", story_path.display());
    }

    // Load all state
    let mut text_buffer = TextBuffer::from_file(story_path)?;
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

    // Load or create DeclaredIntent
    let overrides_path = project_root.join(LAIRES_DIR).join(OVERRIDES_FILE);
    let mut intent = if overrides_path.exists() {
        DeclaredIntent::load(&overrides_path).unwrap_or_default()
    } else {
        DeclaredIntent::new()
    };

    // Initialize CharacterPerspective cache (with disk persistence)
    let perspectives_dir = project_root.join(LAIRES_DIR).join(PERSPECTIVES_CACHE_DIR);
    let perspectives_path = perspectives_dir.join("perspectives.json");
    let graph_json_for_hash = graph.serialize_compact();
    let graph_hash = blake3::hash(graph_json_for_hash.as_bytes()).to_hex().to_string();
    let mut perspectives = if perspectives_path.exists() {
        let mut p = CharacterPerspective::load(&perspectives_path).unwrap_or_default();
        p.invalidate_by_graph_hash(&graph_hash);
        p
    } else {
        CharacterPerspective::new()
    };

    let mut provider = Provider::from_project_config(&config)?;
    let mut skills = Skills::new();

    // Apply cloud skill restrictions (S5 sync)
    if !provider.is_local() {
        for restricted in &config.privacy.restricted_when_cloud {
            skills.set_permission(restricted, crate::concepts::skills::Permission::Disabled);
        }
    }

    // Load or initialize chat history
    let history_path = project_root.join(LAIRES_DIR).join(CHAT_HISTORY_FILE);
    let mut history: Vec<Message> = if !new_session && history_path.exists() {
        match std::fs::read_to_string(&history_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let privacy = if provider.is_local() {
        "local"
    } else {
        "cloud"
    };

    println!("Laires Chat - {} ({})", config.project.title, privacy);
    println!(
        "Model: {} | {} scene(s) | {} characters",
        provider.model_name(),
        scene_map.scene_count(),
        graph.get_characters().len()
    );
    if !history.is_empty() {
        println!("Resumed session ({} messages). Use --new-session to start fresh.", history.len());
    }
    println!("Type your message, or 'quit' to exit.\n");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("> ");
        stdout.flush()?;

        let mut input = String::new();
        stdin.lock().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }
        if input == "quit" || input == "exit" || input == "/q" {
            break;
        }

        // Assemble context (Sync S4.1)
        let graph_json = graph.serialize_compact();
        let pending = scene_map.get_pending();
        let staleness_note = if pending.is_empty() {
            String::new()
        } else {
            format!(
                "\n[Note: {} scene(s) have unanalyzed changes. Graph may be stale.]",
                pending.len()
            )
        };

        // Include divergences in context
        let divs = divergence::detect_divergences(&graph, &intent);
        let div_note = if divs.is_empty() {
            String::new()
        } else {
            let div_json = serde_json::to_string(&divs).unwrap_or_default();
            format!("\n\nActive divergences (inferred vs writer-declared):\n{div_json}")
        };

        let context_msg = format!(
            "Current narrative graph:\n```json\n{graph_json}\n```{staleness_note}{div_note}"
        );

        // Build messages for the LLM
        let mut messages = vec![Message {
            role: Role::User,
            content: context_msg,
            tool_calls: None,
            tool_results: None,
        }];

        // Add history
        messages.extend(history.clone());

        // Add current user message
        messages.push(Message {
            role: Role::User,
            content: input.to_string(),
            tool_calls: None,
            tool_results: None,
        });

        // Get tool schemas
        let tool_schemas = skills.tool_schemas();

        // Multi-turn agent loop
        print!("\n");
        let mut turn_count = 0;
        let mut final_text = None;

        loop {
            if turn_count >= MAX_TOOL_TURNS {
                println!("[Safety: max tool turns reached]");
                break;
            }

            match provider
                .complete(&messages, &tool_schemas, Some(SYSTEM_PROMPT))
                .await
            {
                Ok(response) => {
                    if response.tool_calls.is_empty() {
                        // No tool calls — this is the final response
                        final_text = response.content;
                        break;
                    }

                    // Process tool calls
                    let mut tool_results = Vec::new();
                    for tc in &response.tool_calls {
                        let mut ctx = SkillContext {
                            text_buffer: &mut text_buffer,
                            scene_map: &mut scene_map,
                            graph: &graph,
                            intent: Some(&mut intent),
                            perspectives: Some(&mut perspectives),
                            canvas: None,
                        };

                        let result = skills
                            .invoke(&tc.name, &tc.arguments, &mut ctx, Some(&mut provider))
                            .await;

                        println!(
                            "[tool: {} -> {}]",
                            tc.name,
                            truncate_json(&result, 200)
                        );

                        tool_results.push(ToolResult {
                            tool_call_id: tc.id.clone(),
                            name: tc.name.clone(),
                            result,
                        });
                    }

                    // Save text buffer if modified by tool calls
                    if text_buffer.is_dirty() {
                        if let Err(e) = text_buffer.save() {
                            eprintln!("Warning: failed to save story file: {e}");
                        }
                    }

                    // Add assistant message (with tool calls) to conversation
                    messages.push(Message {
                        role: Role::Assistant,
                        content: response.content.unwrap_or_default(),
                        tool_calls: Some(response.tool_calls),
                        tool_results: None,
                    });

                    // Add tool results as a user message
                    messages.push(Message {
                        role: Role::User,
                        content: String::new(),
                        tool_calls: None,
                        tool_results: Some(tool_results),
                    });

                    turn_count += 1;
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    break;
                }
            }
        }

        // Print final response
        if let Some(text) = &final_text {
            println!("{text}");
        }

        // Update history with user message + final assistant response
        history.push(Message {
            role: Role::User,
            content: input.to_string(),
            tool_calls: None,
            tool_results: None,
        });

        if let Some(text) = final_text {
            history.push(Message {
                role: Role::Assistant,
                content: text,
                tool_calls: None,
                tool_results: None,
            });
        }

        // Keep history manageable (last 20 messages)
        if history.len() > 20 {
            history.drain(..history.len() - 20);
        }

        // Persist any declaration changes (S6.3 sync)
        if let Err(e) = intent.save(&overrides_path) {
            eprintln!("Warning: failed to save overrides: {e}");
        }

        println!();
    }

    // Persist chat history
    if let Ok(json) = serde_json::to_string_pretty(&history) {
        if let Err(e) = std::fs::write(&history_path, json) {
            eprintln!("Warning: failed to save chat history: {e}");
        }
    }

    // Persist perspectives cache
    if let Err(e) = perspectives.save(&perspectives_path) {
        eprintln!("Warning: failed to save perspectives cache: {e}");
    }

    println!("Goodbye!");
    Ok(())
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

    #[test]
    fn test_message_serialization_roundtrip() {
        let messages = vec![
            Message {
                role: Role::User,
                content: "Hello".to_string(),
                tool_calls: None,
                tool_results: None,
            },
            Message {
                role: Role::Assistant,
                content: "Let me check.".to_string(),
                tool_calls: Some(vec![crate::concepts::provider::ToolCall {
                    id: "tc_1".to_string(),
                    name: "read_scene".to_string(),
                    arguments: serde_json::json!({"scene": "1"}),
                }]),
                tool_results: None,
            },
            Message {
                role: Role::User,
                content: String::new(),
                tool_calls: None,
                tool_results: Some(vec![ToolResult {
                    tool_call_id: "tc_1".to_string(),
                    name: "read_scene".to_string(),
                    result: serde_json::json!({"text": "Scene content"}),
                }]),
            },
        ];

        let json = serde_json::to_string_pretty(&messages).unwrap();
        let loaded: Vec<Message> = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].content, "Hello");
        assert!(loaded[1].tool_calls.is_some());
        assert_eq!(loaded[1].tool_calls.as_ref().unwrap()[0].name, "read_scene");
        assert!(loaded[2].tool_results.is_some());
    }
}
