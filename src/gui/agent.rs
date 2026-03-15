use std::sync::Arc;
use tokio::sync::Mutex;

use crate::concepts::analysis::{
    apply_analysis_to_graph, Analysis, AnalysisKind, AnalysisTask, Priority,
};
use crate::concepts::file_buffer_manager::FileBufferManager;
use crate::concepts::manifest::{
    build_classification_prompt, build_manifest_from_classification, discover_files,
    parse_classification_response, Manifest,
};
use crate::concepts::provider::{Message, Provider, Role, ToolCall, ToolResult};
use crate::concepts::skills::{SkillContext, SkillSetContext, Skills};
use crate::config::LAIRES_DIR;
use crate::gui::state::{AgentEvent, GuiRequest};
use crate::gui::ProjectData;
use crate::runtime::agent_session::{
    truncate_json, AgentSession, ChatTurnError, ChatTurnRequest, SessionEvent,
};
use crate::runtime::scene_cache::SceneCache;

pub(super) const SYSTEM_PROMPT: &str = r#"You are Laires, an intelligent narrative analysis agent for fiction writers. You have deep understanding of story structure, character arcs, objectives, conflicts, and pacing.

You are helping a writer analyze and develop their manuscript. You have access to a narrative graph that maps characters, objectives, conflicts, and scenes. You also have tools to search and read the story text, analyze from character perspectives, detect structural issues, and compare how different characters experience the same events.

When answering questions:
- Reference specific scenes, characters, and objectives from the graph
- Cite evidence from the text when making claims
- Be honest about confidence levels
- If the graph is incomplete or stale, mention it
- Offer structural insights, not just surface observations
- Use multiple tools when needed to build a complete picture

You can use tools to search the story, read scenes, query the graph, analyze character arcs, run perspective analysis, detect blind spots, lint for consistency issues, check pacing, write/replace text in the canvas, and scan/analyze the manuscript to populate the narrative graph."#;

struct GuiToolRuntime {
    domain: Arc<Mutex<ProjectData>>,
    events: std::sync::mpsc::Sender<AgentEvent>,
}

