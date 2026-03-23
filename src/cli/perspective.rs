use crate::concepts::character_perspective::CharacterPerspective;
use crate::concepts::narrative_graph::{GraphNode, NarrativeGraph};
use crate::concepts::provider::Provider;
use crate::concepts::scene_map::{ParseMode, SceneMap};
use crate::concepts::text_buffer::TextBuffer;
use crate::config::{self, LAIRES_DIR, ProjectConfig};

pub async fn run(
    character: &str,
    scene: Option<usize>,
    compare_with: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let config = ProjectConfig::load(&project_root)?;
    let story_path = config::story_file_path(&project_root, &config.project.format);

    if !story_path.exists() {
        anyhow::bail!("Story file not found: {}", story_path.display());
    }

    let text_buffer = TextBuffer::from_file(story_path)?;
    let full_text = text_buffer.read_all();

    let parse_mode = match config.project.format.as_str() {
        "fountain" => ParseMode::Fountain,
        _ => ParseMode::Prose,
    };
    let mut scene_map = SceneMap::new(parse_mode);
    scene_map.full_reindex(&full_text, "");

    let graph_path = project_root.join(LAIRES_DIR).join("graph.json");
    let graph = if graph_path.exists() {
        NarrativeGraph::load(&graph_path).unwrap_or_default()
    } else {
        anyhow::bail!("No graph found. Run `laires scan` first.");
    };

    let mut provider = Provider::from_project_config(&config)?;
    let mut perspectives = CharacterPerspective::new();

    // Find character ID by name
    let char_id = find_char_id(&graph, character)?;

    if let Some(compare_name) = compare_with {
        // Comparison mode
        let other_id = find_char_id(&graph, compare_name)?;
        let scene_num = scene.unwrap_or(1);
        let scenes = scene_map.list_scenes();
        if scene_num == 0 || scene_num > scenes.len() {
            anyhow::bail!(
                "Scene {} out of range. There are {} scenes.",
                scene_num,
                scenes.len()
            );
        }
        let scene_span = &scenes[scene_num - 1];
        let scene_text = text_buffer
            .read(scene_span.byte_range())
            .unwrap_or_default();

        println!(
            "Comparing perspectives: {} vs {} in scene {}...\n",
            character, compare_name, scene_num
        );

        let result = perspectives
            .compare_perspectives(
                &char_id,
                &other_id,
                &scene_span.id,
                &graph,
                &scene_text,
                &mut provider,
            )
            .await?;

        if json_output {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!(
                "Scene: {}",
                scene_span.title.as_deref().unwrap_or("(untitled)")
            );
            println!();
            println!("--- {} ---", character);
            println!("  Wants: {}", result.perspective_a.wants);
            println!("  Perceives: {}", result.perspective_a.perceives);
            println!("  Decides: {}", result.perspective_a.decides);
            println!(
                "  Emotional state: {}",
                result.perspective_a.emotional_state
            );
            if let Some(ref blocked) = result.perspective_a.blocked_by {
                println!("  Blocked by: {blocked}");
            }
            println!();
            println!("--- {} ---", compare_name);
            println!("  Wants: {}", result.perspective_b.wants);
            println!("  Perceives: {}", result.perspective_b.perceives);
            println!("  Decides: {}", result.perspective_b.decides);
            println!(
                "  Emotional state: {}",
                result.perspective_b.emotional_state
            );
            if let Some(ref blocked) = result.perspective_b.blocked_by {
                println!("  Blocked by: {blocked}");
            }
            println!();
            println!("--- Divergences ---");
            for d in &result.divergences {
                println!("  - {d}");
            }
        }
    } else if let Some(scene_num) = scene {
        // Single scene perspective
        let scenes = scene_map.list_scenes();
        if scene_num == 0 || scene_num > scenes.len() {
            anyhow::bail!(
                "Scene {} out of range. There are {} scenes.",
                scene_num,
                scenes.len()
            );
        }
        let scene_span = &scenes[scene_num - 1];
        let scene_text = text_buffer
            .read(scene_span.byte_range())
            .unwrap_or_default();

        println!(
            "Generating scene perspective for {} in scene {}...\n",
            character, scene_num
        );

        let sp = perspectives
            .generate_scene_perspective(
                &char_id,
                &scene_span.id,
                &graph,
                &scene_text,
                &mut provider,
            )
            .await?;

        if json_output {
            println!("{}", serde_json::to_string_pretty(&sp)?);
        } else {
            println!(
                "Scene: {}",
                scene_span.title.as_deref().unwrap_or("(untitled)")
            );
            println!("  Wants: {}", sp.wants);
            println!("  Perceives: {}", sp.perceives);
            println!("  Decides: {}", sp.decides);
            println!("  Emotional state: {}", sp.emotional_state);
            if let Some(ref blocked) = sp.blocked_by {
                println!("  Blocked by: {blocked}");
            }
            if !sp.knowledge_gained.is_empty() {
                println!("  Knowledge gained:");
                for k in &sp.knowledge_gained {
                    println!("    - {k}");
                }
            }
        }
    } else {
        // Full perspective
        println!("Generating full perspective for {}...\n", character);

        let perspective = perspectives
            .generate_perspective(&char_id, &graph, &mut provider)
            .await?;

        if json_output {
            println!("{}", serde_json::to_string_pretty(&perspective)?);
        } else {
            println!("Character: {character}");
            println!(
                "Knowledge boundary: {} scene(s)",
                perspective.knowledge_boundary.len()
            );
            println!();

            if !perspective.filtered_arc.is_empty() {
                println!("Objective arc:");
                for obj in &perspective.filtered_arc {
                    println!(
                        "  [{}] {} - {} (awareness: {:?})",
                        obj.scene_id, obj.objective, obj.status, obj.awareness
                    );
                }
                println!();
            }

            if !perspective.interpretation_of_others.is_empty() {
                println!("How they see others:");
                for (cid, interp) in &perspective.interpretation_of_others {
                    println!("  {cid}: {interp}");
                }
            }
        }
    }

    Ok(())
}

fn find_char_id(graph: &NarrativeGraph, name: &str) -> anyhow::Result<String> {
    graph
        .get_characters()
        .iter()
        .find_map(|c| {
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
        .ok_or_else(|| {
            let available: Vec<String> = graph
                .get_characters()
                .iter()
                .filter_map(|c| {
                    if let GraphNode::Character { name, .. } = c {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .collect();
            anyhow::anyhow!(
                "Character '{}' not found. Available: {}",
                name,
                if available.is_empty() {
                    "(none - run `laires scan` first)".to_string()
                } else {
                    available.join(", ")
                }
            )
        })
}
