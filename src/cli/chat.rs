use std::io::{self, BufRead, Write};

use crate::concepts::provider::{Message, ToolCall, ToolResult};
use crate::concepts::skills::{SkillContext, SkillSetContext};
use crate::config::{
    self, CHAT_HISTORY_FILE, LAIRES_DIR,
};
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

You can use tools to search the story, read scenes, query the graph, analyze character arcs, run perspective analysis, detect blind spots, lint for consistency issues, and check pacing."#;

struct ChatToolRuntime<'a> {
    text_buffer: &'a mut crate::concepts::text_buffer::TextBuffer,
    scene_map: &'a mut crate::concepts::scene_map::SceneMap,
    graph: &'a crate::concepts::narrative_graph::NarrativeGraph,
    intent: &'a mut crate::concepts::declared_intent::DeclaredIntent,
    perspectives: &'a mut crate::concepts::character_perspective::CharacterPerspective,
    manifest: Option<&'a crate::concepts::manifest::Manifest>,
    file_buffer_manager: Option<&'a mut crate::concepts::file_buffer_manager::FileBufferManager>,
    project_root: &'a std::path::Path,
}

pub async fn run(new_session: bool) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let load_result = load_project(&project_root)?;
    let mut text_buffer = load_result.project.text_buffer;
    let mut scene_map = load_result.project.scene_map;
    let graph = load_result.project.graph;
    let mut intent = load_result.project.intent;
    let mut perspectives = load_result.project.perspectives;
    let manifest = load_result.project.manifest;
    let mut file_buffer_manager = load_result.project.file_buffer_manager;
    let config = load_result.project.config;
    let mut provider = load_result.provider;
    let mut skills = load_result.skills;
    let overrides_path = project_root.join(LAIRES_DIR).join("overrides.json");
    let perspectives_path = project_root
        .join(LAIRES_DIR)
        .join("cache/perspectives")
        .join("perspectives.json");

    // Load or initialize chat history
    let history_path = project_root.join(LAIRES_DIR).join(CHAT_HISTORY_FILE);
    let history: Vec<Message> = if !new_session && history_path.exists() {
        match std::fs::read_to_string(&history_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let mut session = AgentSession::with_history(history);

    let privacy = if provider.is_local() { "local" } else { "cloud" };

    println!("Laires Chat - {} ({})", config.project.title, privacy);
    let story = StoryAccess::new(&text_buffer, &scene_map, file_buffer_manager.as_ref());

    println!(
        "Model: {} | {} scene(s) | {} characters",
        provider.model_name(),
        story.scene_count(),
        graph.get_characters().len()
    );
    if !session.history().is_empty() {
        println!(
            "Resumed session ({} messages). Use --new-session to start fresh.",
            session.history().len()
        );
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

        let prepared = session.prepare_chat_turn(ChatTurnRequest {
            user_input: input,
            system_prompt: SYSTEM_PROMPT,
            graph: &graph,
            intent: &intent,
            text_buffer: &text_buffer,
            scene_map: &scene_map,
            file_buffer_manager: file_buffer_manager.as_ref(),
            skills: &skills,
            skill_context: SkillSetContext::Chat,
        });
        let mut tool_runtime = ChatToolRuntime {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            graph: &graph,
            intent: &mut intent,
            perspectives: &mut perspectives,
            manifest: manifest.as_ref(),
            file_buffer_manager: file_buffer_manager.as_mut(),
            project_root: &project_root,
        };

        print!("\n");
        match session
            .run_prepared_turn(
                &mut provider,
                &mut skills,
                &mut tool_runtime,
                input,
                prepared,
                |tool_calls, provider, skills, runtime| {
                    Box::pin(execute_chat_tool_calls(
                        tool_calls,
                        provider,
                        skills,
                        runtime,
                    ))
                },
                |event| match event {
                    SessionEvent::ContextPrepared { report } => {
                        eprintln!("[context] {}", report.summary());
                    }
                    SessionEvent::Usage {
                        usage,
                        context_estimate: _,
                    } => {
                        if usage.prompt_tokens > 0 || usage.completion_tokens > 0 {
                            eprintln!(
                                "[usage] {}K prompt / {}K completion tokens",
                                usage.prompt_tokens / 1000,
                                usage.completion_tokens / 1000,
                            );
                        }
                    }
                },
            )
            .await
        {
            Ok(turn) => println!("{}", turn.final_text),
            Err(ChatTurnError::MaxToolTurnsReached) => {
                println!("[Safety: max tool turns reached]");
            }
            Err(ChatTurnError::EmptyResponse) => {
                eprintln!(
                    "Error: LLM returned an empty response. This may indicate the context is too large for the model, a rate limit was hit, or the API returned an error."
                );
            }
            Err(ChatTurnError::Provider {
                source,
                context_estimate,
            }) => {
                let err_str = source.to_string();
                eprintln!("Error: {err_str}");
                if err_str.contains("too large")
                    || err_str.contains("context_length")
                    || err_str.contains("maximum context")
                    || err_str.contains("token")
                    || err_str.contains("413")
                    || err_str.contains("400")
                {
                    eprintln!(
                        "Hint: estimated context was {}. The model may have a smaller context window.",
                        context_estimate
                    );
                }
            }
        }

        // Persist any declaration changes (S6.3 sync)
        if let Err(e) = intent.save(&overrides_path) {
            eprintln!("Warning: failed to save overrides: {e}");
        }

        println!();
    }

    // Persist chat history
    if let Ok(json) = serde_json::to_string_pretty(session.history()) {
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

async fn execute_chat_tool_calls(
    tool_calls: &[ToolCall],
    provider: &mut crate::concepts::provider::Provider,
    skills: &mut crate::concepts::skills::Skills,
    runtime: &mut ChatToolRuntime<'_>,
) -> Vec<ToolResult> {
    let mut tool_results = Vec::new();

    for tc in tool_calls {
        let mut ctx = SkillContext {
            text_buffer: runtime.text_buffer,
            scene_map: runtime.scene_map,
            file_buffer_manager: runtime.file_buffer_manager.as_deref_mut(),
            graph: runtime.graph,
            intent: Some(runtime.intent),
            perspectives: Some(runtime.perspectives),
            manifest: runtime.manifest,
            project_root: Some(runtime.project_root),
        };

        let result = skills
            .invoke(&tc.name, &tc.arguments, &mut ctx, Some(provider))
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

    if let Some(fbm) = runtime.file_buffer_manager.as_deref_mut() {
        if let Err(e) = fbm.save_dirty() {
            eprintln!("Warning: failed to save story file(s): {e}");
        }
    }
    if runtime.text_buffer.is_dirty() {
        if let Err(e) = runtime.text_buffer.save() {
            eprintln!("Warning: failed to save story file: {e}");
        }
    }

    tool_results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization_roundtrip() {
        let messages = vec![
            Message {
                role: crate::concepts::provider::Role::User,
                content: "Hello".to_string(),
                tool_calls: None,
                tool_results: None,
            },
            Message {
                role: crate::concepts::provider::Role::Assistant,
                content: "Let me check.".to_string(),
                tool_calls: Some(vec![crate::concepts::provider::ToolCall {
                    id: "tc_1".to_string(),
                    name: "read_scene".to_string(),
                    arguments: serde_json::json!({"scene": "1"}),
                }]),
                tool_results: None,
            },
            Message {
                role: crate::concepts::provider::Role::User,
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
