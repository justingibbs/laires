use super::{
    Permission, SkillCategory, SkillDefinition, SkillSetContext, Skills,
};

impl Skills {
    pub(super) fn register_defaults(&mut self) {
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
            description: "Insert text at a byte position in the active story file or a specified manifest story file.".to_string(),
            category: SkillCategory::CanvasTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "position": { "type": "integer", "description": "Byte offset to insert text at" },
                    "text": { "type": "string", "description": "Text to insert" },
                    "file": { "type": "string", "description": "Optional manifest story file path. Required when the manifest has multiple story files." }
                },
                "required": ["position", "text"]
            }),
        });

        self.register(SkillDefinition {
            name: "replace_in_canvas".to_string(),
            description: "Replace text in a byte range in the active story file or a specified manifest story file.".to_string(),
            category: SkillCategory::CanvasTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "start": { "type": "integer", "description": "Start byte offset" },
                    "end": { "type": "integer", "description": "End byte offset" },
                    "text": { "type": "string", "description": "Replacement text" },
                    "file": { "type": "string", "description": "Optional manifest story file path. Required when the manifest has multiple story files." }
                },
                "required": ["start", "end", "text"]
            }),
        });

        self.register(SkillDefinition {
            name: "insert_scene".to_string(),
            description: "Insert a new scene break and content into the active story file, a specified manifest story file, or after an existing scene.".to_string(),
            category: SkillCategory::CanvasTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "position": { "type": "integer", "description": "Byte offset to insert the scene break. Optional when after_scene is provided." },
                    "after_scene": { "type": "string", "description": "Optional scene ID or 1-based scene number. Inserts immediately after that scene." },
                    "title": { "type": "string", "description": "Scene heading/title" },
                    "content": { "type": "string", "description": "Initial scene content" },
                    "file": { "type": "string", "description": "Optional manifest story file path. Required when the manifest has multiple story files unless after_scene is provided." }
                },
                "required": ["title"],
                "oneOf": [
                    { "required": ["position"] },
                    { "required": ["after_scene"] }
                ]
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

        // Brief Tools (Consultant mode)
        self.register(SkillDefinition {
            name: "add_to_brief".to_string(),
            description: "Record a specific revision suggestion in the revision brief. Use this in Consultant mode to build up a structured set of actionable revision notes for the writer.".to_string(),
            category: SkillCategory::BriefTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scene_title": { "type": "string", "description": "Title of the scene this revision applies to" },
                    "file": { "type": "string", "description": "File path this revision applies to" },
                    "priority": { "type": "string", "enum": ["high", "medium", "low"], "description": "Revision priority. Default: medium" },
                    "current_state": { "type": "string", "description": "Brief excerpt of the current text being revised" },
                    "issue": { "type": "string", "description": "What the analysis found — the problem or opportunity" },
                    "suggestion": { "type": "string", "description": "Specific, actionable instruction for the revision" },
                    "draft": { "type": "string", "description": "Optional draft passage the writer can adapt" },
                    "graph_impact": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "How this revision affects the narrative graph (e.g. 'New conflict edge: Sarah <-> Marcus')"
                    }
                },
                "required": ["issue", "suggestion"]
            }),
        });

        self.register(SkillDefinition {
            name: "generate_brief".to_string(),
            description: "Finalize and export the revision brief as a Markdown document. Call this after adding revisions with add_to_brief.".to_string(),
            category: SkillCategory::BriefTools,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "focus": { "type": "string", "description": "What this brief addresses (e.g. 'Character arc for Sarah')" },
                    "scope": { "type": "string", "description": "Scene range or 'full manuscript'" },
                    "overview": { "type": "string", "description": "2-3 sentence summary of the revision brief" },
                    "structural_notes": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "category": { "type": "string", "description": "Category (e.g. Pacing, Timeline, Arc completeness)" },
                                "note": { "type": "string", "description": "The structural observation" }
                            },
                            "required": ["category", "note"]
                        },
                        "description": "Broader structural observations that don't map to a single scene"
                    }
                },
                "required": ["focus", "overview"]
            }),
        });
    }

    pub fn register(&mut self, skill: SkillDefinition) {
        let name = skill.name.clone();
        self.registry.insert(name.clone(), skill);
        self.permissions.insert(name, Permission::Enabled);
    }

    #[cfg_attr(not(test), allow(dead_code))]
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

    pub fn tool_schemas_for_context(
        &self,
        context: SkillSetContext,
    ) -> Vec<crate::concepts::provider::ToolSchema> {
        // Determine which categories are allowed (None = all)
        // and which are explicitly excluded.
        let (allowed_categories, excluded_categories): (
            Option<Vec<SkillCategory>>,
            Vec<SkillCategory>,
        ) = match context {
            SkillSetContext::Chat | SkillSetContext::Workshop => (None, vec![]),
            SkillSetContext::Analysis => (Some(vec![]), vec![]),
            SkillSetContext::Perspective => (
                Some(vec![
                    SkillCategory::PerspectiveTools,
                    SkillCategory::GraphTools,
                ]),
                vec![],
            ),
            // Consultant: everything EXCEPT canvas write tools
            SkillSetContext::Consultant => (None, vec![SkillCategory::CanvasTools]),
        };

        self.registry
            .values()
            .filter(|s| self.is_permitted(&s.name))
            .filter(|s| match &allowed_categories {
                None => true,
                Some(cats) => cats.contains(&s.category),
            })
            .filter(|s| !excluded_categories.contains(&s.category))
            .map(|s| crate::concepts::provider::ToolSchema {
                name: s.name.clone(),
                description: s.description.clone(),
                parameters: s.input_schema.clone(),
            })
            .collect()
    }

}
