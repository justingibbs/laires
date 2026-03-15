mod custom;
mod canvas_tools;
mod file_tools;
mod graph_tools;
mod permissions;
mod perspective_tools;
mod registry;
mod structural_tools;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use std::path::Path;

use crate::concepts::character_perspective::CharacterPerspective;
use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::file_buffer_manager::FileBufferManager;
use crate::concepts::manifest::Manifest;
use crate::concepts::narrative_graph::{GraphNode, NarrativeGraph};
use crate::concepts::provider::Provider;
use crate::concepts::scene_map::SceneMap;
use crate::concepts::text_buffer::TextBuffer;
use crate::runtime::story_access::StoryAccess;

#[cfg(test)]
use self::custom::CustomSkillToml;
#[cfg(test)]
use self::custom::toml_to_json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub category: SkillCategory,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillCategory {
    FileTools,
    GraphTools,
    PerspectiveTools,
    StructuralTools,
    CanvasTools,
    CustomTools,
}

/// Context for selecting which tool schemas to include in LLM requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSetContext {
    /// Chat mode: all tools available.
    Chat,
    /// Analysis mode: no tools needed (LLM generates structured output).
    #[allow(dead_code)]
    Analysis,
    /// Perspective mode: only PerspectiveTools and GraphTools.
    #[allow(dead_code)]
    Perspective,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub skill_name: String,
    pub args: serde_json::Value,
    pub result: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    Enabled,
    Disabled,
    #[allow(dead_code)]
    ConditionalOn(String),
}

/// Data specific to custom (LLM-powered) skills loaded from TOML files.
#[derive(Debug, Clone)]
pub struct CustomSkillData {
    pub prompt_template: String,
}

/// Context passed to skill invocations, grouping all available state.
pub struct SkillContext<'a> {
    pub text_buffer: &'a mut TextBuffer,
    pub scene_map: &'a mut SceneMap,
    pub file_buffer_manager: Option<&'a mut FileBufferManager>,
    pub graph: &'a NarrativeGraph,
    pub intent: Option<&'a mut DeclaredIntent>,
    pub perspectives: Option<&'a mut CharacterPerspective>,
    pub manifest: Option<&'a Manifest>,
    pub project_root: Option<&'a Path>,
}

pub struct Skills {
    registry: HashMap<String, SkillDefinition>,
    custom_data: HashMap<String, CustomSkillData>,
    execution_log: Vec<Execution>,
    permissions: HashMap<String, Permission>,
    provider_is_local: bool,
}

impl Skills {
    pub fn new() -> Self {
        let mut skills = Self {
            registry: HashMap::new(),
            custom_data: HashMap::new(),
            execution_log: Vec::new(),
            permissions: HashMap::new(),
            provider_is_local: false,
        };
        skills.register_defaults();
        skills
    }

