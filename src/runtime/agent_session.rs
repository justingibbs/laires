use futures::future::BoxFuture;

use crate::concepts::context_budget::{ContextReport, extract_relevant_ids, summarize_history};
use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::file_buffer_manager::FileBufferManager;
use crate::concepts::narrative_graph::NarrativeGraph;
#[cfg(test)]
use crate::concepts::provider::LlmResponse;
use crate::concepts::provider::{
    Message, Provider, ResponseUsage, Role, ToolCall, ToolResult, ToolSchema,
};
use crate::concepts::scene_map::SceneMap;
use crate::concepts::skills::{SkillSetContext, Skills};
use crate::concepts::text_buffer::TextBuffer;
use crate::error::LairesError;
use crate::runtime::story_access::StoryAccess;
use crate::sync::divergence;

const DEFAULT_MAX_TOOL_TURNS: usize = 10;
const DEFAULT_MAX_HISTORY_MESSAGES: usize = 20;
const DEFAULT_HISTORY_SUMMARY_RECENT: usize = 6;

pub struct AgentSession {
    history: Vec<Message>,
    max_tool_turns: usize,
    max_history_messages: usize,
    history_summary_recent: usize,
}

pub struct ChatTurnRequest<'a> {
    pub user_input: &'a str,
    pub system_prompt: &'a str,
    pub graph: &'a NarrativeGraph,
    pub intent: &'a DeclaredIntent,
    pub text_buffer: &'a TextBuffer,
    pub scene_map: &'a SceneMap,
    pub file_buffer_manager: Option<&'a FileBufferManager>,
    pub skills: &'a Skills,
    pub skill_context: SkillSetContext,
}

pub struct PreparedChatTurn {
    pub messages: Vec<Message>,
    pub tool_schemas: Vec<ToolSchema>,
    pub context_report: ContextReport,
    pub system_prompt: String,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    ContextPrepared {
        report: ContextReport,
    },
    Usage {
        usage: ResponseUsage,
        context_estimate: String,
    },
}

pub struct CompletedChatTurn {
    pub final_text: String,
}

#[derive(Debug)]
pub enum ChatTurnError {
    MaxToolTurnsReached,
    EmptyResponse,
    Provider {
        source: LairesError,
        context_estimate: String,
    },
}

impl AgentSession {
    pub fn new() -> Self {
        Self::with_history(Vec::new())
    }

    pub fn with_history(history: Vec<Message>) -> Self {
        Self {
            history,
            max_tool_turns: DEFAULT_MAX_TOOL_TURNS,
            max_history_messages: DEFAULT_MAX_HISTORY_MESSAGES,
            history_summary_recent: DEFAULT_HISTORY_SUMMARY_RECENT,
        }
    }

    pub fn history(&self) -> &[Message] {
        &self.history
    }

    pub fn clear(&mut self) {
        self.history.clear();
    }

    pub fn compact_history(&mut self, keep_recent: usize) -> (usize, usize) {
        let before = self.history.len();
        self.history = summarize_history(&self.history, keep_recent);
        (before, self.history.len())
    }

    pub fn prepare_chat_turn(&self, request: ChatTurnRequest<'_>) -> PreparedChatTurn {
        let relevant = extract_relevant_ids(request.user_input, request.graph);
        let graph_json = if relevant.is_empty() {
            request.graph.serialize_summary()
        } else {
            let refs: Vec<&str> = relevant.iter().map(|s| s.as_str()).collect();
            request.graph.serialize_subgraph(&refs)
        };

        let story = StoryAccess::new(
            request.text_buffer,
            request.scene_map,
            request.file_buffer_manager,
        );
        let pending_count = story.pending_scene_count();
        let staleness_note = if pending_count == 0 {
            String::new()
        } else {
            format!(
                "\n[Note: {} scene(s) have unanalyzed changes. Graph may be stale.]",
                pending_count
            )
        };

        let divergences = divergence::detect_divergences(request.graph, request.intent);
        let divergence_json = if divergences.is_empty() {
            String::new()
        } else {
            serde_json::to_string(&divergences).unwrap_or_default()
        };
        let divergence_note = if divergence_json.is_empty() {
            String::new()
        } else {
            format!("\n\nActive divergences (inferred vs writer-declared):\n{divergence_json}")
        };

        let context_message = format!(
            "Current narrative graph:\n```json\n{graph_json}\n```{staleness_note}{divergence_note}"
        );

        let mut messages = vec![Message {
            role: Role::User,
            content: context_message,
            tool_calls: None,
            tool_results: None,
        }];
        messages.extend(summarize_history(
            &self.history,
            self.history_summary_recent,
        ));
        messages.push(Message {
            role: Role::User,
            content: request.user_input.to_string(),
            tool_calls: None,
            tool_results: None,
        });

        let tool_schemas = request
            .skills
            .tool_schemas_for_context(request.skill_context);
        let context_report = ContextReport::from_chat_request(
            request.system_prompt,
            &graph_json,
            &divergence_json,
            &self.history,
            request.user_input,
            &tool_schemas,
        );

        PreparedChatTurn {
            messages,
            tool_schemas,
            context_report,
            system_prompt: request.system_prompt.to_string(),
        }
    }

