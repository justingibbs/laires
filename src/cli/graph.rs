use crate::concepts::narrative_graph::NarrativeGraph;
use crate::config::{self, LAIRES_DIR};

pub fn run(character: Option<&str>, json: bool) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let graph_path = project_root.join(LAIRES_DIR).join("graph.json");

    if !graph_path.exists() {
        anyhow::bail!("No graph found. Run `laires scan` first.");
    }

    let graph = NarrativeGraph::load(&graph_path)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&graph.serialize())?);
        return Ok(());
    }

    match character {
        Some(name) => {
            // Find character and show their arc
            let char_id = graph.get_characters().iter().find_map(|c| {
                if let crate::concepts::narrative_graph::GraphNode::Character {
                    id, name: n, ..
                } = c
                {
                    if n.eq_ignore_ascii_case(name) {
                        Some(id.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

            match char_id {
                Some(id) => {
                    let arc = graph.get_character_arc(&id);
                    println!("Character arc for {name}:\n");
                    if arc.is_empty() {
                        println!("  No objectives tracked yet.");
                    } else {
                        for state in &arc {
                            println!("  [{:?}] {}", state.status, state.description);
                            if !state.scene_id.is_empty() {
                                println!("    in scene: {}", state.scene_id);
                            }
                        }
                    }
                }
                None => {
                    println!("Character \"{name}\" not found in the graph.");
                    println!("\nAvailable characters:");
                    for c in graph.get_characters() {
                        if let crate::concepts::narrative_graph::GraphNode::Character {
                            name, ..
                        } = c
                        {
                            println!("  - {name}");
                        }
                    }
                }
            }
        }
        None => {
            print!("{}", graph.summary());
        }
    }

    Ok(())
}