    /// Invoke a skill. Some skills (perspective tools) require an LLM call and are async.
    pub async fn invoke(
        &mut self,
        skill_name: &str,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
        provider: Option<&mut Provider>,
    ) -> serde_json::Value {
        let start = std::time::Instant::now();

        // Check if skill exists
        if !self.registry.contains_key(skill_name) {
            return serde_json::json!({ "error": format!("Unknown skill: {skill_name}") });
        }

        // Permission check
        if !self.is_permitted(skill_name) {
            return serde_json::json!({
                "error": format!("Skill '{}' is not permitted", skill_name)
            });
        }

        let result = match skill_name {
            // File Tools
            "story_grep" => self.with_story_access(ctx, |story| self.exec_story_grep(args, &story)),
            "read_scene" => self.with_story_access(ctx, |story| self.exec_read_scene(args, &story)),
            "list_scenes" => self.with_story_access(ctx, |story| self.exec_list_scenes(&story)),
            "story_stats" => self.with_story_access(ctx, |story| self.exec_story_stats(&story, ctx.graph)),
            "read_context_file" => self.exec_read_context_file(args, ctx.manifest, ctx.project_root),
            "list_files" => self.exec_list_files(ctx.manifest),

            // Graph Tools
            "query_graph" => self.exec_query_graph(args, ctx.graph),
            "get_character_arc" => self.exec_get_character_arc(args, ctx.graph),
            "get_conflicts" => self.exec_get_conflicts(args, ctx.graph),
            "find_dead_scenes" => self.exec_find_dead_scenes(ctx.graph),
            "get_scene_analysis" => self.with_story_access(ctx, |story| {
                self.exec_get_scene_analysis(args, ctx.graph, &story)
            }),
            "scan_story" => {
                // Handled at the agent level (needs mutable graph access).
                // This branch should not be reached in the GUI agent.
                serde_json::json!({ "error": "scan_story must be handled at the agent level" })
            }
            "get_divergences" => {
                let intent_ref: Option<&DeclaredIntent> = ctx.intent.as_deref();
                self.exec_get_divergences(ctx.graph, intent_ref)
            }

            // Perspective Tools (async, need provider)
            "interpret_as_character" => {
                self.exec_interpret_as_character(args, ctx, provider).await
            }
            "compare_perspectives" => {
                self.exec_compare_perspectives(args, ctx, provider).await
            }
            "find_blind_spots" => {
                self.exec_find_blind_spots(args, ctx, provider).await
            }
            "get_knowledge_at" => self.exec_get_knowledge_at(args, ctx),

            // Structural Tools
            "story_lint" => self.exec_story_lint(ctx),
            "pacing_analysis" => self.exec_pacing_analysis(ctx),
            "arc_completeness" => self.exec_arc_completeness(args, ctx.graph),
            "declare_intent" => self.exec_declare_intent(args, ctx),
            "retract_intent" => self.exec_retract_intent(args, ctx),

            // Canvas Tools
            "write_to_canvas" => self.exec_write_to_canvas(args, ctx),
            "replace_in_canvas" => self.exec_replace_in_canvas(args, ctx),
            "insert_scene" => self.exec_insert_scene(args, ctx),

            _ => {
                if self.custom_data.contains_key(skill_name) {
                    self.exec_custom_skill(skill_name, args, provider).await
                } else {
                    serde_json::json!({ "error": format!("Unknown skill: {skill_name}") })
                }
            }
        };

        let duration = start.elapsed();
        let duration_ms = duration.as_millis() as u64;

        self.execution_log.push(Execution {
            skill_name: skill_name.to_string(),
            args: args.clone(),
            result: result.clone(),
            timestamp: Utc::now(),
            duration_ms,
        });

        // Audit trail for custom skills
        if self.custom_data.contains_key(skill_name) {
            if let Some(root) = ctx.project_root {
                self.write_audit_log(root, skill_name, args, &result, duration_ms);
            }
        }

        result
    }

