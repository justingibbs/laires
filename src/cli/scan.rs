use crate::concepts::analysis::{Analysis, AnalysisKind, AnalysisTask, Priority};
use crate::concepts::narrative_graph::NarrativeGraph;
use crate::concepts::provider::Provider;
use crate::concepts::scene_map::{ParseMode, SceneMap};
use crate::concepts::text_buffer::TextBuffer;
use crate::config::{self, ProjectConfig, LAIRES_DIR};

pub async fn run(scene_num: Option<usize>) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let config = ProjectConfig::load(&project_root)?;
    let story_path = config::story_file_path(&project_root, &config.project.format);

    if !story_path.exists() {
        anyhow::bail!("Story file not found: {}", story_path.display());
    }

    // Load text buffer
    let text_buffer = TextBuffer::from_file(story_path)?;
    let full_text = text_buffer.read_all();

    if full_text.trim().is_empty() {
        println!("Story file is empty. Write some text first!");
        return Ok(());
    }

    // Parse scenes
    let parse_mode = match config.project.format.as_str() {
        "fountain" => ParseMode::Fountain,
        _ => ParseMode::Prose,
    };
    let mut scene_map = SceneMap::new(parse_mode);
    scene_map.full_reindex(&full_text);

    println!(
        "Found {} scene(s) in {}",
        scene_map.scene_count(),
        text_buffer.file_path().display()
    );

    // Print scene list
    for (i, scene) in scene_map.list_scenes().iter().enumerate() {
        let scene_text = text_buffer.read(scene.byte_range()).unwrap_or_default();
        let word_count = scene_text.split_whitespace().count();
        let title = scene
            .title
            .as_deref()
            .unwrap_or("(untitled)");
        println!("  Scene {}: {} ({} words)", i + 1, title, word_count);
    }

    // Set up provider
    let mut provider = Provider::from_project_config(&config)?;
    let mut analysis = Analysis::new();

    // Load or create graph
    let graph_path = project_root.join(LAIRES_DIR).join("graph.json");
    let mut graph = if graph_path.exists() {
        NarrativeGraph::load(&graph_path).unwrap_or_default()
    } else {
        NarrativeGraph::new()
    };

    // Enqueue analysis tasks
    let scenes_to_analyze: Vec<_> = match scene_num {
        Some(num) => {
            let scenes = scene_map.list_scenes();
            if num == 0 || num > scenes.len() {
                anyhow::bail!(
                    "Scene {} out of range. There are {} scenes.",
                    num,
                    scenes.len()
                );
            }
            vec![scenes[num - 1].clone()]
        }
        None => scene_map.list_scenes().to_vec(),
    };

    println!("\nAnalyzing {} scene(s)...\n", scenes_to_analyze.len());

    let graph_context = graph.serialize_compact();

    for scene in &scenes_to_analyze {
        let scene_text = text_buffer.read(scene.byte_range()).unwrap_or_default();

        analysis.enqueue(AnalysisTask {
            kind: AnalysisKind::SceneAnalysis {
                scene_id: scene.id.clone(),
            },
            priority: Priority::Normal,
            created: chrono::Utc::now(),
        });

        // Process the task
        match analysis
            .process_next(
                &mut provider,
                &scene_text,
                &graph_context,
                &scene.content_hash,
            )
            .await
        {
            Ok(Some(result)) => {
                let title = scene.title.as_deref().unwrap_or("(untitled)");
                println!("Scene \"{title}\":");
                println!(
                    "  Characters: {}",
                    result
                        .characters_found
                        .iter()
                        .map(|c| c.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                println!(
                    "  Objectives: {}",
                    result.objectives_found.len()
                );
                println!(
                    "  Conflicts: {}",
                    result.conflicts_found.len()
                );
                if let Some(meta) = &result.scene_metadata {
                    println!("  Summary: {}", meta.summary);
                }
                println!();

                // Feed results into the graph (Sync S1.3)
                apply_analysis_to_graph(&mut graph, &result, &scene.id);

                // Mark scene as analyzed
                scene_map.mark_analyzed(&scene.id);
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("Analysis error for scene \"{}\": {e}", scene.id);
            }
        }
    }

    // Auto-generate PresentIn edges from Fountain character cues
    for cue in scene_map.character_cues() {
        // Find or skip character by name
        let char_id = graph.get_characters().iter().find_map(|c| {
            if let crate::concepts::narrative_graph::GraphNode::Character { id, name, .. } = c {
                if name.eq_ignore_ascii_case(&cue.character_name) {
                    Some(id.clone())
                } else {
                    None
                }
            } else {
                None
            }
        });
        if let Some(cid) = char_id {
            graph.add_edge(
                &cid,
                &cue.scene_id,
                crate::concepts::narrative_graph::GraphEdge::PresentIn,
            );
        }
    }

    // Persist
    graph.save(&graph_path)?;
    scene_map.save(&project_root.join(LAIRES_DIR).join("scenes.json"))?;

    println!("{}", graph.summary());

    Ok(())
}

/// Apply analysis results to the narrative graph (implements Sync S1.3).
/// Returns the set of character IDs affected (for perspective invalidation).
pub fn apply_analysis_to_graph(
    graph: &mut NarrativeGraph,
    result: &crate::concepts::analysis::AnalysisResult,
    scene_id: &str,
) -> Vec<String> {
    use crate::concepts::narrative_graph::*;

    // Add characters
    for char_data in &result.characters_found {
        // Check if character already exists by name
        let existing = graph.get_characters().iter().find_map(|c| {
            if let GraphNode::Character { id, name, .. } = c {
                if name.eq_ignore_ascii_case(&char_data.name) {
                    Some(id.clone())
                } else {
                    None
                }
            } else {
                None
            }
        });

        if existing.is_none() {
            let id = new_id();
            graph.add_node(GraphNode::Character {
                id: id.clone(),
                name: char_data.name.clone(),
                aliases: char_data.aliases.clone(),
                description: Some(char_data.description.clone()),
            });
        }
    }

    // Add/update scene node
    let scene_node_exists = graph.get_node(scene_id).is_some();
    let characters_present: Vec<String> = result
        .characters_found
        .iter()
        .map(|c| {
            // Find the character's ID
            graph
                .get_characters()
                .iter()
                .find_map(|gc| {
                    if let GraphNode::Character { id, name, .. } = gc {
                        if name.eq_ignore_ascii_case(&c.name) {
                            Some(id.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        })
        .filter(|id| !id.is_empty())
        .collect();

    let scene_node = GraphNode::Scene {
        id: scene_id.to_string(),
        title: result
            .scene_metadata
            .as_ref()
            .and_then(|m| m.title.clone()),
        summary: result
            .scene_metadata
            .as_ref()
            .map(|m| m.summary.clone())
            .unwrap_or_default(),
        characters_present: characters_present.clone(),
        location: result
            .scene_metadata
            .as_ref()
            .and_then(|m| m.location.clone()),
        time: result
            .scene_metadata
            .as_ref()
            .and_then(|m| m.time.clone()),
    };

    if scene_node_exists {
        graph.update_node(scene_id, scene_node);
    } else {
        graph.add_node(scene_node);
    }

    // Add PresentIn edges
    for char_id in &characters_present {
        graph.add_edge(char_id, scene_id, GraphEdge::PresentIn);
    }

    // Add objectives
    for obj_data in &result.objectives_found {
        let char_id = graph.get_characters().iter().find_map(|c| {
            if let GraphNode::Character { id, name, .. } = c {
                if name.eq_ignore_ascii_case(&obj_data.character_name) {
                    Some(id.clone())
                } else {
                    None
                }
            } else {
                None
            }
        });

        if let Some(cid) = char_id {
            let obj_id = new_id();
            graph.add_node(GraphNode::Objective {
                id: obj_id.clone(),
                character_id: cid.clone(),
                scope: obj_data.scope,
                description: obj_data.description.clone(),
                evidence: obj_data.evidence.clone(),
                confidence: obj_data.confidence,
                status: obj_data.status,
            });

            // Add Pursues edge: Character -> Objective
            graph.add_edge(
                &cid,
                &obj_id,
                GraphEdge::Pursues {
                    scene_id: Some(scene_id.to_string()),
                },
            );

            // Add Advances or Blocks edge: Scene -> Objective
            match obj_data.status {
                Status::Blocked => {
                    graph.add_edge(scene_id, &obj_id, GraphEdge::Blocks);
                }
                _ => {
                    graph.add_edge(scene_id, &obj_id, GraphEdge::Advances);
                }
            }
        }
    }

    // Add conflicts
    for conflict_data in &result.conflicts_found {
        let conflict_id = new_id();
        let objective_ids: Vec<String> = conflict_data
            .between
            .iter()
            .filter_map(|name| {
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
            })
            .collect();

        graph.add_node(GraphNode::Conflict {
            id: conflict_id,
            description: conflict_data.description.clone(),
            objectives: objective_ids,
        });
    }

    // Return affected character IDs for perspective invalidation (Sync S5.2)
    characters_present
}
