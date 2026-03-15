use std::path::{Path, PathBuf};

use crate::concepts::character_perspective::CharacterPerspective;
use crate::concepts::declared_intent::DeclaredIntent;
use crate::concepts::file_buffer_manager::FileBufferManager;
use crate::concepts::manifest::Manifest;
use crate::concepts::narrative_graph::NarrativeGraph;
use crate::concepts::provider::Provider;
use crate::concepts::scene_map::{ParseMode, SceneMap};
use crate::concepts::skills::{Permission, Skills};
use crate::concepts::text_buffer::TextBuffer;
use crate::config::{self, ProjectConfig, LAIRES_DIR, OVERRIDES_FILE, PERSPECTIVES_CACHE_DIR};
use crate::runtime::story_access::StoryAccess;

pub struct LoadedProject {
    pub text_buffer: TextBuffer,
    pub scene_map: SceneMap,
    pub graph: NarrativeGraph,
    pub intent: DeclaredIntent,
    pub perspectives: CharacterPerspective,
    pub manifest: Option<Manifest>,
    pub file_buffer_manager: Option<FileBufferManager>,
    pub project_root: PathBuf,
    pub config: ProjectConfig,
}

pub struct WorkspaceSummary {
    pub privacy_label: String,
    pub model_name: String,
    pub scene_count: usize,
    pub char_count: usize,
    pub word_count: usize,
}

pub struct ProjectLoadResult {
    pub project: LoadedProject,
    pub provider: Provider,
    pub skills: Skills,
    pub summary: WorkspaceSummary,
}

