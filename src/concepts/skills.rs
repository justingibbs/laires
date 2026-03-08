use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use std::io::Write;
use std::path::Path;

use crate::concepts::canvas::Canvas;
use crate::concepts::character_perspective::CharacterPerspective;
use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::manifest::Manifest;
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
    CustomTools,
}

/// Context for selecting which tool schemas to include in LLM requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSetContext {
    /// Chat mode: all tools available.
    Chat,
    /// Analysis mode: no tools needed (LLM generates structured output).
    Analysis,
    /// Perspective mode: only PerspectiveTools and GraphTools.
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
    ConditionalOn(String),
}

/// Data specific to custom (LLM-powered) skills loaded from TOML files.
#[derive(Debug, Clone)]
pub struct CustomSkillData {
    pub prompt_template: String,
    pub output_schema: Option<serde_json::Value>,
}

/// TOML file format for custom skill definitions.
#[derive(Debug, Deserialize)]
struct CustomSkillToml {
    name: String,
    description: String,
    prompt_template: String,
    input_schema: toml::Value,
    #[serde(default)]
    output_schema: Option<toml::Value>,
}

/// Context passed to skill invocations, grouping all available state.
pub struct SkillContext<'a> {
    pub text_buffer: &'a mut TextBuffer,
    pub scene_map: &'a mut SceneMap,
    pub graph: &'a NarrativeGraph,
    pub intent: Option<&'a mut DeclaredIntent>,
    pub perspectives: Option<&'a mut CharacterPerspective>,
    pub canvas: Option<&'a mut Canvas>,
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

    fn register_defaults(&mut self) {
        // File Tools
        self.register(SkillDefinition {
            name: "story_grep".to_string(),
            description: "Search story text by regex pattern.".to_string(),
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
            description: "Read a scene's full text by ID or number.".to_string(),
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
            description: "List all scenes with titles and word counts.".to_string(),
            category: SkillCategory::FileTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(SkillDefinition {
            name: "story_stats".to_string(),
            description: "Get manuscript statistics: word, scene, and character counts.".to_string(),
            category: SkillCategory::FileTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(SkillDefinition {
            name: "read_context_file".to_string(),
            description: "Read a context file (outline, characters, notes) by path or role.".to_string(),
            category: SkillCategory::FileTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "File path or role (e.g. 'outline', 'characters', 'notes', or a specific path like 'outline.md')" }
                },
                "required": ["file"]
            }),
        });

        self.register(SkillDefinition {
            name: "list_files".to_string(),
            description: "List all project files with roles and metadata.".to_string(),
            category: SkillCategory::FileTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        // Graph Tools
        self.register(SkillDefinition {
            name: "query_graph".to_string(),
            description: "Query the narrative graph by node type or character.".to_string(),
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
            description: "Get a character's objective trajectory across scenes.".to_string(),
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
            description: "Get objective conflicts, optionally filtered by character.".to_string(),
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
            description: "Find scenes with no objective progress.".to_string(),
            category: SkillCategory::GraphTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        // New Graph Tools
        self.register(SkillDefinition {
            name: "get_scene_analysis".to_string(),
            description: "Get objective activity for a scene.".to_string(),
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
            description: "Get mismatches between inferred and declared values.".to_string(),
            category: SkillCategory::GraphTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        // Perspective Tools
        self.register(SkillDefinition {
            name: "interpret_as_character".to_string(),
            description: "Analyze the story from a character's perspective.".to_string(),
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
            description: "Compare two characters' perspectives on a scene.".to_string(),
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
            description: "Find dramatic irony where a character lacks reader knowledge.".to_string(),
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
            description: "Get a character's knowledge at a story point.".to_string(),
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
            description: "Run consistency checks on the narrative graph.".to_string(),
            category: SkillCategory::StructuralTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(SkillDefinition {
            name: "pacing_analysis".to_string(),
            description: "Analyze pacing: scene lengths and conflict density.".to_string(),
            category: SkillCategory::StructuralTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(SkillDefinition {
            name: "arc_completeness".to_string(),
            description: "Check if character objectives resolve by story end.".to_string(),
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
            description: "Insert text at a byte position.".to_string(),
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
            description: "Replace text in a byte range.".to_string(),
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
            description: "Insert a new scene break and content.".to_string(),
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
            name: "scan_story".to_string(),
            description: "Scan and analyze all scenes to populate the narrative graph.".to_string(),
            category: SkillCategory::GraphTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "full": {
                        "type": "boolean",
                        "description": "If true, re-classify all files (like --full flag). Default false."
                    }
                }
            }),
        });

        self.register(SkillDefinition {
            name: "declare_intent".to_string(),
            description: "Declare a writer override on a graph node field.".to_string(),
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
            description: "Remove a writer override from a graph node field.".to_string(),
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
            "story_grep" => self.exec_story_grep(args, ctx.text_buffer),
            "read_scene" => self.exec_read_scene(args, ctx.text_buffer, ctx.scene_map),
            "list_scenes" => self.exec_list_scenes(ctx.text_buffer, ctx.scene_map),
            "story_stats" => self.exec_story_stats(ctx.text_buffer, ctx.scene_map, ctx.graph),
            "read_context_file" => self.exec_read_context_file(args, ctx.manifest, ctx.project_root),
            "list_files" => self.exec_list_files(ctx.manifest),

            // Graph Tools
            "query_graph" => self.exec_query_graph(args, ctx.graph),
            "get_character_arc" => self.exec_get_character_arc(args, ctx.graph),
            "get_conflicts" => self.exec_get_conflicts(args, ctx.graph),
            "find_dead_scenes" => self.exec_find_dead_scenes(ctx.graph),
            "get_scene_analysis" => self.exec_get_scene_analysis(args, ctx.graph, ctx.scene_map),
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

    /// Get tool schemas for LLM tool-use
    pub fn tool_schemas(&self) -> Vec<crate::concepts::provider::ToolSchema> {
        self.registry
            .values()
            .filter(|s| self.is_permitted(&s.name))
            .map(|s| crate::concepts::provider::ToolSchema {
                name: s.name.clone(),
                description: s.description.clone(),
                parameters: s.input_schema.clone(),
            })
            .collect()
    }

    /// Get tool schemas filtered by context. Different operations need different tools.
    pub fn tool_schemas_for_context(
        &self,
        context: SkillSetContext,
    ) -> Vec<crate::concepts::provider::ToolSchema> {
        let allowed_categories: Option<Vec<SkillCategory>> = match context {
            SkillSetContext::Chat => None, // all tools
            SkillSetContext::Analysis => Some(vec![]), // no tools
            SkillSetContext::Perspective => Some(vec![
                SkillCategory::PerspectiveTools,
                SkillCategory::GraphTools,
            ]),
        };

        self.registry
            .values()
            .filter(|s| self.is_permitted(&s.name))
            .filter(|s| match &allowed_categories {
                None => true,
                Some(cats) => cats.contains(&s.category),
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
            .filter(|s| self.is_permitted(&s.name))
            .collect()
    }

    pub fn set_permission(&mut self, skill_name: &str, permission: Permission) {
        if self.registry.contains_key(skill_name) {
            self.permissions
                .insert(skill_name.to_string(), permission);
        }
    }

    /// Check if a skill is permitted based on its permission and provider locality.
    pub fn is_permitted(&self, skill_name: &str) -> bool {
        match self.permissions.get(skill_name) {
            Some(Permission::Enabled) => true,
            Some(Permission::Disabled) => false,
            Some(Permission::ConditionalOn(cond)) => match cond.as_str() {
                "is_local" => self.provider_is_local,
                "is_cloud" => !self.provider_is_local,
                _ => false,
            },
            None => false,
        }
    }

    /// Update provider locality flag for conditional permission checks.
    pub fn set_provider_locality(&mut self, is_local: bool) {
        self.provider_is_local = is_local;
    }

    /// Load custom skill definitions from TOML files in the given directory.
    /// Returns a list of validation errors for malformed definitions (which are skipped).
    pub fn load_custom_skills(&mut self, skills_dir: &Path) -> Vec<String> {
        let mut errors = Vec::new();

        if !skills_dir.is_dir() {
            return errors;
        }

        let entries = match std::fs::read_dir(skills_dir) {
            Ok(e) => e,
            Err(e) => {
                errors.push(format!("Failed to read skills directory: {e}"));
                return errors;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(format!("Failed to read directory entry: {e}"));
                    continue;
                }
            };

            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(format!("{}: failed to read: {e}", path.display()));
                    continue;
                }
            };

            let skill_toml: CustomSkillToml = match toml::from_str(&content) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(format!("{}: invalid TOML: {e}", path.display()));
                    continue;
                }
            };

            // Validate
            if skill_toml.name.is_empty() {
                errors.push(format!("{}: name cannot be empty", path.display()));
                continue;
            }
            if skill_toml.prompt_template.is_empty() {
                errors.push(format!(
                    "{}: prompt_template cannot be empty",
                    path.display()
                ));
                continue;
            }

            // Convert schemas from TOML to JSON
            let input_schema = toml_to_json(skill_toml.input_schema);
            let output_schema = skill_toml.output_schema.map(toml_to_json);

            // Register the skill definition
            self.register(SkillDefinition {
                name: skill_toml.name.clone(),
                description: skill_toml.description,
                category: SkillCategory::CustomTools,
                input_schema,
            });

            // Store custom execution data
            self.custom_data.insert(
                skill_toml.name,
                CustomSkillData {
                    prompt_template: skill_toml.prompt_template,
                    output_schema,
                },
            );
        }

        errors
    }

    /// Execute a custom skill by rendering its prompt template and calling the provider.
    async fn exec_custom_skill(
        &self,
        skill_name: &str,
        args: &serde_json::Value,
        provider: Option<&mut Provider>,
    ) -> serde_json::Value {
        let data = match self.custom_data.get(skill_name) {
            Some(d) => d,
            None => {
                return serde_json::json!({
                    "error": format!("Custom skill data not found: {skill_name}")
                })
            }
        };

        let provider = match provider {
            Some(p) => p,
            None => {
                return serde_json::json!({
                    "error": "LLM provider required for custom skills"
                })
            }
        };

        // Render prompt template by replacing {{key}} placeholders with arg values
        let mut prompt = data.prompt_template.clone();
        if let Some(obj) = args.as_object() {
            for (key, value) in obj {
                let placeholder = format!("{{{{{key}}}}}");
                let replacement = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                prompt = prompt.replace(&placeholder, &replacement);
            }
        }

        // Call the LLM provider
        let messages = vec![crate::concepts::provider::Message {
            role: crate::concepts::provider::Role::User,
            content: prompt,
            tool_calls: None,
            tool_results: None,
        }];

        match provider.complete(&messages, &[], None).await {
            Ok(response) => {
                let content = response.content.unwrap_or_default();
                // Try to parse as JSON, fall back to wrapping in a response object
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(json) => json,
                    Err(_) => serde_json::json!({ "response": content }),
                }
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }

    /// Append an execution record to the audit trail file (.laires/skill_log.jsonl).
    fn write_audit_log(
        &self,
        project_root: &Path,
        skill_name: &str,
        args: &serde_json::Value,
        result: &serde_json::Value,
        duration_ms: u64,
    ) {
        let log_path = project_root
            .join(crate::config::LAIRES_DIR)
            .join(crate::config::SKILL_LOG_FILE);
        let entry = serde_json::json!({
            "timestamp": Utc::now().to_rfc3339(),
            "skill": skill_name,
            "args": args,
            "result": result,
            "duration_ms": duration_ms,
        });
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = writeln!(file, "{}", entry);
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

    fn exec_read_context_file(
        &self,
        args: &serde_json::Value,
        manifest: Option<&Manifest>,
        project_root: Option<&Path>,
    ) -> serde_json::Value {
        let manifest = match manifest {
            Some(m) => m,
            None => return serde_json::json!({ "error": "No manifest loaded. Run `laires scan` first." }),
        };
        let project_root = match project_root {
            Some(r) => r,
            None => return serde_json::json!({ "error": "Project root not available" }),
        };

        let file_ref = args["file"].as_str().unwrap_or("");

        // Try to find by role first (e.g. "outline", "characters", "notes")
        let context_file = manifest
            .context_files
            .iter()
            .find(|cf| cf.role.to_string() == file_ref)
            .or_else(|| {
                // Then try by path (exact or suffix match)
                manifest.context_files.iter().find(|cf| {
                    cf.path == file_ref || cf.path.ends_with(file_ref)
                })
            });

        match context_file {
            Some(cf) => {
                let full_path = project_root.join(&cf.path);
                let ext = full_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");

                let content = if ext == "docx" {
                    match crate::concepts::docx::extract_text_from_docx(&full_path) {
                        Ok(text) => text,
                        Err(e) => {
                            return serde_json::json!({
                                "error": format!("Failed to read {}: {e}", cf.path)
                            })
                        }
                    }
                } else {
                    match std::fs::read_to_string(&full_path) {
                        Ok(text) => text,
                        Err(e) => {
                            return serde_json::json!({
                                "error": format!("Failed to read {}: {e}", cf.path)
                            })
                        }
                    }
                };

                serde_json::json!({
                    "path": cf.path,
                    "role": cf.role.to_string(),
                    "content": content,
                    "word_count": content.split_whitespace().count(),
                })
            }
            None => serde_json::json!({
                "error": format!("Context file not found: {file_ref}"),
                "available": manifest.context_files.iter().map(|cf| {
                    serde_json::json!({ "path": cf.path, "role": cf.role.to_string() })
                }).collect::<Vec<_>>(),
            }),
        }
    }

    fn exec_list_files(
        &self,
        manifest: Option<&Manifest>,
    ) -> serde_json::Value {
        let manifest = match manifest {
            Some(m) => m,
            None => return serde_json::json!({ "error": "No manifest loaded. Run `laires scan` first." }),
        };

        let story_files: Vec<serde_json::Value> = manifest
            .story_files
            .iter()
            .map(|sf| {
                serde_json::json!({
                    "path": sf.path,
                    "role": "story",
                    "format": sf.format,
                    "order": sf.order,
                })
            })
            .collect();

        let context_files: Vec<serde_json::Value> = manifest
            .context_files
            .iter()
            .map(|cf| {
                serde_json::json!({
                    "path": cf.path,
                    "role": cf.role.to_string(),
                })
            })
            .collect();

        let excluded: Vec<serde_json::Value> = manifest
            .excluded
            .iter()
            .map(|ef| {
                serde_json::json!({
                    "path": ef.path,
                    "reason": ef.reason,
                })
            })
            .collect();

        serde_json::json!({
            "story_file_count": story_files.len(),
            "context_file_count": context_files.len(),
            "excluded_count": excluded.len(),
            "story_files": story_files,
            "context_files": context_files,
            "excluded": excluded,
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
                .generate_perspective(&char_id, ctx.graph, provider)
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
            .find_blind_spots(&char_id, ctx.graph, provider)
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
                ctx.scene_map.full_reindex(&full_text, "");

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
                ctx.scene_map.full_reindex(&full_text, "");

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
                ctx.scene_map.full_reindex(&full_text, "");

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

/// Convert a TOML value to a serde_json value.
fn toml_to_json(val: toml::Value) -> serde_json::Value {
    match val {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(map) => {
            let obj: serde_json::Map<String, serde_json::Value> =
                map.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect();
            serde_json::Value::Object(obj)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: Some(&mut perspectives),
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
            graph: &graph,
            intent: Some(&mut intent),
            perspectives: None,
            canvas: None,
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
