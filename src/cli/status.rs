use crate::concepts::narrative_graph::NarrativeGraph;
use crate::concepts::scene_map::{ParseMode, SceneMap};
use crate::concepts::text_buffer::TextBuffer;
use crate::config::{self, ProjectConfig, LAIRES_DIR};

pub fn run() -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let project_root = config::find_project_root(&project_dir)
        .ok_or_else(|| anyhow::anyhow!("Not in a Laires project. Run `laires init` first."))?;

    let config = ProjectConfig::load(&project_root)?;

    println!("Laires Project: {}", config.project.title);
    println!("  Format:   {}", config.project.format);
    println!("  Provider: {} ({})", config.llm.provider, config.llm.model);
    println!();

    // Story file info
    let story_path = config::story_file_path(&project_root, &config.project.format);
    if story_path.exists() {
        let text_buffer = TextBuffer::from_file(story_path.clone())?;
        let full_text = text_buffer.read_all();

        let parse_mode = match config.project.format.as_str() {
            "fountain" => ParseMode::Fountain,
            _ => ParseMode::Prose,
        };
        let mut scene_map = SceneMap::new(parse_mode);
        scene_map.full_reindex(&full_text);

        println!("Story: {}", story_path.display());
        println!("  Words:  {}", text_buffer.word_count());
        println!("  Lines:  {}", text_buffer.line_count());
        println!("  Scenes: {}", scene_map.scene_count());
    } else {
        println!("Story file not found: {}", story_path.display());
    }

    // Graph info
    let graph_path = project_root.join(LAIRES_DIR).join("graph.json");
    if graph_path.exists() {
        let graph = NarrativeGraph::load(&graph_path).unwrap_or_default();
        println!();
        println!("Graph:");
        println!("  Characters:  {}", graph.get_characters().len());
        println!("  Objectives:  {}", graph.get_objectives().len());
        println!("  Scenes:      {}", graph.get_scenes().len());
        println!("  Conflicts:   {}", graph.get_conflicts().len());
        println!("  Total nodes: {}", graph.node_count());
        println!("  Total edges: {}", graph.edge_count());
    } else {
        println!("\nNo graph yet. Run `laires scan` to analyze your manuscript.");
    }

    Ok(())
}
