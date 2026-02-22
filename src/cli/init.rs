use std::fs;

use crate::config::{ProjectConfig, LAIRES_DIR};

pub fn run(title: &str, fountain: bool) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    let laires_dir = project_dir.join(LAIRES_DIR);

    if laires_dir.exists() {
        anyhow::bail!(
            "Project already initialized at {}",
            project_dir.display()
        );
    }

    // Create .laires/ directory structure
    fs::create_dir_all(laires_dir.join("cache").join("analysis_cache"))?;

    // Create config
    let mut config = ProjectConfig::default_gemini(title);
    if fountain {
        config.project.format = "fountain".to_string();
    }
    config.save(&project_dir)?;

    // Create story file if it doesn't exist
    let story_file = if fountain {
        project_dir.join("story.fountain")
    } else {
        project_dir.join("story.md")
    };

    if !story_file.exists() {
        let initial_content = if fountain {
            format!(
                "Title: {title}\nCredit: Written by\nAuthor: \n\n\
                 INT. LOCATION - DAY\n\nYour story begins here.\n"
            )
        } else {
            format!(
                "---\ntitle: \"{title}\"\n---\n\n\
                 # {title}\n\nYour story begins here.\n"
            )
        };
        fs::write(&story_file, initial_content)?;
    }

    // Create .gitignore for the .laires directory
    let gitignore_path = project_dir.join(".gitignore");
    if !gitignore_path.exists() {
        fs::write(
            &gitignore_path,
            ".laires/cache/\n",
        )?;
    } else {
        // Append if .laires/cache/ isn't already in .gitignore
        let content = fs::read_to_string(&gitignore_path)?;
        if !content.contains(".laires/cache/") {
            fs::write(
                &gitignore_path,
                format!("{content}\n.laires/cache/\n"),
            )?;
        }
    }

    // Initialize empty graph and scene map
    let graph_path = laires_dir.join("graph.json");
    fs::write(
        &graph_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "nodes": [],
            "edges": []
        }))?,
    )?;

    let scenes_path = laires_dir.join("scenes.json");
    fs::write(
        &scenes_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "scenes": [],
            "parse_mode": if fountain { "Fountain" } else { "Prose" },
            "pending_reindex": []
        }))?,
    )?;

    println!("Initialized Laires project: {title}");
    println!("  Config: {}", laires_dir.join("config.toml").display());
    println!("  Story:  {}", story_file.display());
    println!();
    println!("Next steps:");
    println!("  1. Edit your story file: {}", story_file.display());
    println!("  2. Run `laires scan` to analyze your manuscript");
    println!("  3. Run `laires chat` to talk with the narrative agent");

    Ok(())
}