    fn with_story_access<T>(
        &self,
        ctx: &SkillContext<'_>,
        f: impl FnOnce(StoryAccess<'_>) -> T,
    ) -> T {
        let story = StoryAccess::new(
            &*ctx.text_buffer,
            &*ctx.scene_map,
            ctx.file_buffer_manager.as_deref(),
        );
        f(story)
    }

}

impl Default for Skills {
    fn default() -> Self {
        Self::new()
    }
}

/// Find a character's ID by name (case-insensitive)
fn find_character_id(graph: &NarrativeGraph, name: &str) -> Option<String> {
    graph.get_characters().iter().find_map(|c| {
        if let GraphNode::Character { id, name: n, .. } = c {
            if n.eq_ignore_ascii_case(name) {
                Some(id.clone())
            } else {
                None
            }
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::file_buffer_manager::FileBufferManager;
    use crate::concepts::manifest::{Manifest, ManifestMeta, StoryFile};
    use crate::concepts::narrative_graph::{new_id, GraphEdge, GraphNode, Scope, Status};
    use crate::concepts::scene_map::ParseMode;
    use std::path::PathBuf;

    fn make_test_context() -> (TextBuffer, SceneMap, NarrativeGraph, DeclaredIntent) {
        let text = "## Scene 1\n\nMarcus drew his sword. Elena watched from the shadows.\n\n---\n\n## Scene 2\n\nElena crept forward alone. The night was silent.\n";
        let text_buffer = TextBuffer::from_str(text, PathBuf::from("/tmp/test.md"));

        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text, "");

        let mut graph = NarrativeGraph::new();

        let char_marcus = "marcus_id".to_string();
        let char_elena = "elena_id".to_string();

        graph.add_node(GraphNode::Character {
            id: char_marcus.clone(),
            name: "Marcus".to_string(),
            aliases: vec![],
            description: Some("A warrior".to_string()),
        });
        graph.add_node(GraphNode::Character {
            id: char_elena.clone(),
            name: "Elena".to_string(),
            aliases: vec![],
            description: Some("A spy".to_string()),
        });

        let scene_ids: Vec<String> = scene_map.list_scenes().iter().map(|s| s.id.clone()).collect();

        if scene_ids.len() >= 2 {
            graph.add_node(GraphNode::Scene {
                id: scene_ids[0].clone(),
                title: Some("Scene 1".to_string()),
                summary: "Marcus and Elena".to_string(),
                characters_present: vec![char_marcus.clone(), char_elena.clone()],
                location: None,
                time: None,
                file_path: String::new(),
            });
            graph.add_node(GraphNode::Scene {
                id: scene_ids[1].clone(),
                title: Some("Scene 2".to_string()),
                summary: "Elena alone".to_string(),
                characters_present: vec![char_elena.clone()],
                location: None,
                time: None,
                file_path: String::new(),
            });

            let obj_id = new_id();
            graph.add_node(GraphNode::Objective {
                id: obj_id.clone(),
                character_id: char_marcus.clone(),
                scope: Scope::Overarching,
                description: "Survive the war".to_string(),
                evidence: vec![],
                confidence: 0.9,
                status: Status::Active,
            });
            graph.add_edge(&char_marcus, &obj_id, GraphEdge::Pursues { scene_id: None });
            graph.add_edge(&scene_ids[0], &obj_id, GraphEdge::Advances);
        }

        let intent = DeclaredIntent::new();
        (text_buffer, scene_map, graph, intent)
    }

    fn make_multi_file_canvas_context(
    ) -> (
        tempfile::TempDir,
        TextBuffer,
        SceneMap,
        FileBufferManager,
        NarrativeGraph,
        DeclaredIntent,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("story.md"),
            "Primary story fallback that should remain unchanged.",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("chapter-1.md"),
            "## Scene 1\n\nMarcus arrives at the embassy before dawn.\n\n## Scene 2\n\nElena waits in silence.",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("chapter-2.md"),
            "## Scene 3\n\nThe station is empty and cold.",
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
                },
                StoryFile {
                    path: "chapter-2.md".to_string(),
                    format: "prose".to_string(),
                    order: 2,
                    content_hash: "h2".to_string(),
                },
            ],
            context_files: vec![],
            excluded: vec![],
        };

        let text_buffer = TextBuffer::from_file(tmp.path().join("story.md")).unwrap();
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(&text_buffer.read_all(), "story.md");
        let fbm = FileBufferManager::from_manifest(&manifest, tmp.path()).unwrap();
        let graph = NarrativeGraph::new();
        let intent = DeclaredIntent::new();

        (tmp, text_buffer, scene_map, fbm, graph, intent)
    }

