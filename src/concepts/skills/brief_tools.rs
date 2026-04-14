use crate::concepts::revision_brief::{Priority, Revision};
use crate::concepts::skills::{SkillContext, Skills};
use crate::runtime::story_access::StoryAccess;

impl Skills {
    /// Add a revision suggestion to the active brief.
    ///
    /// The agent calls this during Consultant mode conversation to record
    /// specific, scene-anchored revision suggestions.
    pub(super) fn exec_add_to_brief(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let brief = match ctx.revision_brief.as_mut() {
            Some(b) => b,
            None => {
                return serde_json::json!({
                    "error": "No active revision brief. Brief tools are only available in Consultant mode."
                });
            }
        };

        let scene_title = args["scene_title"].as_str().unwrap_or("").to_string();
        let file = args["file"].as_str().unwrap_or("").to_string();
        let priority = args["priority"]
            .as_str()
            .map(Priority::from_str_lossy)
            .unwrap_or(Priority::Medium);
        let current_state = args["current_state"].as_str().unwrap_or("").to_string();
        let issue = args["issue"].as_str().unwrap_or("").to_string();
        let suggestion = args["suggestion"].as_str().unwrap_or("").to_string();
        let draft = args["draft"].as_str().map(|s| s.to_string());
        let graph_impact = args["graph_impact"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        if issue.is_empty() && suggestion.is_empty() {
            return serde_json::json!({
                "error": "At least one of 'issue' or 'suggestion' must be provided."
            });
        }

        // Try to resolve scene line numbers from StoryAccess
        let story = StoryAccess::new(
            ctx.text_buffer,
            ctx.scene_map,
            ctx.file_buffer_manager.as_deref(),
        );
        let (scene_id, line_start, line_end) = resolve_scene_location(&story, &scene_title, &file);

        let revision = Revision {
            scene_id,
            scene_title,
            file_path: file,
            line_start,
            line_end,
            priority,
            current_state,
            issue,
            suggestion,
            draft_passage: draft,
            graph_impact,
        };

        brief.add_revision(revision);

        serde_json::json!({
            "status": "ok",
            "revision_count": brief.revision_count(),
            "message": format!("Revision #{} added to brief.", brief.revision_count())
        })
    }

    /// Finalize the brief with metadata and export it.
    pub(super) fn exec_generate_brief(
        &self,
        args: &serde_json::Value,
        ctx: &mut SkillContext<'_>,
    ) -> serde_json::Value {
        let brief = match ctx.revision_brief.as_mut() {
            Some(b) => b,
            None => {
                return serde_json::json!({
                    "error": "No active revision brief."
                });
            }
        };

        if brief.is_empty() {
            return serde_json::json!({
                "error": "Brief is empty. Use add_to_brief to add revision suggestions first."
            });
        }

        // Set metadata
        if let Some(focus) = args["focus"].as_str() {
            brief.focus = focus.to_string();
        }
        if let Some(scope) = args["scope"].as_str() {
            brief.scope = scope.to_string();
        }
        if let Some(overview) = args["overview"].as_str() {
            brief.overview = overview.to_string();
        }

        // Add structural notes if provided
        if let Some(notes) = args["structural_notes"].as_array() {
            for note in notes {
                let category = note["category"].as_str().unwrap_or("General");
                let text = note["note"].as_str().unwrap_or("");
                if !text.is_empty() {
                    brief.add_structural_note(category, text);
                }
            }
        }

        // Save to project briefs directory
        let save_result = if let Some(root) = ctx.project_root {
            match brief.save_to_project(root) {
                Ok(path) => Some(path.to_string_lossy().to_string()),
                Err(e) => {
                    return serde_json::json!({
                        "error": format!("Failed to save brief: {e}")
                    });
                }
            }
        } else {
            None
        };

        let markdown = brief.to_markdown();

        serde_json::json!({
            "status": "ok",
            "revision_count": brief.revision_count(),
            "structural_note_count": brief.structural_notes.len(),
            "saved_to": save_result,
            "markdown": markdown,
        })
    }
}

/// Try to resolve scene line numbers by matching title against known scenes.
fn resolve_scene_location(
    story: &StoryAccess<'_>,
    scene_title: &str,
    _file_path: &str,
) -> (Option<String>, usize, usize) {
    if scene_title.is_empty() {
        return (None, 0, 0);
    }

    let title_lower = scene_title.to_lowercase();

    for summary in &story.list_scenes() {
        let span_title = summary.title.as_deref().unwrap_or("").to_lowercase();

        if !span_title.is_empty() && span_title.contains(&title_lower) {
            // Read the scene to get text, then estimate line numbers
            if story.read_scene_by_id(&summary.id).is_some() {
                return (Some(summary.id.clone()), 0, 0);
            }
        }
    }

    (None, 0, 0)
}