    pub async fn run_prepared_turn<ExecState, ExecTools, OnEvent>(
        &mut self,
        provider: &mut Provider,
        skills: &mut Skills,
        exec_state: &mut ExecState,
        user_input: &str,
        prepared: PreparedChatTurn,
        mut exec_tools: ExecTools,
        mut on_event: OnEvent,
    ) -> Result<CompletedChatTurn, ChatTurnError>
    where
        ExecTools: for<'a> FnMut(
            &'a [ToolCall],
            &'a mut Provider,
            &'a mut Skills,
            &'a mut ExecState,
        ) -> BoxFuture<'a, Vec<ToolResult>>,
        OnEvent: FnMut(SessionEvent),
    {
        let PreparedChatTurn {
            mut messages,
            tool_schemas,
            context_report,
            system_prompt,
        } = prepared;
        let context_estimate = context_report.summary();
        on_event(SessionEvent::ContextPrepared {
            report: context_report,
        });

        let mut turn_count = 0usize;
        loop {
            if turn_count >= self.max_tool_turns {
                return Err(ChatTurnError::MaxToolTurnsReached);
            }

            let response = provider
                .complete(&messages, &tool_schemas, Some(&system_prompt))
                .await
                .map_err(|source| ChatTurnError::Provider {
                    source,
                    context_estimate: context_estimate.clone(),
                })?;

            on_event(SessionEvent::Usage {
                usage: response.usage.clone(),
                context_estimate: context_estimate.clone(),
            });

            if response.tool_calls.is_empty() {
                let text = response.content.unwrap_or_default();
                if text.is_empty() {
                    return Err(ChatTurnError::EmptyResponse);
                }

                self.history.push(Message {
                    role: Role::User,
                    content: user_input.to_string(),
                    tool_calls: None,
                    tool_results: None,
                });
                self.history.push(Message {
                    role: Role::Assistant,
                    content: text.clone(),
                    tool_calls: None,
                    tool_results: None,
                });
                self.trim_history();

                return Ok(CompletedChatTurn { final_text: text });
            }

            let tool_results = exec_tools(&response.tool_calls, provider, skills, exec_state).await;

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
    }

    fn trim_history(&mut self) {
        if self.history.len() > self.max_history_messages {
            let drain = self.history.len() - self.max_history_messages;
            self.history.drain(..drain);
        }
    }

    #[cfg(test)]
    async fn run_prepared_turn_with_completion<ExecState, Complete, ExecTools, OnEvent>(
        &mut self,
        exec_state: &mut ExecState,
        user_input: &str,
        prepared: PreparedChatTurn,
        mut complete: Complete,
        mut exec_tools: ExecTools,
        mut on_event: OnEvent,
    ) -> Result<CompletedChatTurn, ChatTurnError>
    where
        Complete: for<'a> FnMut(
            &'a [Message],
            &'a [ToolSchema],
            &'a str,
        ) -> BoxFuture<'a, Result<LlmResponse, LairesError>>,
        ExecTools:
            for<'a> FnMut(&'a [ToolCall], &'a mut ExecState) -> BoxFuture<'a, Vec<ToolResult>>,
        OnEvent: FnMut(SessionEvent),
    {
        let PreparedChatTurn {
            mut messages,
            tool_schemas,
            context_report,
            system_prompt,
        } = prepared;
        let context_estimate = context_report.summary();
        on_event(SessionEvent::ContextPrepared {
            report: context_report,
        });

        let mut turn_count = 0usize;
        loop {
            if turn_count >= self.max_tool_turns {
                return Err(ChatTurnError::MaxToolTurnsReached);
            }

            let response = complete(&messages, &tool_schemas, &system_prompt)
                .await
                .map_err(|source| ChatTurnError::Provider {
                    source,
                    context_estimate: context_estimate.clone(),
                })?;

            on_event(SessionEvent::Usage {
                usage: response.usage.clone(),
                context_estimate: context_estimate.clone(),
            });

            if response.tool_calls.is_empty() {
                let text = response.content.unwrap_or_default();
                if text.is_empty() {
                    return Err(ChatTurnError::EmptyResponse);
                }

                self.history.push(Message {
                    role: Role::User,
                    content: user_input.to_string(),
                    tool_calls: None,
                    tool_results: None,
                });
                self.history.push(Message {
                    role: Role::Assistant,
                    content: text.clone(),
                    tool_calls: None,
                    tool_results: None,
                });
                self.trim_history();

                return Ok(CompletedChatTurn { final_text: text });
            }

            let tool_results = exec_tools(&response.tool_calls, exec_state).await;

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
    }
}