    #[tokio::test]
    async fn test_get_scene_analysis_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke("get_scene_analysis", &serde_json::json!({"scene": "1"}), &mut ctx, None)
            .await;
        // Should return analysis or an error if scene not in graph by that number
        assert!(result.get("error").is_some() || result.get("scene_id").is_some());
    }

    #[tokio::test]
    async fn test_get_divergences_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        intent.declare("marcus_id", "description", "A prince".to_string(), None);

        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke("get_divergences", &serde_json::json!({}), &mut ctx, None)
            .await;
        assert_eq!(result["divergence_count"], 1);
    }

    #[tokio::test]
    async fn test_story_lint_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke("story_lint", &serde_json::json!({}), &mut ctx, None)
            .await;
        assert!(result.get("issue_count").is_some());
    }

    #[tokio::test]
    async fn test_pacing_analysis_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke("pacing_analysis", &serde_json::json!({}), &mut ctx, None)
            .await;
        assert!(result["scene_count"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_arc_completeness_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke("arc_completeness", &serde_json::json!({}), &mut ctx, None)
            .await;
        let chars = result["characters"].as_array().unwrap();
        assert!(!chars.is_empty());
    }

    #[tokio::test]
    async fn test_get_knowledge_at_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        let mut perspectives = CharacterPerspective::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: Some(&mut perspectives),
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke(
                "get_knowledge_at",
                &serde_json::json!({"character": "Elena"}),
                &mut ctx,
                None,
            )
            .await;
        assert!(result["scenes_witnessed"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_declare_intent_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke(
                "declare_intent",
                &serde_json::json!({
                    "node_id": "marcus_id",
                    "field": "description",
                    "value": "A fallen prince"
                }),
                &mut ctx,
                None,
            )
            .await;
        assert_eq!(result["status"], "declared");

        // Verify the intent store was updated
        let decl = intent.get_declaration("marcus_id", "description").unwrap();
        assert_eq!(decl.value, "A fallen prince");
    }

    #[tokio::test]
    async fn test_declare_intent_invalid_node() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke(
                "declare_intent",
                &serde_json::json!({
                    "node_id": "nonexistent_id",
                    "field": "description",
                    "value": "Should fail"
                }),
                &mut ctx,
                None,
            )
            .await;
        assert!(result["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_retract_intent_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        intent.declare("marcus_id", "description", "Override".to_string(), None);

        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke(
                "retract_intent",
                &serde_json::json!({
                    "node_id": "marcus_id",
                    "field": "description"
                }),
                &mut ctx,
                None,
            )
            .await;
        assert_eq!(result["status"], "retracted");

        // Verify removal
        assert!(intent.get_declaration("marcus_id", "description").is_none());
    }

    #[tokio::test]
    async fn test_write_to_canvas_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let original_len = ctx.text_buffer.read_all().len();
        let result = skills
            .invoke(
                "write_to_canvas",
                &serde_json::json!({"position": 0, "text": "INSERTED: "}),
                &mut ctx,
                None,
            )
            .await;
        assert_eq!(result["status"], "written");
        assert!(ctx.text_buffer.read_all().starts_with("INSERTED: "));
        assert!(ctx.text_buffer.read_all().len() > original_len);
    }

    #[tokio::test]
    async fn test_replace_in_canvas_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();

        // First, note original text starts with "## Scene 1"
        assert!(text_buffer.read_all().starts_with("## Scene 1"));

        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke(
                "replace_in_canvas",
                &serde_json::json!({"start": 0, "end": 10, "text": "## Part One"}),
                &mut ctx,
                None,
            )
            .await;
        assert_eq!(result["status"], "replaced");
        assert!(ctx.text_buffer.read_all().starts_with("## Part One"));
    }

    #[tokio::test]
    async fn test_insert_scene_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let original_scene_count = scene_map.scene_count();

        let mut skills = Skills::new();
        let insert_pos = text_buffer.read_all().len();

        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke(
                "insert_scene",
                &serde_json::json!({
                    "position": insert_pos,
                    "title": "New Scene",
                    "content": "This is new content with enough words to be a real scene for testing."
                }),
                &mut ctx,
                None,
            )
            .await;
        assert_eq!(result["status"], "inserted");
        assert!(result["new_scene_count"].as_u64().unwrap() as usize > original_scene_count);
    }

    #[tokio::test]
    async fn test_write_to_canvas_targets_manifest_file() {
        let (_tmp, mut text_buffer, mut scene_map, mut fbm, graph, mut intent) =
            make_multi_file_canvas_context();
        let original_primary = text_buffer.read_all();
        let original_target = fbm
            .get_entry("chapter-2.md")
            .unwrap()
            .text_buffer
            .read_all();

        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: Some(&mut fbm),
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke(
                "write_to_canvas",
                &serde_json::json!({
                    "file": "chapter-2.md",
                    "position": 0,
                    "text": "INSERTED: "
                }),
                &mut ctx,
                None,
            )
            .await;

        assert_eq!(result["status"], "written");
        assert_eq!(result["file_path"], "chapter-2.md");
        assert_eq!(ctx.text_buffer.read_all(), original_primary);
        let updated_target = ctx
            .file_buffer_manager
            .as_deref()
            .unwrap()
            .get_entry("chapter-2.md")
            .unwrap()
            .text_buffer
            .read_all();
        assert!(updated_target.starts_with("INSERTED: "));
        assert!(updated_target.len() > original_target.len());
    }

    #[tokio::test]
    async fn test_replace_in_canvas_requires_file_for_multi_file_manifest() {
        let (_tmp, mut text_buffer, mut scene_map, mut fbm, graph, mut intent) =
            make_multi_file_canvas_context();
        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: Some(&mut fbm),
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke(
                "replace_in_canvas",
                &serde_json::json!({
                    "start": 0,
                    "end": 5,
                    "text": "FAIL"
                }),
                &mut ctx,
                None,
            )
            .await;

        assert!(result["error"]
            .as_str()
            .unwrap()
            .contains("require a 'file' field"));
    }

    #[tokio::test]
    async fn test_insert_scene_after_scene_targets_resolved_manifest_file() {
        let (_tmp, mut text_buffer, mut scene_map, mut fbm, graph, mut intent) =
            make_multi_file_canvas_context();
        let original_scene_count = fbm
            .get_entry("chapter-2.md")
            .unwrap()
            .scene_map
            .scene_count();

        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: Some(&mut fbm),
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke(
                "insert_scene",
                &serde_json::json!({
                    "after_scene": "3",
                    "title": "Aftermath",
                    "content": "A new beat unfolds in the empty station."
                }),
                &mut ctx,
                None,
            )
            .await;

        assert_eq!(result["status"], "inserted");
        assert_eq!(result["file_path"], "chapter-2.md");
        assert_eq!(
            result["new_scene_count"].as_u64().unwrap() as usize,
            original_scene_count + 1
        );
        let updated_target = ctx
            .file_buffer_manager
            .as_deref()
            .unwrap()
            .get_entry("chapter-2.md")
            .unwrap()
            .text_buffer
            .read_all();
        assert!(updated_target.contains("## Aftermath"));
    }

    #[test]
    fn test_s5_2_cloud_restrictions() {
        let mut skills = Skills::new();

        // Verify skill is enabled by default
        let schemas = skills.tool_schemas();
        assert!(schemas.iter().any(|s| s.name == "story_grep"));

        // Disable a skill (simulating cloud restriction)
        skills.set_permission("story_grep", Permission::Disabled);

        // Verify it's excluded from tool schemas
        let schemas = skills.tool_schemas();
        assert!(!schemas.iter().any(|s| s.name == "story_grep"));

        // Other skills still available
        assert!(schemas.iter().any(|s| s.name == "read_scene"));
    }

    #[tokio::test]
    async fn test_list_files_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let manifest = crate::concepts::manifest::Manifest {
            meta: crate::concepts::manifest::ManifestMeta {
                last_scan: "2026-01-01T00:00:00Z".to_string(),
                classification_model: "test".to_string(),
            },
            story_files: vec![crate::concepts::manifest::StoryFile {
                path: "chapter-1.md".to_string(),
                format: "prose".to_string(),
                order: 1,
                content_hash: "abc".to_string(),
            }],
            context_files: vec![
                crate::concepts::manifest::ContextFile {
                    path: "outline.md".to_string(),
                    role: crate::concepts::manifest::FileRole::Outline,
                    content_hash: "def".to_string(),
                },
                crate::concepts::manifest::ContextFile {
                    path: "characters.md".to_string(),
                    role: crate::concepts::manifest::FileRole::Characters,
                    content_hash: "ghi".to_string(),
                },
            ],
            excluded: vec![crate::concepts::manifest::ExcludedFile {
                path: "old-draft.md".to_string(),
                reason: "Older version".to_string(),
            }],
        };

        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: Some(&manifest),
            project_root: None,
        };

        let result = skills
            .invoke("list_files", &serde_json::json!({}), &mut ctx, None)
            .await;

        assert_eq!(result["story_file_count"], 1);
        assert_eq!(result["context_file_count"], 2);
        assert_eq!(result["excluded_count"], 1);

        let story = result["story_files"].as_array().unwrap();
        assert_eq!(story[0]["path"], "chapter-1.md");
        assert_eq!(story[0]["role"], "story");

        let context = result["context_files"].as_array().unwrap();
        assert_eq!(context[0]["path"], "outline.md");
        assert_eq!(context[0]["role"], "outline");
        assert_eq!(context[1]["role"], "characters");

        let excluded = result["excluded"].as_array().unwrap();
        assert_eq!(excluded[0]["path"], "old-draft.md");
    }

    #[tokio::test]
    async fn test_list_files_no_manifest() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke("list_files", &serde_json::json!({}), &mut ctx, None)
            .await;
        assert!(result["error"].as_str().unwrap().contains("manifest"));
    }

    #[tokio::test]
    async fn test_read_context_file_by_role() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("outline.md"), "# Story Outline\n\nAct 1: Setup\nAct 2: Confrontation\nAct 3: Resolution").unwrap();

        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let manifest = crate::concepts::manifest::Manifest {
            meta: crate::concepts::manifest::ManifestMeta {
                last_scan: String::new(),
                classification_model: String::new(),
            },
            story_files: vec![],
            context_files: vec![crate::concepts::manifest::ContextFile {
                path: "outline.md".to_string(),
                role: crate::concepts::manifest::FileRole::Outline,
                content_hash: "hash".to_string(),
            }],
            excluded: vec![],
        };

        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: Some(&manifest),
            project_root: Some(root),
        };

        // Look up by role
        let result = skills
            .invoke("read_context_file", &serde_json::json!({"file": "outline"}), &mut ctx, None)
            .await;
        assert_eq!(result["path"], "outline.md");
        assert_eq!(result["role"], "outline");
        assert!(result["content"].as_str().unwrap().contains("Act 1"));
        assert!(result["word_count"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_read_context_file_by_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("characters.md"), "# Characters\n\nMarcus: A warrior prince").unwrap();

        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let manifest = crate::concepts::manifest::Manifest {
            meta: crate::concepts::manifest::ManifestMeta {
                last_scan: String::new(),
                classification_model: String::new(),
            },
            story_files: vec![],
            context_files: vec![crate::concepts::manifest::ContextFile {
                path: "characters.md".to_string(),
                role: crate::concepts::manifest::FileRole::Characters,
                content_hash: "hash".to_string(),
            }],
            excluded: vec![],
        };

        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: Some(&manifest),
            project_root: Some(root),
        };

        // Look up by path
        let result = skills
            .invoke("read_context_file", &serde_json::json!({"file": "characters.md"}), &mut ctx, None)
            .await;
        assert_eq!(result["path"], "characters.md");
        assert_eq!(result["role"], "characters");
        assert!(result["content"].as_str().unwrap().contains("Marcus"));
    }

    #[tokio::test]
    async fn test_read_context_file_not_found() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let manifest = crate::concepts::manifest::Manifest {
            meta: crate::concepts::manifest::ManifestMeta {
                last_scan: String::new(),
                classification_model: String::new(),
            },
            story_files: vec![],
            context_files: vec![crate::concepts::manifest::ContextFile {
                path: "outline.md".to_string(),
                role: crate::concepts::manifest::FileRole::Outline,
                content_hash: "hash".to_string(),
            }],
            excluded: vec![],
        };

        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: Some(&manifest),
            project_root: Some(std::path::Path::new("/tmp")),
        };

        let result = skills
            .invoke("read_context_file", &serde_json::json!({"file": "nonexistent"}), &mut ctx, None)
            .await;
        assert!(result["error"].as_str().unwrap().contains("not found"));
        // Should list available files
        assert!(result["available"].as_array().is_some());
    }

    #[test]
    fn test_context_file_skills_registered() {
        let skills = Skills::new();
        let schemas = skills.tool_schemas();
        assert!(schemas.iter().any(|s| s.name == "read_context_file"));
        assert!(schemas.iter().any(|s| s.name == "list_files"));
    }

    // -------------------------------------------------------------------------
    // Custom Skills Framework tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_custom_skill_toml_parsing() {
        let toml_content = r#"
