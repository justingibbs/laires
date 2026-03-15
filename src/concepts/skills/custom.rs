use std::io::Write;
use std::path::Path;

use serde::Deserialize;

use crate::concepts::provider::{Message, Provider, Role};

use super::{CustomSkillData, SkillCategory, SkillDefinition, Skills};

/// TOML file format for custom skill definitions.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Deserialize)]
pub(super) struct CustomSkillToml {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) prompt_template: String,
    pub(super) input_schema: toml::Value,
    #[serde(default)]
    pub(super) output_schema: Option<toml::Value>,
}

impl Skills {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn load_custom_skills(&mut self, skills_dir: &Path) -> Vec<String> {
        let mut errors = Vec::new();

        if !skills_dir.is_dir() {
            return errors;
        }

        let entries = match std::fs::read_dir(skills_dir) {
            Ok(e) => e,
            Err(e) => {
                errors.push(format!("Failed to read skills directory: {e}"));
                return errors;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(format!("Failed to read directory entry: {e}"));
                    continue;
                }
            };

            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(format!("{}: failed to read: {e}", path.display()));
                    continue;
                }
            };

            let skill_toml: CustomSkillToml = match toml::from_str(&content) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(format!("{}: invalid TOML: {e}", path.display()));
                    continue;
                }
            };

            if skill_toml.name.is_empty() {
                errors.push(format!("{}: name cannot be empty", path.display()));
                continue;
            }
            if skill_toml.prompt_template.is_empty() {
                errors.push(format!(
                    "{}: prompt_template cannot be empty",
                    path.display()
                ));
                continue;
            }

            let input_schema = toml_to_json(skill_toml.input_schema);
            let _output_schema = skill_toml.output_schema.map(toml_to_json);

            self.register(SkillDefinition {
                name: skill_toml.name.clone(),
                description: skill_toml.description,
                category: SkillCategory::CustomTools,
                input_schema,
            });

            self.custom_data.insert(
                skill_toml.name,
                CustomSkillData {
                    prompt_template: skill_toml.prompt_template,
                },
            );
        }

        errors
    }

    pub(super) async fn exec_custom_skill(
        &self,
        skill_name: &str,
        args: &serde_json::Value,
        provider: Option<&mut Provider>,
    ) -> serde_json::Value {
        let data = match self.custom_data.get(skill_name) {
            Some(d) => d,
            None => {
                return serde_json::json!({
                    "error": format!("Custom skill data not found: {skill_name}")
                })
            }
        };

        let provider = match provider {
            Some(p) => p,
            None => {
                return serde_json::json!({
                    "error": "LLM provider required for custom skills"
                })
            }
        };

        let mut prompt = data.prompt_template.clone();
        if let Some(obj) = args.as_object() {
            for (key, value) in obj {
                let placeholder = format!("{{{{{key}}}}}");
                let replacement = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                prompt = prompt.replace(&placeholder, &replacement);
            }
        }

        let messages = vec![Message {
            role: Role::User,
            content: prompt,
            tool_calls: None,
            tool_results: None,
        }];

        match provider.complete(&messages, &[], None).await {
            Ok(response) => {
                let content = response.content.unwrap_or_default();
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(json) => json,
                    Err(_) => serde_json::json!({ "response": content }),
                }
            }
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    }

    pub(super) fn write_audit_log(
        &self,
        project_root: &Path,
        skill_name: &str,
        args: &serde_json::Value,
        result: &serde_json::Value,
        duration_ms: u64,
    ) {
        let log_path = project_root
            .join(crate::config::LAIRES_DIR)
            .join(crate::config::SKILL_LOG_FILE);
        let entry = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "skill": skill_name,
            "args": args,
            "result": result,
            "duration_ms": duration_ms,
        });
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = writeln!(file, "{}", entry);
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn toml_to_json(val: toml::Value) -> serde_json::Value {
    match val {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(map) => {
            let obj: serde_json::Map<String, serde_json::Value> =
                map.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect();
            serde_json::Value::Object(obj)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}