pub fn load_project(project_root: &Path) -> anyhow::Result<ProjectLoadResult> {
    let config = ProjectConfig::load(project_root)?;
    let story_path = config::story_file_path(project_root, &config.project.format);

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
        NarrativeGraph::new()
    };

    let overrides_path = project_root.join(LAIRES_DIR).join(OVERRIDES_FILE);
    let intent = if overrides_path.exists() {
        DeclaredIntent::load(&overrides_path).unwrap_or_default()
    } else {
        DeclaredIntent::new()
    };

    let perspectives_dir = project_root.join(LAIRES_DIR).join(PERSPECTIVES_CACHE_DIR);
    let perspectives_path = perspectives_dir.join("perspectives.json");
    let graph_hash = blake3::hash(graph.serialize_compact().as_bytes())
        .to_hex()
        .to_string();
    let perspectives = if perspectives_path.exists() {
        let mut p = CharacterPerspective::load(&perspectives_path).unwrap_or_default();
        p.invalidate_by_graph_hash(&graph_hash);
        p
    } else {
        CharacterPerspective::new()
    };

    let manifest = Manifest::load(project_root).ok();
    let file_buffer_manager = manifest
        .as_ref()
        .and_then(|m| FileBufferManager::from_manifest(m, project_root).ok());

    let provider = Provider::from_project_config(&config)?;
    let mut skills = Skills::new();
    skills.set_provider_locality(provider.is_local());

    if !provider.is_local() {
        for restricted in &config.privacy.restricted_when_cloud {
            skills.set_permission(restricted, Permission::Disabled);
        }
    }

    let story = StoryAccess::new(&text_buffer, &scene_map, file_buffer_manager.as_ref());

    let summary = WorkspaceSummary {
        privacy_label: if provider.is_local() {
            "Local".to_string()
        } else {
            "Cloud".to_string()
        },
        model_name: provider.model_name().to_string(),
        scene_count: story.scene_count(),
        char_count: graph.get_characters().len(),
        word_count: story.word_count(),
    };

    let project = LoadedProject {
        text_buffer,
        scene_map,
        graph,
        intent,
        perspectives,
        manifest,
        file_buffer_manager,
        project_root: project_root.to_path_buf(),
        config,
    };

    Ok(ProjectLoadResult {
        project,
        provider,
        skills,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::path::Path;

    use chrono::Utc;

    use super::load_project;
    use crate::concepts::character_perspective::{CharacterPerspective, Perspective};
    use crate::concepts::declared_intent::DeclaredIntent;
    use crate::concepts::manifest::{Manifest, ManifestMeta, StoryFile};
    use crate::concepts::narrative_graph::{GraphNode, NarrativeGraph};
    use crate::config::{
        AnalysisConfig, ClassificationConfig, LlmConfig, PrivacyConfig, ProjectConfig, ProjectMeta,
        GRAPH_FILE, LAIRES_DIR, OVERRIDES_FILE, PERSPECTIVES_CACHE_DIR,
    };

    fn write_config(
        root: &Path,
        provider: &str,
        model: &str,
        restricted_when_cloud: Vec<&str>,
    ) {
        std::fs::create_dir_all(root.join(LAIRES_DIR)).unwrap();
        let config = ProjectConfig {
            llm: LlmConfig {
                provider: provider.to_string(),
                model: model.to_string(),
                api_key_env: None,
                base_url: None,
            },
            project: ProjectMeta {
                title: "Test Project".to_string(),
                format: "prose".to_string(),
            },
            analysis: AnalysisConfig::default(),
            privacy: PrivacyConfig {
                restricted_when_cloud: restricted_when_cloud
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            },
            classification: ClassificationConfig::default(),
        };
        config.save(root).unwrap();
    }

    fn build_graph() -> NarrativeGraph {
        let mut graph = NarrativeGraph::new();
        graph.add_node(GraphNode::Character {
            id: "char-marcus".to_string(),
            name: "Marcus".to_string(),
            aliases: vec!["Mark".to_string()],
            description: Some("A soldier".to_string()),
        });
        graph.add_node(GraphNode::Scene {
            id: "scene-opening".to_string(),
            title: Some("Opening".to_string()),
            summary: "Marcus arrives.".to_string(),
            characters_present: vec!["char-marcus".to_string()],
            location: Some("Embassy".to_string()),
            time: Some("Morning".to_string()),
            file_path: "chapters/one.md".to_string(),
        });
        graph
    }

    fn write_graph(root: &Path) -> NarrativeGraph {
        let graph = build_graph();
        graph
            .save(&root.join(LAIRES_DIR).join(GRAPH_FILE))
            .unwrap();
        graph
    }

    fn write_overrides(root: &Path) {
        let mut intent = DeclaredIntent::new();
        intent.declare(
            "char-marcus",
            "description",
            "The rightful heir".to_string(),
            Some("writer override".to_string()),
        );
        intent
            .save(&root.join(LAIRES_DIR).join(OVERRIDES_FILE))
            .unwrap();
    }

    fn write_stale_perspectives(root: &Path) {
        let mut perspectives = CharacterPerspective::new();
        perspectives.store_perspective(Perspective {
            character_id: "char-marcus".to_string(),
            knowledge_boundary: HashSet::new(),
            filtered_arc: Vec::new(),
            interpretation_of_others: HashMap::new(),
            generated_at: Utc::now(),
            graph_hash: "stale-graph-hash".to_string(),
        });
        let path = root
            .join(LAIRES_DIR)
            .join(PERSPECTIVES_CACHE_DIR)
            .join("perspectives.json");
        perspectives.save(&path).unwrap();
    }

    fn write_manifest_project(root: &Path) {
        std::fs::create_dir_all(root.join("chapters")).unwrap();
        std::fs::write(
            root.join("story.md"),
            "Primary loader story file.\n\nThis file exists so project loading succeeds.",
        )
        .unwrap();
        std::fs::write(
            root.join("chapters/one.md"),
            "## One\n\nMarcus arrives at the embassy in silence.\n\n## Two\n\nElena waits by the sealed gate.",
        )
        .unwrap();
        std::fs::write(
            root.join("chapters/two.md"),
            "## Three\n\nThe station is empty and cold before dawn.",
        )
        .unwrap();

        let manifest = Manifest {
            meta: ManifestMeta {
                last_scan: "2026-01-01T00:00:00Z".to_string(),
                classification_model: "test".to_string(),
            },
            story_files: vec![
                StoryFile {
                    path: "chapters/one.md".to_string(),
                    format: "prose".to_string(),
                    order: 1,
                    content_hash: "h1".to_string(),
                },
                StoryFile {
                    path: "chapters/two.md".to_string(),
                    format: "prose".to_string(),
                    order: 2,
                    content_hash: "h2".to_string(),
                },
            ],
            context_files: vec![],
            excluded: vec![],
        };
        manifest.save(root).unwrap();
    }

    #[test]
    fn loads_manifest_project_with_cloud_permissions_and_aggregate_summary() {
        let tmp = tempfile::tempdir().unwrap();
        write_config(tmp.path(), "anthropic", "claude-sonnet-4-6", vec!["story_grep"]);
        write_manifest_project(tmp.path());
        write_graph(tmp.path());
        write_overrides(tmp.path());
        write_stale_perspectives(tmp.path());

        let result = load_project(tmp.path()).unwrap();
        let tool_names: Vec<String> = result
            .skills
            .tool_schemas_for_context(crate::concepts::skills::SkillSetContext::Chat)
            .into_iter()
            .map(|tool| tool.name)
            .collect();

        assert_eq!(result.summary.privacy_label, "Cloud");
        assert_eq!(result.summary.model_name, "claude-sonnet-4-6");
        assert_eq!(result.summary.scene_count, 3);
        assert_eq!(result.summary.char_count, 1);
        assert!(result.project.manifest.is_some());
        assert_eq!(
            result
                .project
                .intent
                .list_declarations()
                .len(),
            1
        );
        assert!(result.project.file_buffer_manager.is_some());
        assert_eq!(
            result.summary.word_count,
            result
                .project
                .file_buffer_manager
                .as_ref()
                .unwrap()
                .total_word_count()
        );
        assert!(!tool_names.iter().any(|name| name == "story_grep"));
        assert!(result
            .project
            .perspectives
            .get_perspective("char-marcus")
            .is_none());
    }

    #[test]
    fn loads_primary_story_when_manifest_is_absent() {
        let tmp = tempfile::tempdir().unwrap();
        write_config(tmp.path(), "local", "llama3", vec!["story_grep"]);
        std::fs::create_dir_all(tmp.path().join(LAIRES_DIR)).unwrap();
        std::fs::write(
            tmp.path().join("story.md"),
            "## One\n\nMarcus arrives alone.\n\n## Two\n\nElena answers after midnight.",
        )
        .unwrap();
        write_graph(tmp.path());

        let result = load_project(tmp.path()).unwrap();
        let tool_names: Vec<String> = result
            .skills
            .tool_schemas_for_context(crate::concepts::skills::SkillSetContext::Chat)
            .into_iter()
            .map(|tool| tool.name)
            .collect();

        assert_eq!(result.summary.privacy_label, "Local");
        assert_eq!(result.summary.scene_count, 2);
        assert_eq!(result.summary.char_count, 1);
        assert!(result.project.manifest.is_none());
        assert!(result.project.file_buffer_manager.is_none());
        assert!(tool_names.iter().any(|name| name == "story_grep"));
    }
}