name = "check_dialect"
description = "Check if dialogue matches a character's dialect"
prompt_template = "Analyze: {{character}} says: {{text}}"

[input_schema]
type = "object"
required = ["character", "text"]

[input_schema.properties.character]
type = "string"
description = "Character name"

[input_schema.properties.text]
type = "string"
description = "Dialogue to check"

[output_schema]
type = "object"

[output_schema.properties.consistent]
type = "boolean"
"#;
        let skill: CustomSkillToml = toml::from_str(toml_content).unwrap();
        assert_eq!(skill.name, "check_dialect");
        assert_eq!(
            skill.description,
            "Check if dialogue matches a character's dialect"
        );
        assert!(skill.prompt_template.contains("{{character}}"));
        assert!(skill.output_schema.is_some());
    }

    #[test]
    fn test_custom_skill_load_and_register() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        std::fs::create_dir(&skills_dir).unwrap();

        std::fs::write(
            skills_dir.join("check_dialect.toml"),
            r#"
name = "check_dialect"
description = "Check dialect"
prompt_template = "Check: {{text}}"

[input_schema]
type = "object"
required = ["text"]

[input_schema.properties.text]
type = "string"
"#,
        )
        .unwrap();

        let mut skills = Skills::new();
        let errors = skills.load_custom_skills(&skills_dir);
        assert!(errors.is_empty(), "Unexpected errors: {:?}", errors);

        // Should be registered in tool schemas
        let schemas = skills.tool_schemas();
        assert!(schemas.iter().any(|s| s.name == "check_dialect"));

        // Should be in custom_data
        assert!(skills.custom_data.contains_key("check_dialect"));

        // Should have correct category
        let def = skills.registry.get("check_dialect").unwrap();
        assert_eq!(def.category, SkillCategory::CustomTools);
    }

    #[test]
    fn test_custom_skill_validation_empty_name() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        std::fs::create_dir(&skills_dir).unwrap();

        std::fs::write(
            skills_dir.join("bad.toml"),
            r#"
name = ""
description = "Bad skill"
prompt_template = "Do something"

[input_schema]
type = "object"
"#,
        )
        .unwrap();

        let mut skills = Skills::new();
        let errors = skills.load_custom_skills(&skills_dir);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("name cannot be empty"));
    }

    #[test]
    fn test_custom_skill_validation_empty_template() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        std::fs::create_dir(&skills_dir).unwrap();

        std::fs::write(
            skills_dir.join("bad.toml"),
            r#"
name = "bad_skill"
description = "Bad skill"
prompt_template = ""

[input_schema]
type = "object"
"#,
        )
        .unwrap();

        let mut skills = Skills::new();
        let errors = skills.load_custom_skills(&skills_dir);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("prompt_template cannot be empty"));
    }

    #[test]
    fn test_custom_skill_validation_invalid_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        std::fs::create_dir(&skills_dir).unwrap();

        std::fs::write(skills_dir.join("bad.toml"), "not valid toml {{{{").unwrap();

        let mut skills = Skills::new();
        let errors = skills.load_custom_skills(&skills_dir);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("invalid TOML"));
    }

    #[test]
    fn test_custom_skill_nonexistent_dir() {
        let mut skills = Skills::new();
        let errors = skills.load_custom_skills(Path::new("/nonexistent/skills"));
        assert!(errors.is_empty()); // Missing dir is not an error
    }

    #[test]
    fn test_permission_conditional_is_local() {
        let mut skills = Skills::new();
        skills.set_permission(
            "story_grep",
            Permission::ConditionalOn("is_local".to_string()),
        );

        // When provider is not local, should be denied
        skills.set_provider_locality(false);
        assert!(!skills.is_permitted("story_grep"));
        let schemas = skills.tool_schemas();
        assert!(!schemas.iter().any(|s| s.name == "story_grep"));

        // When provider is local, should be permitted
        skills.set_provider_locality(true);
        assert!(skills.is_permitted("story_grep"));
        let schemas = skills.tool_schemas();
        assert!(schemas.iter().any(|s| s.name == "story_grep"));
    }

    #[test]
    fn test_permission_conditional_is_cloud() {
        let mut skills = Skills::new();
        skills.set_permission(
            "story_grep",
            Permission::ConditionalOn("is_cloud".to_string()),
        );

        // When provider is local, "is_cloud" should deny
        skills.set_provider_locality(true);
        assert!(!skills.is_permitted("story_grep"));

        // When provider is not local (cloud), "is_cloud" should permit
        skills.set_provider_locality(false);
        assert!(skills.is_permitted("story_grep"));
    }

    #[test]
    fn test_permission_conditional_unknown_condition() {
        let mut skills = Skills::new();
        skills.set_permission(
            "story_grep",
            Permission::ConditionalOn("unknown_cond".to_string()),
        );
        // Unknown conditions default to denied
        assert!(!skills.is_permitted("story_grep"));
    }

    #[tokio::test]
    async fn test_invoke_permission_denied() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        skills.set_permission("story_grep", Permission::Disabled);

        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            file_buffer_manager: None,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            manifest: None,
            project_root: None,
        };

        let result = skills
            .invoke(
                "story_grep",
                &serde_json::json!({"pattern": "test"}),
                &mut ctx,
                None,
            )
            .await;
        assert!(result["error"]
            .as_str()
            .unwrap()
            .contains("not permitted"));
    }

    #[test]
    fn test_audit_log_written() {
        let tmp = tempfile::tempdir().unwrap();
        let laires_dir = tmp.path().join(".laires");
        std::fs::create_dir(&laires_dir).unwrap();

        let skills = Skills::new();
        let args = serde_json::json!({"text": "Hello"});
        let result = serde_json::json!({"response": "World"});
        skills.write_audit_log(tmp.path(), "test_skill", &args, &result, 42);

        let log_path = laires_dir.join("skill_log.jsonl");
        assert!(log_path.exists());
        let content = std::fs::read_to_string(&log_path).unwrap();
        let entry: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(entry["skill"], "test_skill");
        assert_eq!(entry["duration_ms"], 42);
        assert!(entry["timestamp"].as_str().is_some());
    }

    #[test]
    fn test_toml_to_json_conversion() {
        let toml_val: toml::Value = toml::from_str(
            r#"
type = "object"
required = ["name"]

[properties.name]
type = "string"
description = "A name"
"#,
        )
        .unwrap();

        let json = toml_to_json(toml_val);
        assert_eq!(json["type"], "object");
        assert_eq!(json["required"][0], "name");
        assert_eq!(json["properties"]["name"]["type"], "string");
    }

    #[test]
    fn test_custom_skill_schema_in_tool_schemas() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        std::fs::create_dir(&skills_dir).unwrap();

        std::fs::write(
            skills_dir.join("analyze_tone.toml"),
            r#"
name = "analyze_tone"
description = "Analyze the tone of a passage"
prompt_template = "What is the tone of: {{text}}"

[input_schema]
type = "object"
required = ["text"]

[input_schema.properties.text]
type = "string"
description = "Text to analyze"
"#,
        )
        .unwrap();

        let mut skills = Skills::new();
        skills.load_custom_skills(&skills_dir);

        let schemas = skills.tool_schemas();
        let schema = schemas.iter().find(|s| s.name == "analyze_tone").unwrap();
        assert_eq!(schema.description, "Analyze the tone of a passage");
        assert_eq!(schema.parameters["type"], "object");
        assert_eq!(schema.parameters["properties"]["text"]["type"], "string");
    }

    // -------------------------------------------------------------------------
    // Phase E: tool_schemas_for_context tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_tool_schemas_for_context_chat_returns_all() {
        let skills = Skills::new();
        let all = skills.tool_schemas();
        let chat = skills.tool_schemas_for_context(SkillSetContext::Chat);
        assert_eq!(chat.len(), all.len());
    }

    #[test]
    fn test_tool_schemas_for_context_analysis_returns_empty() {
        let skills = Skills::new();
        let analysis = skills.tool_schemas_for_context(SkillSetContext::Analysis);
        assert!(analysis.is_empty());
    }

    #[test]
    fn test_tool_schemas_for_context_perspective_filters() {
        let skills = Skills::new();
        let perspective = skills.tool_schemas_for_context(SkillSetContext::Perspective);

        // Should contain PerspectiveTools and GraphTools only
        assert!(!perspective.is_empty());

        // Verify all returned schemas are in the allowed categories
        let perspective_names: Vec<&str> = vec![
            "interpret_as_character",
            "compare_perspectives",
            "find_blind_spots",
            "get_knowledge_at",
        ];
        let graph_names: Vec<&str> = vec![
            "query_graph",
            "get_character_arc",
            "get_conflicts",
            "find_dead_scenes",
            "get_scene_analysis",
            "get_divergences",
            "scan_story",
        ];
        let allowed: Vec<&str> = perspective_names
            .iter()
            .chain(graph_names.iter())
            .copied()
            .collect();

        for schema in &perspective {
            assert!(
                allowed.contains(&schema.name.as_str()),
                "Unexpected tool in Perspective context: {}",
                schema.name
            );
        }

        // Should NOT contain file tools, structural tools, or canvas tools
        assert!(!perspective.iter().any(|s| s.name == "story_grep"));
        assert!(!perspective.iter().any(|s| s.name == "read_scene"));
        assert!(!perspective.iter().any(|s| s.name == "story_lint"));
        assert!(!perspective.iter().any(|s| s.name == "write_to_canvas"));
    }

    #[test]
    fn test_tool_schemas_for_context_respects_permissions() {
        let mut skills = Skills::new();
        skills.set_permission("query_graph", Permission::Disabled);

        let perspective = skills.tool_schemas_for_context(SkillSetContext::Perspective);
        assert!(!perspective.iter().any(|s| s.name == "query_graph"));

        let chat = skills.tool_schemas_for_context(SkillSetContext::Chat);
        assert!(!chat.iter().any(|s| s.name == "query_graph"));
    }
}
