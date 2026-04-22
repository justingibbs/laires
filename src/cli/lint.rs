use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::narrative_graph::{GraphNode, NarrativeGraph};
use crate::concepts::scene_map::{ParseMode, SceneMap};
use crate::concepts::text_buffer::TextBuffer;
use crate::config::{self, LAIRES_DIR, OVERRIDES_FILE, ProjectConfig};
use crate::sync::divergence;

pub fn run() -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let config = ProjectConfig::load(&project_root)?;
    let story_path = config::story_file_path(&project_root, &config.project.format);

    if !story_path.exists() {
        anyhow::bail!("Story file not found: {}", story_path.display());
    }

    // Load text and scene map
    let text_buffer = TextBuffer::from_file(story_path)?;
    let full_text = text_buffer.read_all();

    let parse_mode = match config.project.format.as_str() {
        "fountain" => ParseMode::Fountain,
        _ => ParseMode::Prose,
    };
    let mut scene_map = SceneMap::new(parse_mode);
    scene_map.full_reindex(&full_text, "");

    // Load graph
    let graph_path = project_root.join(LAIRES_DIR).join("graph.json");
    let graph = if graph_path.exists() {
        NarrativeGraph::load(&graph_path).unwrap_or_default()
    } else {
        println!("No graph found. Run `laires scan` first.");
        return Ok(());
    };

    // Load declared intent
    let overrides_path = project_root.join(LAIRES_DIR).join(OVERRIDES_FILE);
    let intent = if overrides_path.exists() {
        DeclaredIntent::load(&overrides_path).ok()
    } else {
        None
    };

    println!("Laires Lint - {}\n", config.project.title);

    let mut error_count = 0u32;
    let mut warning_count = 0u32;
    let mut info_count = 0u32;

    // 1. Orphaned objectives (character doesn't exist)
    for obj in graph.get_objectives() {
        if let GraphNode::Objective {
            character_id,
            description,
            ..
        } = obj
            && graph.get_node(character_id).is_none()
        {
            print_issue(
                "error",
                &format!(
                    "Orphaned objective: \"{description}\" references nonexistent character {character_id}"
                ),
            );
            error_count += 1;
        }
    }

    // 2. PresentIn edge consistency
    for scene in graph.get_scenes() {
        if let GraphNode::Scene {
            id,
            characters_present,
            ..
        } = scene
        {
            for cid in characters_present {
                if graph.get_node(cid).is_none() {
                    print_issue(
                        "error",
                        &format!("Scene {id} lists nonexistent character {cid}"),
                    );
                    error_count += 1;
                }
            }
        }
    }

    // 3. Dead scenes (no Advances/Blocks edges)
    let dead = graph.find_dead_scenes();
    for sid in &dead {
        let title = graph
            .get_node(sid)
            .and_then(|n| {
                if let GraphNode::Scene { title, .. } = n {
                    title.clone()
                } else {
                    None
                }
            })
            .unwrap_or_else(|| sid.clone());
        print_issue(
            "warning",
            &format!("Dead scene \"{title}\": no objectives advance or are blocked"),
        );
        warning_count += 1;
    }

    // 4. Stale scenes (pending reindex)
    let pending = scene_map.get_pending();
    if !pending.is_empty() {
        print_issue(
            "info",
            &format!(
                "{} scene(s) have unanalyzed changes. Run `laires scan` to update.",
                pending.len()
            ),
        );
        info_count += 1;
    }

    // 5. Orphaned declarations
    if let Some(ref intent) = intent {
        let valid_ids: std::collections::HashSet<String> =
            graph.all_node_ids().into_iter().collect();
        let orphans = intent.find_orphans(&valid_ids);
        for orphan in &orphans {
            print_issue(
                "warning",
                &format!(
                    "Orphaned declaration: {}.{} references nonexistent node",
                    orphan.node_id, orphan.field
                ),
            );
            warning_count += 1;
        }

        // 6. Unresolved divergences
        let divs = divergence::detect_divergences(&graph, intent);
        for div in &divs {
            print_issue(
                "info",
                &format!(
                    "Divergence on {}.{}: inferred=\"{}\" vs declared=\"{}\"{}",
                    div.node_id,
                    div.field,
                    div.inferred,
                    div.declared,
                    div.rationale
                        .as_ref()
                        .map(|r| format!(" (rationale: {r})"))
                        .unwrap_or_default()
                ),
            );
            info_count += 1;
        }
    }

    // Summary
    println!();
    let total = error_count + warning_count + info_count;
    if total == 0 {
        println!("No issues found.");
    } else {
        println!(
            "{total} issue(s): {error_count} error(s), {warning_count} warning(s), {info_count} info"
        );
    }

    Ok(())
}

fn print_issue(severity: &str, message: &str) {
    let prefix = match severity {
        "error" => "  ERROR",
        "warning" => "  WARN ",
        "info" => "  INFO ",
        _ => "  ?    ",
    };
    println!("{prefix}  {message}");
}
