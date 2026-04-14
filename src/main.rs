mod cli;
mod concepts;
mod config;
mod error;
mod gui;
mod runtime;
mod sync;

use clap::Parser;
use cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load global .env first (~/.config/laires/.env), then project-local .env.
    // Project-local values override global ones.
    if let Some(global_env) = config::global_env_path() {
        dotenvy::from_path(&global_env).ok();
    }
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    cli::run(cli).await
}