impl Default for AgentSession {
    fn default() -> Self {
        Self::new()
    }
}

pub fn truncate_json(value: &serde_json::Value, max_len: usize) -> String {
    let text = serde_json::to_string(value).unwrap_or_default();
    if text.len() > max_len {
        format!("{}...", &text[..max_len])
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::file_buffer_manager::FileBufferManager;
    use std::collections::VecDeque;

    use crate::concepts::declared_intent::DeclaredIntent;
    use crate::concepts::manifest::{Manifest, ManifestMeta, StoryFile};
    use crate::concepts::narrative_graph::{GraphNode, NarrativeGraph};
    use crate::concepts::provider::{ResponseUsage, Role, ToolCall};
    use crate::concepts::scene_map::{ParseMode, SceneMap};
    use crate::concepts::skills::Skills;
    use crate::concepts::text_buffer::TextBuffer;

    fn make_graph() -> NarrativeGraph {
        let mut graph = NarrativeGraph::new();
        graph.add_node(GraphNode::Character {
            id: "char-marcus".to_string(),
            name: "Marcus".to_string(),
            aliases: vec!["Mark".to_string()],
            description: Some("A soldier".to_string()),
        });
        graph.add_node(GraphNode::Character {
            id: "char-elena".to_string(),
            name: "Elena".to_string(),
            aliases: vec![],
            description: Some("A diplomat".to_string()),
        });
        graph.add_node(GraphNode::Scene {
            id: "scene-opening".to_string(),
            title: Some("The Opening".to_string()),
            summary: "Marcus arrives at the embassy.".to_string(),
            characters_present: vec!["char-marcus".to_string()],
            location: Some("Embassy".to_string()),
            time: Some("Morning".to_string()),
            file_path: "story.md".to_string(),
        });
        graph
    }

    fn make_story() -> (TextBuffer, SceneMap) {
        let text =
            "## Scene 1\n\nMarcus arrives at the embassy.\n\n## Scene 2\n\nElena waits in silence.";
        let text_buffer = TextBuffer::from_str(text, std::path::PathBuf::from("/tmp/story.md"));
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text, "story.md");
        (text_buffer, scene_map)
    }

    fn make_multi_file_story() -> (tempfile::TempDir, TextBuffer, SceneMap, FileBufferManager) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("chapter-1.md"),
            "## One\n\nMarcus arrives at the embassy before dawn.",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("chapter-2.md"),
            "## Two\n\nElena waits in silence by the gate.",
        )
        .unwrap();

        let manifest = Manifest {
            meta: ManifestMeta {
                last_scan: "2026-01-01T00:00:00Z".to_string(),
                classification_model: "test".to_string(),
            },
            story_files: vec![
                StoryFile {
                    path: "chapter-1.md".to_string(),
                    format: "prose".to_string(),
                    order: 1,
                    content_hash: "h1".to_string(),
                    editable: true,
                },
                StoryFile {
                    path: "chapter-2.md".to_string(),
                    format: "prose".to_string(),
                    order: 2,
                    content_hash: "h2".to_string(),
                    editable: true,
                },
            ],
            context_files: vec![],
            excluded: vec![],
        };

        let fbm = FileBufferManager::from_manifest(&manifest, tmp.path()).unwrap();
        let (text_buffer, mut scene_map) = make_story();
        for scene in scene_map.list_scenes().to_vec() {
            scene_map.mark_analyzed(&scene.id);
        }

        (tmp, text_buffer, scene_map, fbm)
    }

    fn make_history(len: usize) -> Vec<Message> {
        (0..len)
            .map(|i| Message {
                role: if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                content: format!("message {i}"),
                tool_calls: None,
                tool_results: None,
            })
            .collect()
    }

    #[test]
    fn prepare_chat_turn_includes_staleness_and_relevant_subgraph() {
        let session = AgentSession::with_history(make_history(8));
        let graph = make_graph();
        let intent = DeclaredIntent::new();
        let (text_buffer, mut scene_map) = make_story();
        let first_scene = scene_map.list_scenes()[0].id.clone();
        scene_map.mark_analyzed(&first_scene);
        let skills = Skills::new();

        let prepared = session.prepare_chat_turn(ChatTurnRequest {
            user_input: "Tell me about Marcus",
            system_prompt: "system",
            graph: &graph,
            intent: &intent,
            text_buffer: &text_buffer,
            scene_map: &scene_map,
            file_buffer_manager: None,
            skills: &skills,
            skill_context: SkillSetContext::Chat,
        });

        assert!(prepared.messages[0].content.contains("Graph may be stale."));
        assert!(prepared.messages[0].content.contains("Marcus"));
        assert!(!prepared.messages[0].content.contains("Elena"));
        assert_eq!(
            prepared.messages.last().unwrap().content,
            "Tell me about Marcus"
        );
        assert_eq!(prepared.system_prompt, "system");
    }

    #[test]
    fn prepare_chat_turn_uses_manifest_pending_count_over_primary_scene_map() {
        let session = AgentSession::new();
        let graph = make_graph();
        let intent = DeclaredIntent::new();
        let (_tmp, text_buffer, scene_map, fbm) = make_multi_file_story();
        let skills = Skills::new();

        let prepared = session.prepare_chat_turn(ChatTurnRequest {
            user_input: "What changed in chapter two?",
            system_prompt: "system",
            graph: &graph,
            intent: &intent,
            text_buffer: &text_buffer,
            scene_map: &scene_map,
            file_buffer_manager: Some(&fbm),
            skills: &skills,
            skill_context: SkillSetContext::Chat,
        });

        assert!(
            prepared.messages[0]
                .content
                .contains("2 scene(s) have unanalyzed changes")
        );
    }

    #[test]
    fn compact_history_returns_before_and_after_counts() {
        let mut session = AgentSession::with_history(make_history(10));

        let (before, after) = session.compact_history(4);

        assert_eq!(before, 10);
        assert_eq!(after, 5);
        assert!(
            session.history()[0]
                .content
                .starts_with("Earlier conversation summary:")
        );
    }

    #[test]
    fn run_prepared_turn_with_completion_executes_tools_and_trims_history() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut session = AgentSession::with_history(make_history(4));
            session.max_history_messages = 5;

            let graph = make_graph();
            let intent = DeclaredIntent::new();
            let (text_buffer, scene_map) = make_story();
            let skills = Skills::new();
            let prepared = session.prepare_chat_turn(ChatTurnRequest {
                user_input: "Audit Marcus",
                system_prompt: "system",
                graph: &graph,
                intent: &intent,
                text_buffer: &text_buffer,
                scene_map: &scene_map,
                file_buffer_manager: None,
                skills: &skills,
                skill_context: SkillSetContext::Chat,
            });

            let mut responses = VecDeque::from([
                Ok(LlmResponse {
                    content: Some("Checking a tool".to_string()),
                    tool_calls: vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "story_stats".to_string(),
                        arguments: serde_json::json!({"scope": "all"}),
                    }],
                    usage: ResponseUsage {
                        prompt_tokens: 120,
                        completion_tokens: 40,
                    },
                }),
                Ok(LlmResponse {
                    content: Some("Marcus is driving the opening scene.".to_string()),
                    tool_calls: Vec::new(),
                    usage: ResponseUsage {
                        prompt_tokens: 140,
                        completion_tokens: 60,
                    },
                }),
            ]);
            let mut tool_invocations = Vec::new();
            let mut saw_tool_result_message = false;
            let mut event_count = 0usize;

            let turn = session
                .run_prepared_turn_with_completion(
                    &mut (),
                    "Audit Marcus",
                    prepared,
                    |messages, _, system_prompt| {
                        if messages.iter().any(|m| {
                            m.tool_results
                                .as_ref()
                                .map(|results| !results.is_empty())
                                .unwrap_or(false)
                        }) {
                            saw_tool_result_message = true;
                        }
                        assert_eq!(system_prompt, "system");
                        let response = responses.pop_front().unwrap();
                        Box::pin(async move { response })
                    },
                    |tool_calls, _| {
                        tool_invocations.extend(tool_calls.iter().map(|tc| tc.name.clone()));
                        let results = tool_calls
                            .iter()
                            .map(|tc| ToolResult {
                                tool_call_id: tc.id.clone(),
                                name: tc.name.clone(),
                                result: serde_json::json!({"ok": true}),
                            })
                            .collect();
                        Box::pin(async move { results })
                    },
                    |_| {
                        event_count += 1;
                    },
                )
                .await
                .unwrap();

            assert_eq!(turn.final_text, "Marcus is driving the opening scene.");
            assert_eq!(tool_invocations, vec!["story_stats".to_string()]);
            assert!(saw_tool_result_message);
            assert_eq!(event_count, 3);
            assert_eq!(session.history().len(), 5);
            assert_eq!(session.history()[3].role, Role::User);
            assert_eq!(session.history()[3].content, "Audit Marcus");
            assert_eq!(session.history()[4].role, Role::Assistant);
        });
    }

    #[test]
    fn run_prepared_turn_with_completion_rejects_empty_final_response() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut session = AgentSession::new();
            let graph = make_graph();
            let intent = DeclaredIntent::new();
            let (text_buffer, scene_map) = make_story();
            let skills = Skills::new();
            let prepared = session.prepare_chat_turn(ChatTurnRequest {
                user_input: "Audit Marcus",
                system_prompt: "system",
                graph: &graph,
                intent: &intent,
                text_buffer: &text_buffer,
                scene_map: &scene_map,
                file_buffer_manager: None,
                skills: &skills,
                skill_context: SkillSetContext::Chat,
            });

            let err = session
                .run_prepared_turn_with_completion(
                    &mut (),
                    "Audit Marcus",
                    prepared,
                    |_, _, _| {
                        Box::pin(async {
                            Ok(LlmResponse {
                                content: Some(String::new()),
                                tool_calls: Vec::new(),
                                usage: ResponseUsage::default(),
                            })
                        })
                    },
                    |_, _| Box::pin(async { Vec::new() }),
                    |_| {},
                )
                .await
                .err()
                .unwrap();

            assert!(matches!(err, ChatTurnError::EmptyResponse));
            assert!(session.history().is_empty());
        });
    }

    #[test]
    fn run_prepared_turn_with_completion_stops_after_max_tool_turns() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut session = AgentSession::new();
            session.max_tool_turns = 2;

            let graph = make_graph();
            let intent = DeclaredIntent::new();
            let (text_buffer, scene_map) = make_story();
            let skills = Skills::new();
            let prepared = session.prepare_chat_turn(ChatTurnRequest {
                user_input: "Keep using tools",
                system_prompt: "system",
                graph: &graph,
                intent: &intent,
                text_buffer: &text_buffer,
                scene_map: &scene_map,
                file_buffer_manager: None,
                skills: &skills,
                skill_context: SkillSetContext::Chat,
            });

            let err = session
                .run_prepared_turn_with_completion(
                    &mut (),
                    "Keep using tools",
                    prepared,
                    |_, _, _| {
                        Box::pin(async {
                            Ok(LlmResponse {
                                content: Some("Still using tools".to_string()),
                                tool_calls: vec![ToolCall {
                                    id: "call-loop".to_string(),
                                    name: "story_stats".to_string(),
                                    arguments: serde_json::json!({}),
                                }],
                                usage: ResponseUsage::default(),
                            })
                        })
                    },
                    |tool_calls, _| {
                        let results = tool_calls
                            .iter()
                            .map(|tc| ToolResult {
                                tool_call_id: tc.id.clone(),
                                name: tc.name.clone(),
                                result: serde_json::json!({"ok": true}),
                            })
                            .collect();
                        Box::pin(async move { results })
                    },
                    |_| {},
                )
                .await
                .err()
                .unwrap();

            assert!(matches!(err, ChatTurnError::MaxToolTurnsReached));
            assert!(session.history().is_empty());
        });
    }
}
