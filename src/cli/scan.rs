use crate::concepts::analysis::{apply_analysis_to_graph, Analysis, AnalysisKind, AnalysisTask, Priority};
use crate::concepts::file_buffer_manager::FileBufferManager;
use crate::concepts::manifest::{
    self, build_classification_prompt, build_manifest_from_classification,
    discover_files, diff_against_manifest, parse_classification_response,
    print_classification, Manifest,
};
use crate::concepts::narrative_graph::NarrativeGraph;
use crate::concepts::provider::{Message, Provider, Role};
use crate::config::{self, ProjectConfig, LAIRES_DIR};
use crate::runtime::scene_cache::SceneCache;

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

    // Persist graph and scene cache
    graph.save(&graph_path)?;
    SceneCache::from_file_buffer_manager(&fbm).save_to_project(&project_root)?;

    println!("{}", graph.summary());

    Ok(())
}

// ── Classification helpers ─────────────────────────────────────────

async fn classify_and_confirm(
    discovered: &[manifest::DiscoveredFile],
    config: &ProjectConfig,
    project_root: &std::path::Path,
) -> anyhow::Result<Manifest> {
    let classification_model = if config.classification.model.is_empty() {
        &config.llm.model
    } else {
        &config.classification.model
    };

    // Build and send classification prompt
    let prompt = build_classification_prompt(discovered);

    let mut provider = Provider::from_project_config_with_model(config, classification_model)
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
