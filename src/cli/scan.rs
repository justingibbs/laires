use crate::concepts::analysis::{Analysis, AnalysisKind, AnalysisTask, Priority};
use crate::concepts::file_buffer_manager::FileBufferManager;
use crate::concepts::manifest::{
    self, build_classification_prompt, build_manifest_from_classification,
    discover_files, diff_against_manifest, parse_classification_response,
    print_classification, Manifest,
};
use crate::concepts::narrative_graph::NarrativeGraph;
use crate::concepts::provider::{Message, Provider, Role};
use crate::config::{self, ProjectConfig, LAIRES_DIR};

pub async fn run(scene_num: Option<usize>, full: bool) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let config = ProjectConfig::load(&project_root)?;

    // ── Phase 1: File Discovery & Classification ───────────────────

    let manifest = if Manifest::exists(&project_root) && !full {
        let existing = Manifest::load(&project_root)?;
        let discovered = discover_files(&project_root)?;
        let diff = diff_against_manifest(&discovered, &existing);

        if !diff.removed_files.is_empty() {
            println!(
                "Removed {} file(s) no longer on disk.",
                diff.removed_files.len()
            );
        }

        if diff.new_files.is_empty() && diff.removed_files.is_empty() {
            if !diff.changed_files.is_empty() {
                println!(
                    "{} file(s) changed since last scan. Re-analyzing...",
                    diff.changed_files.len()
                );
            }
            // No structural changes — use existing manifest
            // (but remove entries for deleted files)
            existing
        } else {
            // New or removed files: re-classify everything
            println!(
                "Found {} new file(s). Classifying...",
                diff.new_files.len()
            );
            classify_and_confirm(&discovered, &config, &project_root).await?
        }
    } else {
        // Full scan or no manifest: classify everything
        let discovered = discover_files(&project_root)?;
        if discovered.is_empty() {
            println!("No text files found in project directory.");
            println!(
                "Add .md, .fountain, or .txt files and run `laires scan` again."
            );
            return Ok(());
        }
        println!(
            "Discovered {} file(s). Classifying...",
            discovered.len()
        );
        classify_and_confirm(&discovered, &config, &project_root).await?
    };

    if manifest.story_files.is_empty() {
        println!("No story files in manifest. Nothing to analyze.");
        return Ok(());
    }

    // ── Phase 2: Scene Analysis (per story file) ───────────────────

    let mut provider = Provider::from_project_config(&config)?;
    let mut analysis = Analysis::new();

    let graph_path = project_root.join(LAIRES_DIR).join("graph.json");
    let mut graph = if graph_path.exists() {
        NarrativeGraph::load(&graph_path).unwrap_or_default()
    } else {
        NarrativeGraph::new()
    };

    let mut fbm = FileBufferManager::from_manifest(&manifest, &project_root)?;

    // Print scene summary per file
    for entry in fbm.entries() {
        if entry.text_buffer.read_all().trim().is_empty() {
            println!("{}: empty, skipping.", entry.file_path);
            continue;
        }

        println!(
            "\n{}: {} scene(s)",
            entry.file_path,
            entry.scene_map.scene_count(),
        );

        for (i, scene) in entry.scene_map.list_scenes().iter().enumerate() {
            let scene_text = entry
                .text_buffer
                .read(scene.byte_range())
                .unwrap_or_default();
            let word_count = scene_text.split_whitespace().count();
            let title = scene.title.as_deref().unwrap_or("(untitled)");
            println!("  Scene {}: {} ({} words)", i + 1, title, word_count);
        }
    }

    // Determine which scenes to analyze (respecting --scene flag for single-file)
    let all_scenes = fbm.list_all_scenes();
    let scenes_to_analyze: Vec<_> = match scene_num {
        Some(num) => {
            if num == 0 || num > all_scenes.len() {
                anyhow::bail!(
                    "Scene {} out of range. There are {} scenes.",
                    num,
                    all_scenes.len()
                );
            }
            vec![all_scenes[num - 1].clone()]
        }
        None => all_scenes.iter().copied().cloned().collect(),
    };

    println!(
        "\nAnalyzing {} scene(s) across {} file(s)...\n",
        scenes_to_analyze.len(),
        fbm.story_file_count(),
    );

    let graph_context = graph.serialize_compact();

    for scene in &scenes_to_analyze {
        let scene_text = fbm
            .get_scene(&scene.id)
            .and_then(|(s, buf)| buf.read(s.byte_range()).ok())
            .unwrap_or_default();

        analysis.enqueue(AnalysisTask {
            kind: AnalysisKind::SceneAnalysis {
                scene_id: scene.id.clone(),
            },
            priority: Priority::Normal,
            created: chrono::Utc::now(),
        });

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
                let title =
                    scene.title.as_deref().unwrap_or("(untitled)");
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

                apply_analysis_to_graph(&mut graph, &result, &scene.id, &scene.file_path);
                if let Some(entry) = fbm.get_entry_mut(&scene.file_path) {
                    entry.scene_map.mark_analyzed(&scene.id);
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!(
                    "Analysis error for scene \"{}\": {e}",
                    scene.id
                );
            }
        }
    }

    // Auto-generate PresentIn edges from Fountain character cues
    for entry in fbm.entries() {
        for cue in entry.scene_map.character_cues() {
            let char_id =
                graph.get_characters().iter().find_map(|c| {
                    if let crate::concepts::narrative_graph::GraphNode::Character {
                        id,
                        name,
                        ..
                    } = c
                    {
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
    }

    // Persist graph and scene maps
    graph.save(&graph_path)?;
    for entry in fbm.entries() {
        entry
            .scene_map
            .save(&project_root.join(LAIRES_DIR).join("scenes.json"))?;
    }

    println!("{}", graph.summary());

    Ok(())
}

// ── Classification helpers ─────────────────────────────────────────

async fn classify_and_confirm(
    discovered: &[manifest::DiscoveredFile],
    config: &ProjectConfig,
    project_root: &std::path::Path,
) -> anyhow::Result<Manifest> {
    let classification_model = &config.classification.model;

    // Build and send classification prompt
    let prompt = build_classification_prompt(discovered);

    let mut provider = Provider::from_project_config_with_model(config, &config.classification.model)
        .map_err(|e| anyhow::anyhow!("Classification provider error: {e}"))?;

    let messages = vec![Message {
        role: Role::User,
        content: prompt,
        tool_calls: None,
        tool_results: None,
    }];

    let system = "You are a file classification assistant for a fiction writing tool. \
                  Classify files into story, outline, characters, notes, or excluded. \
                  Respond with JSON only.";

    println!("Classifying files with {}...", classification_model);

    let response = provider
        .complete(&messages, &[], Some(system))
        .await
        .map_err(|e| anyhow::anyhow!("Classification LLM error: {e}"))?;

    let raw = response.content.unwrap_or_default();
    let result = parse_classification_response(&raw)?;

    // Show classification and ask for confirmation
    print_classification(&result);

    if confirm_classification()? {
        let manifest = build_manifest_from_classification(
            result,
            discovered,
            classification_model,
        );
        manifest.save(project_root)?;
        println!("Manifest saved.");
        Ok(manifest)
    } else {
        anyhow::bail!(
            "Classification not accepted. Edit .laires/manifest.toml manually or run `laires scan --full`."
        )
    }
}

fn confirm_classification() -> anyhow::Result<bool> {
    use std::io::{self, Write};
    print!("Accept this classification? [Y/n] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();

    Ok(trimmed.is_empty() || trimmed == "y" || trimmed == "yes")
}

/// Apply analysis results to the narrative graph (implements Sync S1.3).
/// Returns the set of character IDs affected (for perspective invalidation).
pub fn apply_analysis_to_graph(
    graph: &mut NarrativeGraph,
    result: &crate::concepts::analysis::AnalysisResult,
    scene_id: &str,
    file_path: &str,
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
        file_path: file_path.to_string(),
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