pub async fn agent_loop(
    domain: Arc<Mutex<ProjectData>>,
    mut requests: tokio::sync::mpsc::UnboundedReceiver<GuiRequest>,
    events: std::sync::mpsc::Sender<AgentEvent>,
    mut provider: Provider,
    mut skills: Skills,
) {
    let mut session = AgentSession::new();
    let mut tool_runtime = GuiToolRuntime {
        domain: domain.clone(),
        events: events.clone(),
    };

    while let Some(request) = requests.recv().await {
        match request {
            GuiRequest::Scan => {
                let _ = events.send(AgentEvent::Thinking);
                let mut guard = domain.lock().await;
                let d = &mut *guard;
                match run_gui_scan(d, &mut provider, &events).await {
                    Ok(summary) => {
                        let _ = events.send(AgentEvent::StateChanged);
                        let _ = events.send(AgentEvent::Response(summary));
                        let _ = events.send(AgentEvent::Idle);
                    }
                    Err(e) => {
                        let _ = events.send(AgentEvent::Error(format!("Scan error: {e}")));
                        let _ = events.send(AgentEvent::Idle);
                    }
                }
            }
            GuiRequest::UpdateProvider(new_config) => {
                match provider.switch_provider(&new_config) {
                    Ok(()) => {
                        // Update skills permissions based on new provider
                        if !provider.is_local() {
                            for restricted in &new_config.privacy.restricted_when_cloud {
                                skills.set_permission(
                                    restricted,
                                    crate::concepts::skills::Permission::Disabled,
                                );
                            }
                        }
                        // Update domain config
                        {
                            let mut guard = domain.lock().await;
                            guard.config = new_config;
                        }
                        session.clear();
                        let _ = events.send(AgentEvent::Response(format!(
                            "Provider switched to {} ({})",
                            provider.model_name(),
                            if provider.is_local() { "local" } else { "cloud" }
                        )));
                        let _ = events.send(AgentEvent::StateChanged);
                        let _ = events.send(AgentEvent::Idle);
                    }
                    Err(e) => {
                        let _ = events.send(AgentEvent::Error(format!(
                            "Failed to switch provider: {e}"
                        )));
                        let _ = events.send(AgentEvent::Idle);
                    }
                }
            }
            GuiRequest::TestConnection => {
                let _ = events.send(AgentEvent::Thinking);
                let success = provider.test_connection().await;
                let message = if success {
                    format!("Connected to {} successfully", provider.model_name())
                } else {
                    match provider.connection_status() {
                        crate::concepts::provider::ConnectionStatus::Error(e) => {
                            format!("Connection failed: {e}")
                        }
                        _ => "Connection failed".to_string(),
                    }
                };
                let _ = events.send(AgentEvent::ConnectionTestResult(success, message));
                let _ = events.send(AgentEvent::Idle);
            }
            GuiRequest::NewSession => {
                session.clear();
                let _ = events.send(AgentEvent::Response(
                    "Session cleared. Ready for a new conversation.".to_string(),
                ));
                let _ = events.send(AgentEvent::Idle);
            }
            GuiRequest::CompactContext => {
                let (before, after) = session.compact_history(4);
                let _ = events.send(AgentEvent::Response(format!(
                    "Context compacted: {} messages condensed to {}.",
                    before, after
                )));
                let _ = events.send(AgentEvent::Idle);
            }
            GuiRequest::Chat(input) => {
                let _ = events.send(AgentEvent::Thinking);

                let prepared = {
                    let d = domain.lock().await;
                    session.prepare_chat_turn(ChatTurnRequest {
                        user_input: &input,
                        system_prompt: SYSTEM_PROMPT,
                        graph: &d.graph,
                        intent: &d.intent,
                        text_buffer: &d.text_buffer,
                        scene_map: &d.scene_map,
                        file_buffer_manager: d.file_buffer_manager.as_ref(),
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
                            Box::pin(execute_gui_tool_calls(
                                runtime,
                                tool_calls,
                                provider,
                                skills,
                            ))
                        },
                        |event| match event {
                            SessionEvent::ContextPrepared { report } => {
                                eprintln!("[context] {}", report.summary());
                            }
                            SessionEvent::Usage {
                                usage,
                                context_estimate,
                            } => {
                                let _ = events.send(AgentEvent::UsageReport {
                                    prompt_tokens: usage.prompt_tokens,
                                    completion_tokens: usage.completion_tokens,
                                    context_estimate,
                                });
                            }
                        },
                    )
                    .await
                {
                    Ok(turn) => {
                        let _ = events.send(AgentEvent::Response(turn.final_text));
                        let _ = events.send(AgentEvent::Idle);
                    }
                    Err(ChatTurnError::MaxToolTurnsReached) => {
                        let _ = events.send(AgentEvent::Error(
                            "[Max tool turns reached]".to_string(),
                        ));
                        let _ = events.send(AgentEvent::Idle);
                    }
                    Err(ChatTurnError::EmptyResponse) => {
                        let _ = events.send(AgentEvent::Error(
                            "LLM returned an empty response. This may indicate the context is too large for the model, a rate limit was hit, or the API returned an error. Check provider logs."
                                .to_string(),
                        ));
                        let _ = events.send(AgentEvent::Idle);
                    }
                    Err(ChatTurnError::Provider {
                        source,
                        context_estimate,
                    }) => {
                        let err_str = source.to_string();
                        let hint = if err_str.contains("too large")
                            || err_str.contains("context_length")
                            || err_str.contains("maximum context")
                            || err_str.contains("token")
                            || err_str.contains("413")
                            || err_str.contains("400")
                        {
                            format!(" (estimated context: {})", context_estimate)
                        } else {
                            String::new()
                        };
                        let _ = events.send(AgentEvent::Error(format!(
                            "LLM error: {err_str}{hint}"
                        )));
                        let _ = events.send(AgentEvent::Idle);
                    }
                }
            }
        }
    }
}

/// Run a story scan within the GUI context.
/// Uses manifest if available, otherwise auto-classifies discovered files via LLM.
/// Falls back to the single loaded text_buffer/scene_map if discovery fails.
async fn run_gui_scan(
    d: &mut ProjectData,
    provider: &mut Provider,
    events: &std::sync::mpsc::Sender<AgentEvent>,
) -> anyhow::Result<String> {
    let project_root = d.project_root.clone();
    let graph_path = project_root.join(LAIRES_DIR).join("graph.json");

    // Determine which manifest to use
    let manifest = if let Some(ref m) = d.manifest {
        m.clone()
    } else if Manifest::exists(&project_root) {
        let m = Manifest::load(&project_root)?;
        d.manifest = Some(m.clone());
        m
    } else {
        // Auto-classify: discover files and classify via LLM (no interactive confirmation)
        let _ = events.send(AgentEvent::ToolCall {
            name: "scan_story".to_string(),
            args_summary: "Discovering and classifying files...".to_string(),
        });
        let discovered = discover_files(&project_root)?;
        if discovered.is_empty() {
            // Fall back to single text_buffer/scene_map
            return run_scan_single_buffer(d, provider, events, &graph_path).await;
        }

        let prompt = build_classification_prompt(&discovered);
        let class_model = if d.config.classification.model.is_empty() {
            &d.config.llm.model
        } else {
            &d.config.classification.model
        };
        let mut class_provider =
            Provider::from_project_config_with_model(&d.config, class_model)
                .map_err(|e| anyhow::anyhow!("Classification provider error: {e}"))?;

        let messages = vec![Message {
            role: Role::User,
            content: prompt,
            tool_calls: None,
            tool_results: None,
        }];
        let system = "You are a file classification assistant for a fiction writing tool. \
                      Classify files into story, outline, characters, notes, or excluded. \
                      Respond with JSON only.";

        let response = class_provider
            .complete(&messages, &[], Some(system))
            .await
            .map_err(|e| anyhow::anyhow!("Classification LLM error: {e}"))?;

        let raw = response.content.unwrap_or_default();
        let result = parse_classification_response(&raw)?;
        let manifest = build_manifest_from_classification(
            result,
            &discovered,
            &d.config.classification.model,
        );
        manifest.save(&project_root)?;
        d.manifest = Some(manifest.clone());
        manifest
    };

    if manifest.story_files.is_empty() {
        return Ok("No story files found in manifest. Nothing to analyze.".to_string());
    }

    // Build FileBufferManager from manifest
    let mut fbm = FileBufferManager::from_manifest(&manifest, &project_root)?;
    let all_scenes = fbm.list_all_scenes();
    let scenes_to_analyze: Vec<_> = all_scenes.iter().copied().cloned().collect();

    if scenes_to_analyze.is_empty() {
        return Ok("No scenes found in story files. Nothing to analyze.".to_string());
    }

    let _ = events.send(AgentEvent::ToolCall {
        name: "scan_story".to_string(),
        args_summary: format!(
            "Analyzing {} scene(s) across {} file(s)...",
            scenes_to_analyze.len(),
            fbm.story_file_count()
        ),
    });

    let mut analysis = Analysis::new();
    let graph_context = d.graph.serialize_compact();
    let mut scenes_analyzed = 0usize;

    for scene in &scenes_to_analyze {
        let scene_text = fbm
            .get_scene(&scene.id)
            .and_then(|(s, buf)| buf.read(s.byte_range()).ok())
            .unwrap_or_default();

        analysis.enqueue(AnalysisTask {
            kind: AnalysisKind::SceneAnalysis {
                scene_id: scene.id.clone(),
            },
            priority: Priority::Normal,
            created: chrono::Utc::now(),
        });

        let title = scene.title.as_deref().unwrap_or("(untitled)");
        let _ = events.send(AgentEvent::ToolCall {
            name: "scan_story".to_string(),
            args_summary: format!("Analyzing scene \"{}\"...", title),
        });

        match analysis
            .process_next(provider, &scene_text, &graph_context, &scene.content_hash)
            .await
        {
            Ok(Some(result)) => {
                apply_analysis_to_graph(&mut d.graph, &result, &scene.id, &scene.file_path);
                if let Some(entry) = fbm.get_entry_mut(&scene.file_path) {
                    entry.scene_map.mark_analyzed(&scene.id);
                }
                scenes_analyzed += 1;
            }
            Ok(None) => {}
            Err(e) => {
                let _ = events.send(AgentEvent::Error(format!(
                    "Analysis error for scene \"{}\": {e}",
                    scene.id
                )));
            }
        }
    }

    // Persist graph and aggregate scene cache
    d.graph.save(&graph_path)?;
    SceneCache::from_file_buffer_manager(&fbm).save_to_project(&project_root)?;

    // Persist FBM into ProjectData so sidebar can read it
    d.file_buffer_manager = Some(fbm);

    // Reload manifest
    if let Ok(m) = Manifest::load(&project_root) {
        d.manifest = Some(m);
    }

    let char_count = d.graph.get_characters().len();
    let summary = format!(
        "Scan complete. Analyzed {} scene(s), found {} unique character(s), {} total characters/objectives/conflicts in graph.",
        scenes_analyzed,
        char_count,
        d.graph.node_count(),
    );

    Ok(summary)
}

/// Fallback: scan using the single loaded text_buffer/scene_map when no manifest or files found.
async fn run_scan_single_buffer(
    d: &mut ProjectData,
    provider: &mut Provider,
    events: &std::sync::mpsc::Sender<AgentEvent>,
    graph_path: &std::path::Path,
) -> anyhow::Result<String> {
    let scenes = d.scene_map.list_scenes().to_vec();
    if scenes.is_empty() {
        return Ok("No scenes found in the current buffer. Nothing to analyze.".to_string());
    }

    let _ = events.send(AgentEvent::ToolCall {
        name: "scan_story".to_string(),
        args_summary: format!("Analyzing {} scene(s) from buffer...", scenes.len()),
    });

    let mut analysis = Analysis::new();
    let graph_context = d.graph.serialize_compact();
    let mut scenes_analyzed = 0usize;

    for scene in &scenes {
        let scene_text = d
            .text_buffer
            .read(scene.byte_range())
            .unwrap_or_default();

        analysis.enqueue(AnalysisTask {
            kind: AnalysisKind::SceneAnalysis {
                scene_id: scene.id.clone(),
            },
            priority: Priority::Normal,
            created: chrono::Utc::now(),
        });

        let title = scene.title.as_deref().unwrap_or("(untitled)");
        let _ = events.send(AgentEvent::ToolCall {
            name: "scan_story".to_string(),
            args_summary: format!("Analyzing scene \"{}\"...", title),
        });

        match analysis
            .process_next(provider, &scene_text, &graph_context, &scene.content_hash)
            .await
        {
            Ok(Some(result)) => {
                apply_analysis_to_graph(&mut d.graph, &result, &scene.id, &scene.file_path);
                d.scene_map.mark_analyzed(&scene.id);
                scenes_analyzed += 1;
            }
            Ok(None) => {}
            Err(e) => {
                let _ = events.send(AgentEvent::Error(format!(
                    "Analysis error for scene \"{}\": {e}",
                    scene.id
                )));
            }
        }
    }

    d.graph.save(graph_path)?;
    let file_path_str = d.text_buffer.file_path().to_string_lossy().to_string();
    let rel_path = file_path_str
        .strip_prefix(&d.project_root.to_string_lossy().as_ref())
        .unwrap_or(&file_path_str)
        .trim_start_matches('/')
        .to_string();
    SceneCache::from_single_file(rel_path.clone(), &d.scene_map).save_to_project(&d.project_root)?;

    // Create a manifest from the loaded story file so the Files tab is populated
    if d.manifest.is_none() {
        let content_hash = blake3::hash(d.text_buffer.read_all().as_bytes())
            .to_hex()
            .to_string();
        let manifest = Manifest {
            meta: crate::concepts::manifest::ManifestMeta {
                last_scan: chrono::Utc::now().to_rfc3339(),
                classification_model: "auto-detected".to_string(),
            },
            story_files: vec![crate::concepts::manifest::StoryFile {
                path: rel_path,
                format: d.config.project.format.clone(),
                order: 0,
                content_hash,
            }],
            context_files: Vec::new(),
            excluded: Vec::new(),
        };
        let _ = manifest.save(&d.project_root);
        d.manifest = Some(manifest);
    }

    let char_count = d.graph.get_characters().len();
    let summary = format!(
        "Scan complete. Analyzed {} scene(s), found {} unique character(s), {} total nodes in graph.",
        scenes_analyzed,
        char_count,
        d.graph.node_count(),
    );

    Ok(summary)
}

async fn execute_gui_tool_calls(
    runtime: &mut GuiToolRuntime,
    tool_calls: &[ToolCall],
    provider: &mut Provider,
    skills: &mut Skills,
) -> Vec<ToolResult> {
    let mut tool_results = Vec::new();
    let mut guard = runtime.domain.lock().await;
    let d = &mut *guard;

    for tc in tool_calls {
        let args_summary = truncate_json(&tc.arguments, 80);
        let _ = runtime.events.send(AgentEvent::ToolCall {
            name: tc.name.clone(),
            args_summary,
        });

        let result = if tc.name == "scan_story" {
            match run_gui_scan(d, provider, &runtime.events).await {
                Ok(summary) => serde_json::json!({ "result": summary }),
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            }
        } else {
            let mut ctx = SkillContext {
                text_buffer: &mut d.text_buffer,
                scene_map: &mut d.scene_map,
                file_buffer_manager: d.file_buffer_manager.as_mut(),
                graph: &d.graph,
                intent: Some(&mut d.intent),
                perspectives: Some(&mut d.perspectives),
                manifest: d.manifest.as_ref(),
                project_root: Some(&d.project_root),
            };
            skills
                .invoke(&tc.name, &tc.arguments, &mut ctx, Some(provider))
                .await
        };

        let _ = runtime.events.send(AgentEvent::ToolResult {
            name: tc.name.clone(),
            result_summary: truncate_json(&result, 100),
        });

        tool_results.push(ToolResult {
            tool_call_id: tc.id.clone(),
            name: tc.name.clone(),
            result,
        });
    }

    if let Some(fbm) = d.file_buffer_manager.as_mut() {
        if let Err(e) = fbm.save_dirty() {
            let _ = runtime
                .events
                .send(AgentEvent::Error(format!("Failed to save: {e}")));
        }
    }
    if d.text_buffer.is_dirty() {
        if let Err(e) = d.text_buffer.save() {
            let _ = runtime
                .events
                .send(AgentEvent::Error(format!("Failed to save: {e}")));
        }
    }

    let _ = runtime.events.send(AgentEvent::StateChanged);
    tool_results
}
