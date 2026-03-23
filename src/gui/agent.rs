use std::sync::Arc;
use tokio::sync::Mutex;

use crate::concepts::analysis::{
    Analysis, AnalysisKind, AnalysisTask, Priority, apply_analysis_to_graph,
};
use crate::concepts::file_buffer_manager::FileBufferManager;
use crate::concepts::manifest::{
    Manifest, build_classification_prompt, build_manifest_from_classification, discover_files,
    parse_classification_response,
};
use crate::concepts::provider::{Message, Provider, Role, ToolCall, ToolResult};
use crate::concepts::skills::{SkillContext, SkillSetContext, Skills};
use crate::config::LAIRES_DIR;
use crate::gui::ProjectData;
use crate::gui::state::{
    AgentEvent, GuiRequest, SessionMode, StoryChangeReason, StoryChangeReview, StoryChangeScope,
};
use crate::runtime::agent_session::{
    AgentSession, ChatTurnError, ChatTurnRequest, SessionEvent, truncate_json,
};
use crate::runtime::scene_cache::SceneCache;
use crate::runtime::story_access::StoryAccess;
use crate::runtime::story_eval::StoryEvalSnapshot;

const SYSTEM_PROMPT_BASE: &str = r#"You are Laires, an intelligent narrative analysis agent for fiction writers. You have deep understanding of story structure, character arcs, objectives, conflicts, and pacing.

You are helping a writer analyze and develop their manuscript. You have access to a narrative graph that maps characters, objectives, conflicts, and scenes. You also have tools to search and read the story text, analyze from character perspectives, detect structural issues, and compare how different characters experience the same events.

When answering questions:
- Reference specific scenes, characters, and objectives from the graph
- Cite evidence from the text when making claims
- Be honest about confidence levels
- If the graph is incomplete or stale, mention it
- Offer structural insights, not just surface observations
- Use multiple tools when needed to build a complete picture"#;

const SYSTEM_PROMPT_CONSULTANT: &str = r#"

You are in CONSULTANT mode. The writer's files are read-only — do not attempt to modify any files.

When the writer asks you to make changes or improve something:
- Describe exactly what should change, referencing the specific scene, file, and location
- Explain why the change would improve the story
- Optionally provide a draft passage they can adapt in their own editor
- Use the `add_to_brief` tool to record each revision suggestion with scene_title, file, priority, issue, and suggestion
- When the conversation reaches a natural conclusion, use `generate_brief` to finalize and export a Markdown revision brief

The revision brief is the key deliverable in Consultant mode — a structured document the writer takes back to their own editor.

You can search the story, read scenes, query the graph, analyze character arcs, run perspective analysis, detect blind spots, lint for consistency issues, check pacing, and scan/analyze the manuscript."#;

const SYSTEM_PROMPT_WORKSHOP: &str = r#"

You are in WORKSHOP mode. You can directly edit .md and .fountain files using canvas tools.

When the writer asks you to make changes:
- Explain what you plan to change before making edits
- Use write_to_canvas, replace_in_canvas, or insert_scene to modify files
- After significant edits, suggest running scan_story to update the narrative graph

You can search the story, read scenes, query the graph, analyze character arcs, run perspective analysis, detect blind spots, lint for consistency issues, check pacing, write/replace text in the canvas, and scan/analyze the manuscript."#;

pub(super) fn system_prompt_for_mode(mode: SessionMode) -> String {
    let suffix = match mode {
        SessionMode::Consultant => SYSTEM_PROMPT_CONSULTANT,
        SessionMode::Workshop => SYSTEM_PROMPT_WORKSHOP,
    };
    format!("{SYSTEM_PROMPT_BASE}{suffix}")
}

fn skill_context_for_mode(mode: SessionMode) -> SkillSetContext {
    match mode {
        SessionMode::Consultant => SkillSetContext::Consultant,
        SessionMode::Workshop => SkillSetContext::Workshop,
    }
}

struct GuiToolRuntime {
    domain: Arc<Mutex<ProjectData>>,
    events: std::sync::mpsc::Sender<AgentEvent>,
}

