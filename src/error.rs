use thiserror::Error;

#[derive(Error, Debug)]
pub enum LairesError {
    #[error("Project not initialized. Run `laires init` first.")]
    NotInitialized,

    #[error("Project already initialized at {0}")]
    AlreadyInitialized(String),

    #[error("Story file not found: {0}")]
    StoryFileNotFound(String),

    #[error("Scene not found: {0}")]
    SceneNotFound(String),

    #[error("Invalid byte range: {start}..{end}")]
    InvalidRange { start: usize, end: usize },

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Analysis error: {0}")]
    Analysis(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Toml(#[from] toml::de::Error),
}

pub type Result<T> = std::result::Result<T, LairesError>;
