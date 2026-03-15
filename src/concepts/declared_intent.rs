use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::concepts::narrative_graph::NodeId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeclKey {
    pub node_id: NodeId,
    pub field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Declaration {
    pub value: String,
    pub rationale: Option<String>,
    pub timestamp: DateTime<Utc>,
}

pub struct DeclaredIntent {
    declarations: HashMap<DeclKey, Declaration>,
}

impl DeclaredIntent {
    pub fn new() -> Self {
        Self {
            declarations: HashMap::new(),
        }
    }

    /// Set or update a writer override
    pub fn declare(
        &mut self,
        node_id: &str,
        field: &str,
        value: String,
        rationale: Option<String>,
    ) {
        let key = DeclKey {
            node_id: node_id.to_string(),
            field: field.to_string(),
        };
        self.declarations.insert(
            key,
            Declaration {
                value,
                rationale,
                timestamp: Utc::now(),
            },
        );
    }

    /// Remove a writer override
    pub fn retract(&mut self, node_id: &str, field: &str) -> bool {
        let key = DeclKey {
            node_id: node_id.to_string(),
            field: field.to_string(),
        };
        self.declarations.remove(&key).is_some()
    }

    /// List all declarations
    pub fn list_declarations(&self) -> Vec<(&DeclKey, &Declaration)> {
        self.declarations.iter().collect()
    }

    /// Get a specific declaration
    #[allow(dead_code)]
    pub fn get_declaration(&self, node_id: &str, field: &str) -> Option<&Declaration> {
        let key = DeclKey {
            node_id: node_id.to_string(),
            field: field.to_string(),
        };
        self.declarations.get(&key)
    }

    /// Find declarations referencing nodes that no longer exist
    pub fn find_orphans(&self, valid_node_ids: &HashSet<String>) -> Vec<DeclKey> {
        self.declarations
            .keys()
            .filter(|key| !valid_node_ids.contains(&key.node_id))
            .cloned()
            .collect()
    }

    /// Check if there are any declarations
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }

    /// Get the number of declarations
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.declarations.len()
    }

    /// Save to disk as JSON
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let entries: Vec<SerializedDecl> = self
            .declarations
            .iter()
            .map(|(key, decl)| SerializedDecl {
                node_id: key.node_id.clone(),
                field: key.field.clone(),
                value: decl.value.clone(),
                rationale: decl.rationale.clone(),
                timestamp: decl.timestamp,
            })
            .collect();
        let json = serde_json::to_string_pretty(&entries)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load from disk
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let entries: Vec<SerializedDecl> = serde_json::from_str(&content)?;
        let mut intent = Self::new();
        for entry in entries {
            let key = DeclKey {
                node_id: entry.node_id,
                field: entry.field,
            };
            intent.declarations.insert(
                key,
                Declaration {
                    value: entry.value,
                    rationale: entry.rationale,
                    timestamp: entry.timestamp,
                },
            );
        }
        Ok(intent)
    }
}

impl Default for DeclaredIntent {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SerializedDecl {
    node_id: String,
    field: String,
    value: String,
    rationale: Option<String>,
    timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_declare_and_get() {
        let mut intent = DeclaredIntent::new();
        intent.declare("node1", "description", "Revenge".to_string(), Some("Writer override".to_string()));

        let decl = intent.get_declaration("node1", "description").unwrap();
        assert_eq!(decl.value, "Revenge");
        assert_eq!(decl.rationale.as_deref(), Some("Writer override"));
    }

    #[test]
    fn test_retract() {
        let mut intent = DeclaredIntent::new();
        intent.declare("node1", "description", "Revenge".to_string(), None);
        assert_eq!(intent.len(), 1);

        let removed = intent.retract("node1", "description");
        assert!(removed);
        assert_eq!(intent.len(), 0);
        assert!(intent.get_declaration("node1", "description").is_none());

        // Retracting nonexistent returns false
        assert!(!intent.retract("node1", "description"));
    }

    #[test]
    fn test_find_orphans() {
        let mut intent = DeclaredIntent::new();
        intent.declare("alive", "description", "Exists".to_string(), None);
        intent.declare("dead", "description", "Gone".to_string(), None);
        intent.declare("dead", "status", "Also gone".to_string(), None);

        let valid: HashSet<String> = ["alive".to_string()].into_iter().collect();
        let orphans = intent.find_orphans(&valid);

        assert_eq!(orphans.len(), 2);
        assert!(orphans.iter().all(|k| k.node_id == "dead"));
    }

    #[test]
    fn test_save_load_roundtrip() {
        let mut intent = DeclaredIntent::new();
        intent.declare("n1", "desc", "Value 1".to_string(), Some("Reason".to_string()));
        intent.declare("n2", "status", "Active".to_string(), None);

        let dir = std::env::temp_dir().join("laires_test_decl");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("overrides.json");

        intent.save(&path).unwrap();
        let loaded = DeclaredIntent::load(&path).unwrap();

        assert_eq!(loaded.len(), 2);
        let decl = loaded.get_declaration("n1", "desc").unwrap();
        assert_eq!(decl.value, "Value 1");
        assert_eq!(decl.rationale.as_deref(), Some("Reason"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_list_declarations() {
        let mut intent = DeclaredIntent::new();
        intent.declare("n1", "f1", "v1".to_string(), None);
        intent.declare("n2", "f2", "v2".to_string(), None);

        let list = intent.list_declarations();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_declare_overwrites() {
        let mut intent = DeclaredIntent::new();
        intent.declare("n1", "desc", "First".to_string(), None);
        intent.declare("n1", "desc", "Second".to_string(), None);

        assert_eq!(intent.len(), 1);
        let decl = intent.get_declaration("n1", "desc").unwrap();
        assert_eq!(decl.value, "Second");
    }
}
