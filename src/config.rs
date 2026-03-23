use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const LAIRES_DIR: &str = ".laires";
pub const CONFIG_FILE: &str = "config.toml";
pub const GRAPH_FILE: &str = "graph.json";
pub const SCENES_FILE: &str = "scenes.json";
pub const OVERRIDES_FILE: &str = "overrides.json";
pub const PERSPECTIVES_CACHE_DIR: &str = "cache/perspectives";
pub const CHAT_HISTORY_FILE: &str = "chat_history.json";
pub const MANIFEST_FILE: &str = "manifest.toml";
pub const SKILL_LOG_FILE: &str = "skill_log.jsonl";
pub const BRIEFS_DIR: &str = "briefs";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub llm: LlmConfig,
    pub project: ProjectMeta,
    #[serde(default)]
    pub analysis: AnalysisConfig,
    #[serde(default)]
    pub privacy: PrivacyConfig,
    #[serde(default)]
    pub classification: ClassificationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub title: String,
    #[serde(default = "default_format")]
    pub format: String,
    /// Default session mode: "consultant" or "workshop".
    /// If unset, inferred from file types in the manifest.
    #[serde(default)]
    pub default_mode: Option<String>,
}

fn default_format() -> String {
    "prose".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisConfig {
    #[serde(default = "default_debounce")]
    pub debounce_ms: u64,
    #[serde(default = "default_auto_scan")]
    pub auto_scan: bool,
}

fn default_debounce() -> u64 {
    2000
}

fn default_auto_scan() -> bool {
    true
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            debounce_ms: default_debounce(),
            auto_scan: default_auto_scan(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrivacyConfig {
    #[serde(default)]
    pub restricted_when_cloud: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationConfig {
    #[serde(default = "default_classification_model")]
    pub model: String,
}

fn default_classification_model() -> String {
    String::new() // empty = use the project's main LLM model
}

impl Default for ClassificationConfig {
    fn default() -> Self {
        Self {
            model: default_classification_model(),
        }
    }
}

impl ProjectConfig {
    pub fn load(project_dir: &Path) -> anyhow::Result<Self> {
        let config_path = project_dir.join(LAIRES_DIR).join(CONFIG_FILE);
        let content = std::fs::read_to_string(&config_path)?;
        let config: ProjectConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, project_dir: &Path) -> anyhow::Result<()> {
        let config_path = project_dir.join(LAIRES_DIR).join(CONFIG_FILE);
        let content = toml::to_string_pretty(self)?;
        std::fs::write(config_path, content)?;
        Ok(())
    }

    pub fn default_gemini(title: &str) -> Self {
        Self {
            llm: LlmConfig {
                provider: "gemini".to_string(),
                model: "gemini-2.5-flash".to_string(),
                api_key_env: Some("GEMINI_API_KEY".to_string()),
                base_url: Some(
                    "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
                ),
            },
            project: ProjectMeta {
                title: title.to_string(),
                format: "prose".to_string(),
                default_mode: None,
            },
            analysis: AnalysisConfig::default(),
            privacy: PrivacyConfig::default(),
            classification: ClassificationConfig::default(),
        }
    }
}

/// Write or update a KEY=VALUE line in a `.env` file.
/// Creates the file if it doesn't exist. Updates the value if the key already exists.
pub fn write_env_file(env_path: &Path, key: &str, value: &str) -> anyhow::Result<()> {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    let new_line = format!("{key}=\"{escaped}\"");

    if env_path.exists() {
        let content = std::fs::read_to_string(env_path)?;
        let prefix = format!("{key}=");
        let mut found = false;
        let mut lines: Vec<String> = content
            .lines()
            .map(|line| {
                if line.starts_with(&prefix) {
                    found = true;
                    new_line.clone()
                } else {
                    line.to_string()
                }
            })
            .collect();
        if !found {
            lines.push(new_line);
        }
        std::fs::write(env_path, lines.join("\n") + "\n")?;
    } else {
        std::fs::write(env_path, format!("{new_line}\n"))?;
    }
    Ok(())
}

/// Ensure `.env` is listed in `.gitignore`. Appends it if missing.
pub fn ensure_gitignore_has_dotenv(project_root: &Path) -> anyhow::Result<()> {
    let gitignore_path = project_root.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        if !content.lines().any(|line| line.trim() == ".env") {
            std::fs::write(&gitignore_path, format!("{content}\n.env\n"))?;
        }
    } else {
        std::fs::write(&gitignore_path, ".env\n")?;
    }
    Ok(())
}

/// Resolve the project root by walking up from `start` looking for .laires/
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(LAIRES_DIR).is_dir() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Get the story file path for a project
pub fn story_file_path(project_dir: &Path, format: &str) -> PathBuf {
    match format {
        "fountain" => project_dir.join("story.fountain"),
        _ => project_dir.join("story.md"),
    }
}
