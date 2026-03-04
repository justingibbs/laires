mod chat;
pub(crate) mod diff;
mod graph;
pub(crate) mod init;
mod lint;
mod log;
mod perspective;
pub(crate) mod scan;
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

        /// Re-classify and re-analyze all files from scratch
        #[arg(long)]
        full: bool,
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

    /// Show graph changes since last commit
    Diff,

    /// Show commit history with graph change summaries
    Log {
        /// Number of commits to show
        #[arg(short = 'n', long, default_value = "10")]
        count: usize,

        /// Show detailed per-node changes
        #[arg(short, long)]
        verbose: bool,
    },

    /// Open the desktop GUI
    Gui {
        /// Path to a Laires project directory (optional)
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },

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
        Commands::Scan { scene, full } => {
            scan::run(scene, full).await?;
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
        Commands::Diff => {
            diff::run()?;
        }
        Commands::Log { count, verbose } => {
            log::run(count, verbose)?;
        }
        Commands::Chat { new_session } => {
            chat::run(new_session).await?;
        }
        Commands::Open => {
            tui::run_tui().await?;
        }
        Commands::Gui { path } => {
            crate::gui::run_gui(path).await?;
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