fn project_relative_path(project_root: &std::path::Path, file_path: &std::path::Path) -> String {
    file_path
        .strip_prefix(project_root)
        .ok()
        .map(|p| p.to_string_lossy().trim_start_matches('/').to_string())
        .unwrap_or_else(|| file_path.display().to_string())
}

fn record_pending_review_scope(
    d: &mut ProjectData,
    reason: StoryChangeReason,
    changed_files: Vec<String>,
    changed_scene_ids: Vec<String>,
) {
    let scope = StoryChangeScope {
        reason,
        changed_files,
        changed_scene_ids,
    };
    d.pending_change_scope = if scope.is_empty() { None } else { Some(scope) };
}

fn sync_pending_review_scope_from_scene_maps(d: &mut ProjectData, reason: StoryChangeReason) {
    let mut changed_files = Vec::new();
    let mut changed_scene_ids = Vec::new();

    if let Some(ref fbm) = d.file_buffer_manager {
        for entry in fbm.entries() {
            let pending_ids = entry.scene_map.get_pending();
            if pending_ids.is_empty() {
                continue;
            }
            changed_files.push(entry.file_path.clone());
            for scene in entry.scene_map.list_scenes() {
                if pending_ids.contains(&scene.id) {
                    changed_scene_ids.push(scene.id.clone());
                }
            }
        }
    } else {
        let pending_ids = d.scene_map.get_pending();
        if !pending_ids.is_empty() {
            changed_files.push(project_relative_path(
                &d.project_root,
                d.text_buffer.file_path(),
            ));
            for scene in d.scene_map.list_scenes() {
                if pending_ids.contains(&scene.id) {
                    changed_scene_ids.push(scene.id.clone());
                }
            }
        }
    }

    record_pending_review_scope(d, reason, changed_files, changed_scene_ids);
}

fn capture_story_eval_snapshot(d: &ProjectData) -> StoryEvalSnapshot {
    let story = StoryAccess::new(&d.text_buffer, &d.scene_map, d.file_buffer_manager.as_ref());
    let stale_scene_count = d
        .pending_change_scope
        .as_ref()
        .map(|scope| scope.changed_scene_ids.len())
        .unwrap_or(0);
    StoryEvalSnapshot::capture(&d.graph, &d.intent, &story, stale_scene_count)
}

fn changed_range(
    old_text: &str,
    new_text: &str,
) -> Option<crate::concepts::text_buffer::ByteRange> {
    if old_text == new_text {
        return None;
    }

    let old_bytes = old_text.as_bytes();
    let new_bytes = new_text.as_bytes();

    let mut prefix = 0usize;
    let prefix_cap = old_bytes.len().min(new_bytes.len());
    while prefix < prefix_cap && old_bytes[prefix] == new_bytes[prefix] {
        prefix += 1;
    }

    let mut old_suffix = old_bytes.len();
    let mut new_suffix = new_bytes.len();
    while old_suffix > prefix
        && new_suffix > prefix
        && old_bytes[old_suffix - 1] == new_bytes[new_suffix - 1]
    {
        old_suffix -= 1;
        new_suffix -= 1;
    }

    Some(crate::concepts::text_buffer::ByteRange::new(
        prefix, new_suffix,
    ))
}

#[derive(Clone)]
struct SceneAnalysisJob {
    scene_id: String,
    file_path: String,
    title: Option<String>,
    scene_text: String,
    content_hash: String,
}

