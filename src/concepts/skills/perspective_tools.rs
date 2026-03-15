use crate::concepts::character_perspective::CharacterPerspective;
use crate::concepts::narrative_graph::GraphNode;
use crate::concepts::provider::Provider;

use super::{find_character_id, SkillContext, Skills};

impl Skills {
    pub(super) async fn exec_interpret_as_character(
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

        if let Some(scene_ref) = args["scene"].as_str() {
            let scene_id = self.with_story_access(ctx, |story| story.resolve_scene_id(scene_ref));
            match scene_id {
                Some(sid) => {
                    let scene_text = self.with_story_access(ctx, |story| {
                        story
                            .read_scene_by_id(&sid)
                            .map(|scene| scene.text)
                            .unwrap_or_default()
                    });
                    let perspectives = match ctx.perspectives.as_mut() {
                        Some(p) => p,
                        None => {
                            return serde_json::json!({
                                "error": "Perspective store not available"
                            })
                        }
                    };
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
            let perspectives = match ctx.perspectives.as_mut() {
                Some(p) => p,
                None => {
                    return serde_json::json!({
                        "error": "Perspective store not available"
                    })
                }
            };
            match perspectives
                .generate_perspective(&char_id, ctx.graph, provider)
                .await
            {
                Ok(p) => serde_json::json!(p),
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            }
        }
    }

    pub(super) async fn exec_compare_perspectives(
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

        let scene_id = match self.with_story_access(ctx, |story| story.resolve_scene_id(scene_ref))
        {
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

        let scene_text = self.with_story_access(ctx, |story| {
            story
                .read_scene_by_id(&scene_id)
                .map(|scene| scene.text)
                .unwrap_or_default()
        });
        let perspectives = match ctx.perspectives.as_mut() {
            Some(p) => p,
            None => {
                return serde_json::json!({
                    "error": "Perspective store not available"
                })
            }
        };
        match perspectives
            .compare_perspectives(&char_a, &char_b, &scene_id, ctx.graph, &scene_text, provider)
            .await
        {
            Ok(result) => serde_json::json!(result),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }

    pub(super) async fn exec_find_blind_spots(
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

        match perspectives.find_blind_spots(&char_id, ctx.graph, provider).await {
            Ok(spots) => serde_json::json!({
                "character": character_name,
                "blind_spot_count": spots.len(),
                "blind_spots": spots,
            }),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }

    pub(super) fn exec_get_knowledge_at(
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

        let full_boundary = CharacterPerspective::compute_knowledge_boundary(ctx.graph, &char_id);

        let boundary = if let Some(scene_ref) = args["scene"].as_str() {
            let target_scene_id =
                self.with_story_access(ctx, |story| story.resolve_scene_id(scene_ref));
            if let Some(target_id) = target_scene_id {
                let scene_list = self.with_story_access(ctx, |story| story.list_scenes());
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

        let scene_info: Vec<serde_json::Value> = boundary
            .iter()
            .filter_map(|sid| {
                ctx.graph.get_node(sid).map(|n| {
                    if let GraphNode::Scene { title, summary, .. } = n {
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
}
