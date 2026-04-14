use std::path::Path;

use crate::concepts::manifest::Manifest;
use crate::concepts::narrative_graph::NarrativeGraph;
use crate::runtime::story_access::StoryAccess;

use super::Skills;

impl Skills {
    pub(super) fn exec_story_grep(
        &self,
        args: &serde_json::Value,
        story: &StoryAccess<'_>,
    ) -> serde_json::Value {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let matches = match story.search(pattern) {
            Ok(hits) => hits,
            Err(e) => return serde_json::json!({ "error": format!("Invalid regex: {e}") }),
        };

        serde_json::json!({
            "pattern": pattern,
            "match_count": matches.len(),
            "matches": matches.into_iter().map(|hit| serde_json::json!({
                "file_path": hit.file_path,
                "line": hit.line_number,
                "byte_offset": hit.byte_offset,
                "text": hit.context,
            })).collect::<Vec<_>>(),
        })
    }

    pub(super) fn exec_read_scene(
        &self,
        args: &serde_json::Value,
        story: &StoryAccess<'_>,
    ) -> serde_json::Value {
        let scene_ref = args["scene"].as_str().unwrap_or("1");

        match story.read_scene(scene_ref) {
            Some(scene) => {
                serde_json::json!({
                    "number": scene.number,
                    "scene_id": scene.id,
                    "title": scene.title,
                    "file_path": scene.file_path,
                    "text": scene.text,
                    "word_count": scene.word_count,
                })
            }
            None => serde_json::json!({
                "error": format!("Scene not found: {scene_ref}")
            }),
        }
    }

    pub(super) fn exec_list_scenes(&self, story: &StoryAccess<'_>) -> serde_json::Value {
        let scenes: Vec<serde_json::Value> = story
            .list_scenes()
            .into_iter()
            .map(|scene| {
                serde_json::json!({
                    "number": scene.number,
                    "id": scene.id,
                    "title": scene.title,
                    "file_path": scene.file_path,
                    "word_count": scene.word_count,
                })
            })
            .collect();

        serde_json::json!({ "scenes": scenes })
    }

    pub(super) fn exec_story_stats(
        &self,
        story: &StoryAccess<'_>,
        graph: &NarrativeGraph,
    ) -> serde_json::Value {
        serde_json::json!({
            "word_count": story.word_count(),
            "line_count": story.line_count(),
            "scene_count": story.scene_count(),
            "character_count": graph.get_characters().len(),
            "objective_count": graph.get_objectives().len(),
            "conflict_count": graph.get_conflicts().len(),
        })
    }

    pub(super) fn exec_read_context_file(
        &self,
        args: &serde_json::Value,
        manifest: Option<&Manifest>,
        project_root: Option<&Path>,
    ) -> serde_json::Value {
        let manifest = match manifest {
            Some(m) => m,
            None => {
                return serde_json::json!({ "error": "No manifest loaded. Run `laires scan` first." });
            }
        };
        let project_root = match project_root {
            Some(r) => r,
            None => return serde_json::json!({ "error": "Project root not available" }),
        };

        let file_ref = args["file"].as_str().unwrap_or("");

        let context_file = manifest
            .context_files
            .iter()
            .find(|cf| cf.role.to_string() == file_ref)
            .or_else(|| {
                manifest
                    .context_files
                    .iter()
                    .find(|cf| cf.path == file_ref || cf.path.ends_with(file_ref))
            });

        match context_file {
            Some(cf) => {
                let full_path = project_root.join(&cf.path);
                let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");

                let content = if ext == "docx" {
                    match crate::concepts::docx::extract_text_from_docx(&full_path) {
                        Ok(text) => text,
                        Err(e) => {
                            return serde_json::json!({
                                "error": format!("Failed to read {}: {e}", cf.path)
                            });
                        }
                    }
                } else {
                    match std::fs::read_to_string(&full_path) {
                        Ok(text) => text,
                        Err(e) => {
                            return serde_json::json!({
                                "error": format!("Failed to read {}: {e}", cf.path)
                            });
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

    pub(super) fn exec_list_files(&self, manifest: Option<&Manifest>) -> serde_json::Value {
        let manifest = match manifest {
            Some(m) => m,
            None => {
                return serde_json::json!({ "error": "No manifest loaded. Run `laires scan` first." });
            }
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
}