async fn run_incremental_review(
    d: &mut ProjectData,
    provider: &mut Provider,
    jobs: Vec<SceneAnalysisJob>,
) -> anyhow::Result<usize> {
    if jobs.is_empty() {
        return Ok(0);
    }

    let mut analysis = Analysis::new();
    let mut scenes_analyzed = 0usize;

    for job in jobs {
        analysis.enqueue(AnalysisTask {
            kind: AnalysisKind::SceneAnalysis {
                scene_id: job.scene_id.clone(),
            },
            priority: Priority::High,
            created: chrono::Utc::now(),
        });

        let graph_context = d.graph.serialize_compact();
        match analysis
            .process_next(provider, &job.scene_text, &graph_context, &job.content_hash)
            .await
        {
            Ok(Some(result)) => {
                apply_analysis_to_graph(&mut d.graph, &result, &job.scene_id, &job.file_path);
                if let Some(ref mut fbm) = d.file_buffer_manager {
                    if let Some(entry) = fbm.get_entry_mut(&job.file_path) {
                        entry.scene_map.mark_analyzed(&job.scene_id);
                    }
                } else {
                    d.scene_map.mark_analyzed(&job.scene_id);
                }
                scenes_analyzed += 1;
            }
            Ok(None) => {}
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Analysis error for scene \"{}\": {e}",
                    job.title.as_deref().unwrap_or(&job.scene_id)
                ));
            }
        }
    }

    let graph_path = d.project_root.join(LAIRES_DIR).join("graph.json");
    d.graph.save(&graph_path)?;
    if let Some(ref fbm) = d.file_buffer_manager {
        SceneCache::from_file_buffer_manager(fbm).save_to_project(&d.project_root)?;
    } else {
        let rel_path = project_relative_path(&d.project_root, d.text_buffer.file_path());
        SceneCache::from_single_file(rel_path, &d.scene_map).save_to_project(&d.project_root)?;
    }

    let graph_hash = blake3::hash(d.graph.serialize_compact().as_bytes())
        .to_hex()
        .to_string();
    d.perspectives.invalidate_by_graph_hash(&graph_hash);
    sync_pending_review_scope_from_scene_maps(d, StoryChangeReason::CanvasEdit);

    Ok(scenes_analyzed)
}

