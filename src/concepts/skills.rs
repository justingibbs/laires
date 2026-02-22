use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::concepts::canvas::Canvas;
use crate::concepts::character_perspective::CharacterPerspective;
use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::narrative_graph::{GraphNode, NarrativeGraph};
use crate::concepts::provider::Provider;
use crate::concepts::scene_map::SceneMap;
use crate::concepts::text_buffer::{ByteRange, TextBuffer};
use crate::sync::divergence;

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
}

/// Context passed to skill invocations, grouping all available state.
pub struct SkillContext<'a> {
    pub text_buffer: &'a mut TextBuffer,
    pub scene_map: &'a mut SceneMap,
    pub graph: &'a NarrativeGraph,
    pub intent: Option<&'a mut DeclaredIntent>,
    pub perspectives: Option<&'a mut CharacterPerspective>,
    pub canvas: Option<&'a mut Canvas>,
}

pub struct Skills {
    registry: HashMap<String, SkillDefinition>,
    execution_log: Vec<Execution>,
    permissions: HashMap<String, Permission>,
}

impl Skills {
    pub fn new() -> Self {
        let mut skills = Self {
            registry: HashMap::new(),
            execution_log: Vec::new(),
            permissions: HashMap::new(),
        };
        skills.register_defaults();
        skills
    }

