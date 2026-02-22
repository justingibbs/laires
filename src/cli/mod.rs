mod chat;
mod graph;
mod init;
mod lint;
mod perspective;
mod scan;
mod status;
mod tui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "laires")]
#[command(about = "An agentic writing tool powered by narrative graph intelligence")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Laires project in the current directory
    Init {
        /// Project title
        #[arg(long, default_value = "Untitled")]
        title: String,

        /// Initialize as a Fountain/screenplay project
        #[arg(long)]
        fountain: bool,
    },

    /// Trigger a full or scene-level re-analysis
    Scan {
        /// Re-analyze only a specific scene (by number)
        #[arg(long)]
        scene: Option<usize>,
    },

    /// Print narrative graph summary
    Graph {
        /// Show arc for a specific character
        #[arg(long)]
        character: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show project status
    Status,

    /// Run consistency checks
    Lint,

    /// Open interactive chat with the narrative agent
    Chat {
        /// Start a fresh session (discard previous chat history)
        #[arg(long)]
        new_session: bool,
    },

    /// Open the TUI (split-pane chat + canvas)
    Open,

    /// Generate character perspective analysis
    Perspective {
        /// Character name
        character: String,

        /// Analyze a specific scene (by number)
        #[arg(long)]
        scene: Option<usize>,

        /// Compare with another character
        #[arg(long)]
        compare_with: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Init { title, fountain } => {
            init::run(&title, fountain)?;
        }
        Commands::Scan { scene } => {
            scan::run(scene).await?;
        }
        Commands::Graph { character, json } => {
            graph::run(character.as_deref(), json)?;
        }
        Commands::Status => {
            status::run()?;
        }
        Commands::Lint => {
            lint::run()?;
        }
        Commands::Chat { new_session } => {
            chat::run(new_session).await?;
        }
        Commands::Open => {
            tui::run_tui().await?;
        }
        Commands::Perspective {
            character,
            scene,
            compare_with,
            json,
        } => {
            perspective::run(&character, scene, compare_with.as_deref(), json).await?;
        }
    }
    Ok(())
}