pub async fn agent_loop(
    domain: Arc<Mutex<ProjectData>>,
    mut requests: tokio::sync::mpsc::UnboundedReceiver<GuiRequest>,
    events: std::sync::mpsc::Sender<AgentEvent>,
    mut provider: Provider,
    mut skills: Skills,
    initial_mode: SessionMode,
) {
    let mut session = AgentSession::new();
    let mut tool_runtime = GuiToolRuntime {
        domain: domain.clone(),
        events: events.clone(),
    };
    let mut current_mode = initial_mode;

    while let Some(request) = requests.recv().await {
        match request {
            GuiRequest::SwitchMode(new_mode) => {
                current_mode = new_mode;
                let _ = events.send(AgentEvent::Response(format!(
                    "Switched to **{}** mode.",
                    current_mode
                )));
                let _ = events.send(AgentEvent::Idle);
                continue;
            }
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
                            if provider.is_local() {
                                "local"
                            } else {
                                "cloud"
                            }
                        )));
                        let _ = events.send(AgentEvent::StateChanged);
                        let _ = events.send(AgentEvent::Idle);
                    }
                    Err(e) => {
                        let _ = events
                            .send(AgentEvent::Error(format!("Failed to switch provider: {e}")));
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
                // Auto-save brief if there are pending revisions
                {
                    let mut guard = domain.lock().await;
                    if let Some(ref brief) = guard.revision_brief {
                        if !brief.is_empty() {
                            match brief.save_to_project(&guard.project_root) {
                                Ok(path) => {
                                    let _ = events.send(AgentEvent::Response(format!(
                                        "Revision brief saved to `{}`.",
                                        path.display()
                                    )));
                                }
                                Err(e) => {
                                    let _ = events.send(AgentEvent::Error(format!(
                                        "Failed to save brief: {e}"
                                    )));
                                }
                            }
                        }
                    }
                    // Reset brief for the new session
                    let title = guard.config.project.title.clone();
                    guard.revision_brief =
                        Some(crate::concepts::revision_brief::RevisionBrief::new(&title));
                }
                session.clear();
                let _ = events.send(AgentEvent::Response(
                    "Session cleared. Ready for a new conversation.".to_string(),
                ));
                let _ = events.send(AgentEvent::StateChanged);
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
            GuiRequest::CanvasTextChanged { file, text } => {
                let mut guard = domain.lock().await;
                let d = &mut *guard;
                let mut did_change = false;
                let mut review_jobs: Vec<SceneAnalysisJob> = Vec::new();

                let save_result = if let Some(ref file_path) = file {
                    // Multi-file: update in FileBufferManager
                    if let Some(ref mut fbm) = d.file_buffer_manager {
                        if let Some(entry) = fbm.get_entry_mut(file_path) {
                            if entry.text_buffer.read_all() == text {
                                Ok(())
                            } else {
                                did_change = true;
                                let old_text = entry.text_buffer.read_all();
                                let current_len = old_text.len();
                                let range =
                                    crate::concepts::text_buffer::ByteRange::new(0, current_len);
                                if let Err(e) = entry.text_buffer.replace(range, &text) {
                                    Err(format!("{e}"))
                                } else {
                                    let new_text = entry.text_buffer.read_all();
                                    let removed_scene_ids = {
                                        let previous_scene_ids = entry
                                            .scene_map
                                            .list_scenes()
                                            .iter()
                                            .map(|scene| scene.id.clone())
                                            .collect::<std::collections::HashSet<_>>();
                                        let changed_ranges = changed_range(&old_text, &new_text)
                                            .into_iter()
                                            .collect::<Vec<_>>();
                                        entry.scene_map.reindex(
                                            &new_text,
                                            file_path,
                                            &changed_ranges,
                                        );
                                        let current_scene_ids = entry
                                            .scene_map
                                            .list_scenes()
                                            .iter()
                                            .map(|scene| scene.id.clone())
                                            .collect::<std::collections::HashSet<_>>();
                                        previous_scene_ids
                                            .difference(&current_scene_ids)
                                            .cloned()
                                            .collect::<Vec<_>>()
                                    };
                                    for removed_scene_id in removed_scene_ids {
                                        d.graph.clear_scene_analysis(&removed_scene_id, true);
                                    }
                                    review_jobs = entry
                                        .scene_map
                                        .list_scenes()
                                        .iter()
                                        .filter(|scene| {
                                            entry.scene_map.get_pending().contains(&scene.id)
                                        })
                                        .filter_map(|scene| {
                                            entry.text_buffer.read(scene.byte_range()).ok().map(
                                                |scene_text| SceneAnalysisJob {
                                                    scene_id: scene.id.clone(),
                                                    file_path: file_path.clone(),
                                                    title: scene.title.clone(),
                                                    scene_text,
                                                    content_hash: scene.content_hash.clone(),
                                                },
                                            )
                                        })
                                        .collect();
                                    entry.text_buffer.save().map_err(|e| format!("{e}"))
                                }
                            }
                        } else {
                            Err(format!("File '{}' not found in manifest", file_path))
                        }
                    } else {
                        Err("No file buffer manager".to_string())
                    }
                } else {
                    // Single-file: update primary text buffer
                    if d.text_buffer.read_all() == text {
                        Ok(())
                    } else {
                        did_change = true;
                        let old_text = d.text_buffer.read_all();
                        let current_len = old_text.len();
                        let range = crate::concepts::text_buffer::ByteRange::new(0, current_len);
                        if let Err(e) = d.text_buffer.replace(range, &text) {
                            Err(format!("{e}"))
                        } else {
                            let full_text = d.text_buffer.read_all();
                            let rel_path =
                                project_relative_path(&d.project_root, d.text_buffer.file_path());
                            let previous_scene_ids = d
                                .scene_map
                                .list_scenes()
                                .iter()
                                .map(|scene| scene.id.clone())
                                .collect::<std::collections::HashSet<_>>();
                            let changed_ranges = changed_range(&old_text, &full_text)
                                .into_iter()
                                .collect::<Vec<_>>();
                            d.scene_map.reindex(&full_text, &rel_path, &changed_ranges);
                            let current_scene_ids = d
                                .scene_map
                                .list_scenes()
                                .iter()
                                .map(|scene| scene.id.clone())
                                .collect::<std::collections::HashSet<_>>();
                            for removed_scene_id in previous_scene_ids
                                .difference(&current_scene_ids)
                                .cloned()
                                .collect::<Vec<_>>()
                            {
                                d.graph.clear_scene_analysis(&removed_scene_id, true);
                            }
                            review_jobs =
                                d.scene_map
                                    .list_scenes()
                                    .iter()
                                    .filter(|scene| d.scene_map.get_pending().contains(&scene.id))
                                    .filter_map(|scene| {
                                        d.text_buffer.read(scene.byte_range()).ok().map(
                                            |scene_text| SceneAnalysisJob {
                                                scene_id: scene.id.clone(),
                                                file_path: rel_path.clone(),
                                                title: scene.title.clone(),
                                                scene_text,
                                                content_hash: scene.content_hash.clone(),
                                            },
                                        )
                                    })
                                    .collect();
                            d.text_buffer.save().map_err(|e| format!("{e}"))
                        }
                    }
                };

                match save_result {
                    Ok(()) if did_change => {
                        d.latest_change_review = None;
                        sync_pending_review_scope_from_scene_maps(d, StoryChangeReason::CanvasEdit);
                        let review_scope = d.pending_change_scope.clone();
                        let before_eval = capture_story_eval_snapshot(d);
                        let _ = events.send(AgentEvent::StateChanged);
                        if !review_jobs.is_empty() {
                            let _ = events.send(AgentEvent::Thinking);
                            match run_incremental_review(d, &mut provider, review_jobs).await {
                                Ok(_) => {
                                    if let Some(scope) = review_scope {
                                        d.latest_change_review = Some(StoryChangeReview {
                                            scope,
                                            before: before_eval,
                                            after: capture_story_eval_snapshot(d),
                                        });
                                    }
                                    let _ = events.send(AgentEvent::StateChanged);
                                }
                                Err(e) => {
                                    let _ = events.send(AgentEvent::Error(format!(
                                        "Incremental review error: {e}"
                                    )));
                                    let _ = events.send(AgentEvent::StateChanged);
                                }
                            }
                            let _ = events.send(AgentEvent::Idle);
                        }
                    }
                    Ok(()) => {}
                    Err(e) => {
                        let _ = events.send(AgentEvent::Error(format!("Canvas save error: {e}")));
                    }
                }
            }
            GuiRequest::Chat(input) => {
                let _ = events.send(AgentEvent::Thinking);

                let system_prompt = system_prompt_for_mode(current_mode);
                let skill_ctx = skill_context_for_mode(current_mode);
                let prepared = {
                    let d = domain.lock().await;
                    session.prepare_chat_turn(ChatTurnRequest {
                        user_input: &input,
                        system_prompt: &system_prompt,
                        graph: &d.graph,
                        intent: &d.intent,
                        text_buffer: &d.text_buffer,
                        scene_map: &d.scene_map,
                        file_buffer_manager: d.file_buffer_manager.as_ref(),
                        skills: &skills,
                        skill_context: skill_ctx,
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
                                runtime, tool_calls, provider, skills,
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
                        let _ =
                            events.send(AgentEvent::Error("[Max tool turns reached]".to_string()));
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
                        let _ =
                            events.send(AgentEvent::Error(format!("LLM error: {err_str}{hint}")));
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
        let manifest =
            build_manifest_from_classification(result, &discovered, &d.config.classification.model);
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
    d.pending_change_scope = None;
    d.latest_change_review = None;

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
        let scene_text = d.text_buffer.read(scene.byte_range()).unwrap_or_default();

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
    SceneCache::from_single_file(rel_path.clone(), &d.scene_map)
        .save_to_project(&d.project_root)?;
    d.pending_change_scope = None;
    d.latest_change_review = None;

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
                editable: true,
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
                revision_brief: d.revision_brief.as_mut(),
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

#[cfg(test)]
mod tests {
    use super::changed_range;

    #[test]
    fn changed_range_detects_middle_replacement() {
        let range = changed_range("abcXYZdef", "abc123def").unwrap();
        assert_eq!(range.start, 3);
        assert_eq!(range.end, 6);
    }

    #[test]
    fn changed_range_detects_insertion() {
        let range = changed_range("abcdef", "abcZZdef").unwrap();
        assert_eq!(range.start, 3);
        assert_eq!(range.end, 5);
    }

    #[test]
    fn changed_range_none_for_identical_text() {
        assert!(changed_range("same", "same").is_none());
    }
}