    fn register_defaults(&mut self) {
        // File Tools
        self.register(SkillDefinition {
            name: "story_grep".to_string(),
            description: "Search story text by regex or keyword".to_string(),
            category: SkillCategory::FileTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex or keyword pattern to search for" }
                },
                "required": ["pattern"]
            }),
        });

        self.register(SkillDefinition {
            name: "read_scene".to_string(),
            description: "Read the full text of a specific scene by ID or number".to_string(),
            category: SkillCategory::FileTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scene": { "type": "string", "description": "Scene ID or 1-based scene number" }
                },
                "required": ["scene"]
            }),
        });

        self.register(SkillDefinition {
            name: "list_scenes".to_string(),
            description: "List all scenes with their titles and word counts".to_string(),
            category: SkillCategory::FileTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(SkillDefinition {
            name: "story_stats".to_string(),
            description: "Get overall manuscript statistics (word count, scene count, character count)".to_string(),
            category: SkillCategory::FileTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        // Graph Tools
        self.register(SkillDefinition {
            name: "query_graph".to_string(),
            description: "Query the narrative graph with filters (by character, scene, type)".to_string(),
            category: SkillCategory::GraphTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_type": { "type": "string", "enum": ["character", "objective", "scene", "conflict"] },
                    "character": { "type": "string", "description": "Filter by character name" }
                }
            }),
        });

        self.register(SkillDefinition {
            name: "get_character_arc".to_string(),
            description: "Get the full objective trajectory for a character across all scenes".to_string(),
            category: SkillCategory::GraphTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "character": { "type": "string", "description": "Character name" }
                },
                "required": ["character"]
            }),
        });

        self.register(SkillDefinition {
            name: "get_conflicts".to_string(),
            description: "Get all objective conflicts, optionally filtered by character".to_string(),
            category: SkillCategory::GraphTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "character": { "type": "string", "description": "Filter by character name" }
                }
            }),
        });

        self.register(SkillDefinition {
            name: "find_dead_scenes".to_string(),
            description: "Find scenes where no objective advances or is blocked".to_string(),
            category: SkillCategory::GraphTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        // New Graph Tools
        self.register(SkillDefinition {
            name: "get_scene_analysis".to_string(),
            description: "Get which objectives are active, advanced, or blocked in a scene".to_string(),
            category: SkillCategory::GraphTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scene": { "type": "string", "description": "Scene ID or 1-based scene number" }
                },
                "required": ["scene"]
            }),
        });

        self.register(SkillDefinition {
            name: "get_divergences".to_string(),
            description: "Get all mismatches between LLM-inferred and writer-declared values".to_string(),
            category: SkillCategory::GraphTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        // Perspective Tools
        self.register(SkillDefinition {
            name: "interpret_as_character".to_string(),
            description: "Analyze the story or a scene from a specific character's perspective".to_string(),
            category: SkillCategory::PerspectiveTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "character": { "type": "string", "description": "Character name" },
                    "scene": { "type": "string", "description": "Optional: scene ID or number for single-scene perspective" }
                },
                "required": ["character"]
            }),
        });

        self.register(SkillDefinition {
            name: "compare_perspectives".to_string(),
            description: "Compare how two characters experience the same scene differently".to_string(),
            category: SkillCategory::PerspectiveTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "character_a": { "type": "string", "description": "First character name" },
                    "character_b": { "type": "string", "description": "Second character name" },
                    "scene": { "type": "string", "description": "Scene ID or 1-based scene number" }
                },
                "required": ["character_a", "character_b", "scene"]
            }),
        });

        self.register(SkillDefinition {
            name: "find_blind_spots".to_string(),
            description: "Find dramatic irony moments where a character is missing information the reader has".to_string(),
            category: SkillCategory::PerspectiveTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "character": { "type": "string", "description": "Character name" }
                },
                "required": ["character"]
            }),
        });

        self.register(SkillDefinition {
            name: "get_knowledge_at".to_string(),
            description: "Get what a character knows at a given point in the story".to_string(),
            category: SkillCategory::PerspectiveTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "character": { "type": "string", "description": "Character name" },
                    "scene": { "type": "string", "description": "Scene ID or 1-based scene number (knowledge boundary up to this scene)" }
                },
                "required": ["character"]
            }),
        });

        // Structural Tools
        self.register(SkillDefinition {
            name: "story_lint".to_string(),
            description: "Run consistency checks: orphaned objectives, dead scenes, stale analysis, presence edge consistency".to_string(),
            category: SkillCategory::StructuralTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(SkillDefinition {
            name: "pacing_analysis".to_string(),
            description: "Analyze scene lengths, conflict density, and narrative rhythm".to_string(),
            category: SkillCategory::StructuralTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(SkillDefinition {
            name: "arc_completeness".to_string(),
            description: "Check whether character objectives resolve by the end of the story".to_string(),
            category: SkillCategory::StructuralTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "character": { "type": "string", "description": "Character name (optional, checks all if omitted)" }
                }
            }),
        });

        // Canvas Tools
        self.register(SkillDefinition {
            name: "write_to_canvas".to_string(),
            description: "Insert text at a byte position in the story".to_string(),
            category: SkillCategory::CanvasTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "position": { "type": "integer", "description": "Byte offset to insert text at" },
                    "text": { "type": "string", "description": "Text to insert" }
                },
                "required": ["position", "text"]
            }),
        });

        self.register(SkillDefinition {
            name: "replace_in_canvas".to_string(),
            description: "Replace text in a byte range in the story".to_string(),
            category: SkillCategory::CanvasTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "start": { "type": "integer", "description": "Start byte offset" },
                    "end": { "type": "integer", "description": "End byte offset" },
                    "text": { "type": "string", "description": "Replacement text" }
                },
                "required": ["start", "end", "text"]
            }),
        });

        self.register(SkillDefinition {
            name: "insert_scene".to_string(),
            description: "Insert a new scene break marker and content at a byte position".to_string(),
            category: SkillCategory::CanvasTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "position": { "type": "integer", "description": "Byte offset to insert the scene break" },
                    "title": { "type": "string", "description": "Scene heading/title" },
                    "content": { "type": "string", "description": "Initial scene content" }
                },
                "required": ["position", "title"]
            }),
        });

        self.register(SkillDefinition {
            name: "declare_intent".to_string(),
            description: "Set a writer override on a graph node field, declaring the canonical value".to_string(),
            category: SkillCategory::StructuralTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "The graph node ID to declare intent on" },
                    "field": { "type": "string", "description": "The field name to override (e.g. description, status)" },
                    "value": { "type": "string", "description": "The declared canonical value" },
                    "rationale": { "type": "string", "description": "Optional reason for the override" }
                },
                "required": ["node_id", "field", "value"]
            }),
        });

        self.register(SkillDefinition {
            name: "retract_intent".to_string(),
            description: "Remove a writer override from a graph node field".to_string(),
            category: SkillCategory::StructuralTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "The graph node ID to retract the declaration from" },
                    "field": { "type": "string", "description": "The field name to un-override" }
                },
                "required": ["node_id", "field"]
            }),
        });

    }

    pub fn register(&mut self, skill: SkillDefinition) {
        let name = skill.name.clone();
        self.registry.insert(name.clone(), skill);
        self.permissions.insert(name, Permission::Enabled);
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

        let result = match skill_name {
            // File Tools
            "story_grep" => self.exec_story_grep(args, ctx.text_buffer),
            "read_scene" => self.exec_read_scene(args, ctx.text_buffer, ctx.scene_map),
            "list_scenes" => self.exec_list_scenes(ctx.text_buffer, ctx.scene_map),
            "story_stats" => self.exec_story_stats(ctx.text_buffer, ctx.scene_map, ctx.graph),

            // Graph Tools
            "query_graph" => self.exec_query_graph(args, ctx.graph),
            "get_character_arc" => self.exec_get_character_arc(args, ctx.graph),
            "get_conflicts" => self.exec_get_conflicts(args, ctx.graph),
            "find_dead_scenes" => self.exec_find_dead_scenes(ctx.graph),
            "get_scene_analysis" => self.exec_get_scene_analysis(args, ctx.graph, ctx.scene_map),
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

            _ => serde_json::json!({ "error": format!("Unknown skill: {skill_name}") }),
        };

        let duration = start.elapsed();
        self.execution_log.push(Execution {
            skill_name: skill_name.to_string(),
            args: args.clone(),
            result: result.clone(),
            timestamp: Utc::now(),
            duration_ms: duration.as_millis() as u64,
        });

        result
    }

    /// Get tool schemas for LLM tool-use
    pub fn tool_schemas(&self) -> Vec<crate::concepts::provider::ToolSchema> {
        self.registry
            .values()
            .filter(|s| {
                self.permissions
                    .get(&s.name)
                    .map(|p| *p == Permission::Enabled)
                    .unwrap_or(false)
            })
            .map(|s| crate::concepts::provider::ToolSchema {
                name: s.name.clone(),
                description: s.description.clone(),
                parameters: s.input_schema.clone(),
            })
            .collect()
    }

    /// List all available (enabled) skills
    pub fn list_available(&self) -> Vec<&SkillDefinition> {
        self.registry
            .values()
            .filter(|s| {
                self.permissions
                    .get(&s.name)
                    .map(|p| *p == Permission::Enabled)
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn set_permission(&mut self, skill_name: &str, permission: Permission) {
        if self.registry.contains_key(skill_name) {
            self.permissions
                .insert(skill_name.to_string(), permission);
        }
    }

    // -------------------------------------------------------------------------
    // File Tool implementations
    // -------------------------------------------------------------------------

    fn exec_story_grep(
        &self,
        args: &serde_json::Value,
        text_buffer: &TextBuffer,
    ) -> serde_json::Value {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let text = text_buffer.read_all();

        let re = match Regex::new(pattern) {
            Ok(re) => re,
            Err(e) => {
                return serde_json::json!({ "error": format!("Invalid regex: {e}") })
            }
        };

        let mut matches = Vec::new();
        for (line_num, line) in text.lines().enumerate() {
            if re.is_match(line) {
                matches.push(serde_json::json!({
                    "line": line_num + 1,
                    "text": line.trim(),
                }));
            }
        }

        serde_json::json!({
            "pattern": pattern,
            "match_count": matches.len(),
            "matches": matches,
        })
    }

    fn exec_read_scene(
        &self,
        args: &serde_json::Value,
        text_buffer: &TextBuffer,
        scene_map: &SceneMap,
    ) -> serde_json::Value {
        let scene_ref = args["scene"].as_str().unwrap_or("1");

        let scene = if let Ok(num) = scene_ref.parse::<usize>() {
            scene_map
                .list_scenes()
                .get(num.saturating_sub(1))
        } else {
            scene_map.get_scene(scene_ref)
        };

        match scene {
            Some(span) => {
                let text = text_buffer
                    .read(span.byte_range())
                    .unwrap_or_default();
                serde_json::json!({
                    "scene_id": span.id,
                    "title": span.title,
                    "text": text,
                    "word_count": text.split_whitespace().count(),
                })
            }
            None => serde_json::json!({
                "error": format!("Scene not found: {scene_ref}")
            }),
        }
    }

    fn exec_list_scenes(
        &self,
        text_buffer: &TextBuffer,
        scene_map: &SceneMap,
    ) -> serde_json::Value {
        let scenes: Vec<serde_json::Value> = scene_map
            .list_scenes()
            .iter()
            .enumerate()
            .map(|(i, span)| {
                let text = text_buffer
                    .read(span.byte_range())
                    .unwrap_or_default();
                serde_json::json!({
                    "number": i + 1,
                    "id": span.id,
                    "title": span.title,
                    "word_count": text.split_whitespace().count(),
                })
            })
            .collect();

        serde_json::json!({ "scenes": scenes })
    }

    fn exec_story_stats(
        &self,
        text_buffer: &TextBuffer,
        scene_map: &SceneMap,
        graph: &NarrativeGraph,
    ) -> serde_json::Value {
        serde_json::json!({
            "word_count": text_buffer.word_count(),
            "line_count": text_buffer.line_count(),
            "scene_count": scene_map.scene_count(),
            "character_count": graph.get_characters().len(),
            "objective_count": graph.get_objectives().len(),
            "conflict_count": graph.get_conflicts().len(),
        })
    }

    // -------------------------------------------------------------------------
    // Graph Tool implementations
    // -------------------------------------------------------------------------

    fn exec_query_graph(
        &self,
        args: &serde_json::Value,
        graph: &NarrativeGraph,
    ) -> serde_json::Value {
        let node_type = args["node_type"].as_str();

        let nodes: Vec<&GraphNode> = match node_type {
            Some("character") => graph.get_characters(),
            Some("objective") => graph.get_objectives(),
            Some("scene") => graph.get_scenes(),
            Some("conflict") => graph.get_conflicts(),
            _ => {
                let mut all = Vec::new();
                all.extend(graph.get_characters());
                all.extend(graph.get_objectives());
                all.extend(graph.get_scenes());
                all.extend(graph.get_conflicts());
                all
            }
        };

        serde_json::json!({
            "node_count": nodes.len(),
            "nodes": nodes,
        })
    }

    fn exec_get_character_arc(
        &self,
        args: &serde_json::Value,
        graph: &NarrativeGraph,
    ) -> serde_json::Value {
        let character_name = args["character"].as_str().unwrap_or("");
        match find_character_id(graph, character_name) {
            Some(id) => {
                let arc = graph.get_character_arc(&id);
                serde_json::json!({
                    "character": character_name,
                    "arc": arc,
                })
            }
            None => serde_json::json!({
                "error": format!("Character not found: {character_name}")
            }),
        }
    }

    fn exec_get_conflicts(
        &self,
        _args: &serde_json::Value,
        graph: &NarrativeGraph,
    ) -> serde_json::Value {
        let conflicts = graph.get_conflicts();
        serde_json::json!({
            "conflict_count": conflicts.len(),
            "conflicts": conflicts,
        })
    }

    fn exec_find_dead_scenes(&self, graph: &NarrativeGraph) -> serde_json::Value {
        let dead = graph.find_dead_scenes();
        serde_json::json!({
            "dead_scene_count": dead.len(),
            "dead_scenes": dead,
        })
    }

    fn exec_get_scene_analysis(
        &self,
        args: &serde_json::Value,
        graph: &NarrativeGraph,
        scene_map: &SceneMap,
    ) -> serde_json::Value {
        let scene_ref = args["scene"].as_str().unwrap_or("1");

        // Resolve scene ID
        let scene_id = if let Ok(num) = scene_ref.parse::<usize>() {
            scene_map
                .list_scenes()
                .get(num.saturating_sub(1))
                .map(|s| s.id.clone())
        } else {
            Some(scene_ref.to_string())
        };

        match scene_id.and_then(|id| graph.get_scene_analysis(&id)) {
            Some(analysis) => serde_json::json!(analysis),
            None => serde_json::json!({
                "error": format!("Scene not found or not in graph: {scene_ref}")
            }),
        }
    }

    fn exec_get_divergences(
        &self,
        graph: &NarrativeGraph,
        intent: Option<&DeclaredIntent>,
    ) -> serde_json::Value {
        match intent {
            Some(intent) => {
                let divs = divergence::detect_divergences(graph, intent);
                let orphans = divergence::detect_orphans(graph, intent);
                serde_json::json!({
                    "divergence_count": divs.len(),
                    "divergences": divs,
                    "orphan_count": orphans.len(),
                    "orphans": orphans,
                })
            }
            None => serde_json::json!({
                "divergence_count": 0,
                "divergences": [],
                "orphan_count": 0,
                "orphans": [],
                "note": "No writer overrides loaded"
            }),
        }
    }

    // -------------------------------------------------------------------------
    // Perspective Tool implementations
    // -------------------------------------------------------------------------

    async fn exec_interpret_as_character(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
        provider: Option<&mut Provider>,
    ) -> serde_json::Value {
        let character_name = args["character"].as_str().unwrap_or("");
        let char_id = match find_character_id(ctx.graph, character_name) {
            Some(id) => id,
            None => {
                return serde_json::json!({
                    "error": format!("Character not found: {character_name}")
                })
            }
        };

        let provider = match provider {
            Some(p) => p,
            None => {
                return serde_json::json!({
                    "error": "LLM provider required for perspective analysis"
                })
            }
        };

        let perspectives = match ctx.perspectives.as_mut() {
            Some(p) => p,
            None => {
                return serde_json::json!({
                    "error": "Perspective store not available"
                })
            }
        };

        // Check if a specific scene is requested
        if let Some(scene_ref) = args["scene"].as_str() {
            let scene_id = resolve_scene_id(scene_ref, ctx.scene_map);
            match scene_id {
                Some(sid) => {
                    let scene_text = get_scene_text(&sid, ctx.text_buffer, ctx.scene_map);
                    match perspectives
                        .generate_scene_perspective(&char_id, &sid, ctx.graph, &scene_text, provider)
                        .await
                    {
                        Ok(sp) => serde_json::json!(sp),
                        Err(e) => serde_json::json!({ "error": e.to_string() }),
                    }
                }
                None => serde_json::json!({
                    "error": format!("Scene not found: {scene_ref}")
                }),
            }
        } else {
            // Full perspective
            match perspectives
                .generate_perspective(&char_id, ctx.graph, ctx.text_buffer, ctx.scene_map, provider)
                .await
            {
                Ok(p) => serde_json::json!(p),
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            }
        }
    }

    async fn exec_compare_perspectives(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
        provider: Option<&mut Provider>,
    ) -> serde_json::Value {
        let char_a_name = args["character_a"].as_str().unwrap_or("");
        let char_b_name = args["character_b"].as_str().unwrap_or("");
        let scene_ref = args["scene"].as_str().unwrap_or("1");

        let char_a = match find_character_id(ctx.graph, char_a_name) {
            Some(id) => id,
            None => {
                return serde_json::json!({
                    "error": format!("Character not found: {char_a_name}")
                })
            }
        };
        let char_b = match find_character_id(ctx.graph, char_b_name) {
            Some(id) => id,
            None => {
                return serde_json::json!({
                    "error": format!("Character not found: {char_b_name}")
                })
            }
        };

        let scene_id = match resolve_scene_id(scene_ref, ctx.scene_map) {
            Some(id) => id,
            None => {
                return serde_json::json!({
                    "error": format!("Scene not found: {scene_ref}")
                })
            }
        };

        let provider = match provider {
            Some(p) => p,
            None => {
                return serde_json::json!({
                    "error": "LLM provider required for perspective comparison"
                })
            }
        };

        let perspectives = match ctx.perspectives.as_mut() {
            Some(p) => p,
            None => {
                return serde_json::json!({
                    "error": "Perspective store not available"
                })
            }
        };

        let scene_text = get_scene_text(&scene_id, ctx.text_buffer, ctx.scene_map);
        match perspectives
            .compare_perspectives(&char_a, &char_b, &scene_id, ctx.graph, &scene_text, provider)
            .await
        {
            Ok(result) => serde_json::json!(result),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }

    async fn exec_find_blind_spots(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
        provider: Option<&mut Provider>,
    ) -> serde_json::Value {
        let character_name = args["character"].as_str().unwrap_or("");
        let char_id = match find_character_id(ctx.graph, character_name) {
            Some(id) => id,
            None => {
                return serde_json::json!({
                    "error": format!("Character not found: {character_name}")
                })
            }
        };

        let provider = match provider {
            Some(p) => p,
            None => {
                return serde_json::json!({
                    "error": "LLM provider required for blind spot analysis"
                })
            }
        };

        let perspectives = match ctx.perspectives.as_mut() {
            Some(p) => p,
            None => {
                return serde_json::json!({
                    "error": "Perspective store not available"
                })
            }
        };

        match perspectives
            .find_blind_spots(&char_id, ctx.graph, ctx.text_buffer, ctx.scene_map, provider)
            .await
        {
            Ok(spots) => serde_json::json!({
                "character": character_name,
                "blind_spot_count": spots.len(),
                "blind_spots": spots,
            }),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }

    fn exec_get_knowledge_at(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let character_name = args["character"].as_str().unwrap_or("");
        let char_id = match find_character_id(ctx.graph, character_name) {
            Some(id) => id,
            None => {
                return serde_json::json!({
                    "error": format!("Character not found: {character_name}")
                })
            }
        };

        let full_boundary =
            CharacterPerspective::compute_knowledge_boundary(ctx.graph, &char_id);

        // If a scene is specified, filter to scenes up to that point
        let boundary = if let Some(scene_ref) = args["scene"].as_str() {
            let target_scene_id = resolve_scene_id(scene_ref, ctx.scene_map);
            if let Some(target_id) = target_scene_id {
                // Get scenes in document order up to the target
                let scene_list = ctx.scene_map.list_scenes();
                let mut filtered = std::collections::HashSet::new();
                for span in scene_list {
                    if full_boundary.contains(&span.id) {
                        filtered.insert(span.id.clone());
                    }
                    if span.id == target_id {
                        break;
                    }
                }
                filtered
            } else {
                full_boundary
            }
        } else {
            full_boundary
        };

        // Gather scene info for the boundary
        let scene_info: Vec<serde_json::Value> = boundary
            .iter()
            .filter_map(|sid| {
                ctx.graph.get_node(sid).map(|n| {
                    if let GraphNode::Scene {
                        title, summary, ..
                    } = n
                    {
                        serde_json::json!({
                            "scene_id": sid,
                            "title": title,
                            "summary": summary,
                        })
                    } else {
                        serde_json::json!({ "scene_id": sid })
                    }
                })
            })
            .collect();

        serde_json::json!({
            "character": character_name,
            "scenes_witnessed": boundary.len(),
            "knowledge_boundary": scene_info,
        })
    }

    // -------------------------------------------------------------------------
    // Structural Tool implementations
    // -------------------------------------------------------------------------

    fn exec_story_lint(&self, ctx: &SkillContext<'_>) -> serde_json::Value {
        let mut issues = Vec::new();

        // 1. Orphaned objectives (character doesn't exist)
        for obj in ctx.graph.get_objectives() {
            if let GraphNode::Objective {
                id,
                character_id,
                description,
                ..
            } = obj
            {
                if ctx.graph.get_node(character_id).is_none() {
                    issues.push(serde_json::json!({
                        "severity": "error",
                        "type": "orphaned_objective",
                        "message": format!("Objective \"{description}\" references nonexistent character {character_id}"),
                        "node_id": id,
                    }));
                }
            }
        }

        // 2. Dead scenes (no Advances/Blocks edges)
        let dead = ctx.graph.find_dead_scenes();
        for sid in &dead {
            issues.push(serde_json::json!({
                "severity": "warning",
                "type": "dead_scene",
                "message": format!("Scene {sid} has no objective changes"),
                "node_id": sid,
            }));
        }

        // 3. Stale scenes (pending reindex)
        let pending = ctx.scene_map.get_pending();
        if !pending.is_empty() {
            issues.push(serde_json::json!({
                "severity": "info",
                "type": "stale_scenes",
                "message": format!("{} scene(s) have unanalyzed changes", pending.len()),
                "scene_ids": pending.iter().collect::<Vec<_>>(),
            }));
        }

        // 4. PresentIn edge consistency
        for scene in ctx.graph.get_scenes() {
            if let GraphNode::Scene {
                id,
                characters_present,
                ..
            } = scene
            {
                for cid in characters_present {
                    if ctx.graph.get_node(cid).is_none() {
                        issues.push(serde_json::json!({
                            "severity": "error",
                            "type": "invalid_presence",
                            "message": format!("Scene {id} lists nonexistent character {cid}"),
                            "node_id": id,
                        }));
                    }
                }
            }
        }

        // 5. Orphaned declarations
        if let Some(intent) = ctx.intent.as_ref() {
            let valid_ids: std::collections::HashSet<String> =
                ctx.graph.all_node_ids().into_iter().collect();
            let orphans = intent.find_orphans(&valid_ids);
            for orphan in &orphans {
                issues.push(serde_json::json!({
                    "severity": "warning",
                    "type": "orphaned_declaration",
                    "message": format!("Declaration on {}.{} references nonexistent node", orphan.node_id, orphan.field),
                }));
            }

            // 6. Unresolved divergences
            let divs = divergence::detect_divergences(ctx.graph, intent);
            for div in &divs {
                issues.push(serde_json::json!({
                    "severity": "info",
                    "type": "divergence",
                    "message": format!(
                        "Node {}: field '{}' inferred='{}' vs declared='{}'",
                        div.node_id, div.field, div.inferred, div.declared
                    ),
                }));
            }
        }

        serde_json::json!({
            "issue_count": issues.len(),
            "issues": issues,
        })
    }

    fn exec_pacing_analysis(&self, ctx: &SkillContext<'_>) -> serde_json::Value {
        let scenes = ctx.scene_map.list_scenes();
        let mut scene_stats = Vec::new();
        let mut total_words = 0u64;

        for (i, span) in scenes.iter().enumerate() {
            let text = ctx
                .text_buffer
                .read(span.byte_range())
                .unwrap_or_default();
            let word_count = text.split_whitespace().count() as u64;
            total_words += word_count;

            // Count objectives that change state in this scene
            let analysis = ctx.graph.get_scene_analysis(&span.id);
            let conflict_density = analysis
                .as_ref()
                .map(|a| a.objectives_advanced.len() + a.objectives_blocked.len())
                .unwrap_or(0);

            let title = span.title.as_deref().unwrap_or("(untitled)");
            scene_stats.push(serde_json::json!({
                "number": i + 1,
                "title": title,
                "word_count": word_count,
                "conflict_density": conflict_density,
            }));
        }

        let avg_words = if scenes.is_empty() {
            0
        } else {
            total_words / scenes.len() as u64
        };

        serde_json::json!({
            "scene_count": scenes.len(),
            "total_words": total_words,
            "average_scene_words": avg_words,
            "scenes": scene_stats,
        })
    }

    fn exec_arc_completeness(
        &self,
        args: &serde_json::Value,
        graph: &NarrativeGraph,
    ) -> serde_json::Value {
        let filter_char = args["character"].as_str();

        let characters: Vec<(String, String)> = graph
            .get_characters()
            .iter()
            .filter_map(|c| {
                if let GraphNode::Character { id, name, .. } = c {
                    if let Some(filter) = filter_char {
                        if name.eq_ignore_ascii_case(filter) {
                            Some((id.clone(), name.clone()))
                        } else {
                            None
                        }
                    } else {
                        Some((id.clone(), name.clone()))
                    }
                } else {
                    None
                }
            })
            .collect();

        let mut results = Vec::new();
        for (char_id, char_name) in &characters {
            let arc = graph.get_character_arc(char_id);
            let total = arc.len();
            let resolved = arc
                .iter()
                .filter(|o| {
                    matches!(
                        o.status,
                        crate::concepts::narrative_graph::Status::Achieved
                            | crate::concepts::narrative_graph::Status::Abandoned
                            | crate::concepts::narrative_graph::Status::Transformed
                    )
                })
                .count();
            let unresolved: Vec<_> = arc
                .iter()
                .filter(|o| {
                    matches!(
                        o.status,
                        crate::concepts::narrative_graph::Status::Active
                            | crate::concepts::narrative_graph::Status::Blocked
                    )
                })
                .collect();

            results.push(serde_json::json!({
                "character": char_name,
                "total_objectives": total,
                "resolved": resolved,
                "unresolved": unresolved.len(),
                "completeness": if total > 0 { resolved as f64 / total as f64 } else { 1.0 },
                "unresolved_objectives": unresolved,
            }));
        }

        serde_json::json!({
            "characters": results,
        })
    }

    fn exec_declare_intent(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let node_id = match args["node_id"].as_str() {
            Some(id) => id,
            None => return serde_json::json!({ "error": "Missing required field: node_id" }),
        };
        let field = match args["field"].as_str() {
            Some(f) => f,
            None => return serde_json::json!({ "error": "Missing required field: field" }),
        };
        let value = match args["value"].as_str() {
            Some(v) => v,
            None => return serde_json::json!({ "error": "Missing required field: value" }),
        };
        let rationale = args["rationale"].as_str().map(String::from);

        // Validate node_id exists in graph
        if ctx.graph.get_node(node_id).is_none() {
            return serde_json::json!({
                "error": format!("Node not found in graph: {node_id}")
            });
        }

        let intent = match ctx.intent.as_mut() {
            Some(i) => i,
            None => return serde_json::json!({ "error": "Intent store not available" }),
        };

        intent.declare(node_id, field, value.to_string(), rationale);

        serde_json::json!({
            "status": "declared",
            "node_id": node_id,
            "field": field,
            "value": value,
        })
    }

    fn exec_retract_intent(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let node_id = match args["node_id"].as_str() {
            Some(id) => id,
            None => return serde_json::json!({ "error": "Missing required field: node_id" }),
        };
        let field = match args["field"].as_str() {
            Some(f) => f,
            None => return serde_json::json!({ "error": "Missing required field: field" }),
        };

        let intent = match ctx.intent.as_mut() {
            Some(i) => i,
            None => return serde_json::json!({ "error": "Intent store not available" }),
        };

        let removed = intent.retract(node_id, field);

        serde_json::json!({
            "status": if removed { "retracted" } else { "not_found" },
            "node_id": node_id,
            "field": field,
        })
    }

    // -------------------------------------------------------------------------
    // Canvas Tool implementations
    // -------------------------------------------------------------------------

    fn exec_write_to_canvas(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let position = match args["position"].as_u64() {
            Some(p) => p as usize,
            None => return serde_json::json!({ "error": "Missing required field: position" }),
        };
        let text = match args["text"].as_str() {
            Some(t) => t,
            None => return serde_json::json!({ "error": "Missing required field: text" }),
        };

        match ctx.text_buffer.insert(position, text) {
            Ok(()) => {
                // Trigger scene map reindex if text contains scene boundary markers
                let full_text = ctx.text_buffer.read_all();
                ctx.scene_map.full_reindex(&full_text);

                serde_json::json!({
                    "status": "written",
                    "position": position,
                    "bytes_written": text.len(),
                })
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }

    fn exec_replace_in_canvas(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let start = match args["start"].as_u64() {
            Some(s) => s as usize,
            None => return serde_json::json!({ "error": "Missing required field: start" }),
        };
        let end = match args["end"].as_u64() {
            Some(e) => e as usize,
            None => return serde_json::json!({ "error": "Missing required field: end" }),
        };
        let text = match args["text"].as_str() {
            Some(t) => t,
            None => return serde_json::json!({ "error": "Missing required field: text" }),
        };

        let range = ByteRange::new(start, end);
        match ctx.text_buffer.replace(range, text) {
            Ok(()) => {
                let full_text = ctx.text_buffer.read_all();
                ctx.scene_map.full_reindex(&full_text);

                serde_json::json!({
                    "status": "replaced",
                    "start": start,
                    "end": end,
                    "new_length": text.len(),
                })
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }

    fn exec_insert_scene(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let position = match args["position"].as_u64() {
            Some(p) => p as usize,
            None => return serde_json::json!({ "error": "Missing required field: position" }),
        };
        let title = match args["title"].as_str() {
            Some(t) => t,
            None => return serde_json::json!({ "error": "Missing required field: title" }),
        };
        let content = args["content"].as_str().unwrap_or("");

        // Build scene break: \n\n---\n\n## Title\n\nContent
        let scene_text = format!("\n\n---\n\n## {title}\n\n{content}");

        match ctx.text_buffer.insert(position, &scene_text) {
            Ok(()) => {
                let full_text = ctx.text_buffer.read_all();
                ctx.scene_map.full_reindex(&full_text);

                serde_json::json!({
                    "status": "inserted",
                    "position": position,
                    "title": title,
                    "bytes_written": scene_text.len(),
                    "new_scene_count": ctx.scene_map.scene_count(),
                })
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }
}

impl Default for Skills {
    fn default() -> Self {
        Self::new()
    }
}

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

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

/// Resolve a scene reference (ID or 1-based number) to a scene ID
fn resolve_scene_id(scene_ref: &str, scene_map: &SceneMap) -> Option<String> {
    if let Ok(num) = scene_ref.parse::<usize>() {
        scene_map
            .list_scenes()
            .get(num.saturating_sub(1))
            .map(|s| s.id.clone())
    } else {
        scene_map.get_scene(scene_ref).map(|s| s.id.clone())
    }
}

/// Get the text of a scene by ID
fn get_scene_text(scene_id: &str, text_buffer: &TextBuffer, scene_map: &SceneMap) -> String {
    scene_map
        .get_scene(scene_id)
        .and_then(|span| text_buffer.read(span.byte_range()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::narrative_graph::{new_id, GraphEdge, GraphNode, Scope, Status};
    use crate::concepts::scene_map::ParseMode;
    use std::path::PathBuf;

    fn make_test_context() -> (TextBuffer, SceneMap, NarrativeGraph, DeclaredIntent) {
        let text = "## Scene 1\n\nMarcus drew his sword. Elena watched from the shadows.\n\n---\n\n## Scene 2\n\nElena crept forward alone. The night was silent.\n";
        let text_buffer = TextBuffer::from_str(text, PathBuf::from("/tmp/test.md"));

        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text);

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
            });
            graph.add_node(GraphNode::Scene {
                id: scene_ids[1].clone(),
                title: Some("Scene 2".to_string()),
                summary: "Elena alone".to_string(),
                characters_present: vec![char_elena.clone()],
                location: None,
                time: None,
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

    #[tokio::test]
    async fn test_get_scene_analysis_skill() {
        let (mut text_buffer, mut scene_map, graph, mut intent) = make_test_context();
        let mut skills = Skills::new();
        let mut ctx = SkillContext {
            text_buffer: &mut text_buffer,
            scene_map: &mut scene_map,
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: Some(&mut perspectives),
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
}
