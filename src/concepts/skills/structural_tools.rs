use crate::concepts::narrative_graph::{GraphNode, NarrativeGraph, Status};

use super::{SkillContext, Skills};

impl Skills {
    pub(super) fn exec_story_lint(&self, ctx: &SkillContext<'_>) -> serde_json::Value {
        let mut issues = Vec::new();

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

        let dead = ctx.graph.find_dead_scenes();
        for sid in &dead {
            issues.push(serde_json::json!({
                "severity": "warning",
                "type": "dead_scene",
                "message": format!("Scene {sid} has no objective changes"),
                "node_id": sid,
            }));
        }

        let pending = ctx.scene_map.get_pending();
        if !pending.is_empty() {
            issues.push(serde_json::json!({
                "severity": "info",
                "type": "stale_scenes",
                "message": format!("{} scene(s) have unanalyzed changes", pending.len()),
                "scene_ids": pending.iter().collect::<Vec<_>>(),
            }));
        }

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

            let divs = crate::sync::divergence::detect_divergences(ctx.graph, intent);
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

    pub(super) fn exec_pacing_analysis(&self, ctx: &SkillContext<'_>) -> serde_json::Value {
        let scenes = ctx.scene_map.list_scenes();
        let mut scene_stats = Vec::new();
        let mut total_words = 0u64;

        for (i, span) in scenes.iter().enumerate() {
            let text = ctx.text_buffer.read(span.byte_range()).unwrap_or_default();
            let word_count = text.split_whitespace().count() as u64;
            total_words += word_count;

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

    pub(super) fn exec_arc_completeness(
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
                        Status::Achieved | Status::Abandoned | Status::Transformed
                    )
                })
                .count();
            let unresolved: Vec<_> = arc
                .iter()
                .filter(|o| matches!(o.status, Status::Active | Status::Blocked))
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

    pub(super) fn exec_declare_intent(
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

    pub(super) fn exec_retract_intent(
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
}
