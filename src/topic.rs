use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use ts_rs::TS;

const TOPICS_DIR: &str = ".aipair/topics";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../web/src/types/")]
#[serde(rename_all = "lowercase")]
pub enum TopicStatus {
    Active,
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct Topic {
    pub id: String,
    pub name: String,
    pub base: String,
    pub changes: HashSet<String>,
    pub status: TopicStatus,
    pub created_at: DateTime<Utc>,
}

pub struct TopicStore {
    base_path: PathBuf,
}

impl TopicStore {
    pub fn new(repo_path: impl AsRef<Path>) -> Self {
        Self {
            base_path: repo_path.as_ref().join(TOPICS_DIR),
        }
    }

    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.base_path)?;
        Ok(())
    }

    fn topic_dir(&self, topic_id: &str) -> PathBuf {
        self.base_path.join(topic_id)
    }

    fn topic_json_path(&self, topic_id: &str) -> PathBuf {
        self.topic_dir(topic_id).join("topic.json")
    }

    fn notes_path(&self, topic_id: &str) -> PathBuf {
        self.topic_dir(topic_id).join("notes.md")
    }

    pub fn get(&self, topic_id: &str) -> Result<Option<Topic>> {
        let path = self.topic_json_path(topic_id);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read topic file: {}", path.display()))?;
        let topic: Topic = serde_json::from_str(&content)?;
        Ok(Some(topic))
    }

    pub fn save(&self, topic: &Topic) -> Result<()> {
        let dir = self.topic_dir(&topic.id);
        std::fs::create_dir_all(&dir)?;
        let path = self.topic_json_path(&topic.id);
        let content = serde_json::to_string_pretty(topic)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn create(&self, id: &str, name: &str, base: &str) -> Result<Topic> {
        if self.get(id)?.is_some() {
            anyhow::bail!("Topic already exists: {}", id);
        }

        let topic = Topic {
            id: id.to_string(),
            name: name.to_string(),
            base: base.to_string(),
            changes: HashSet::new(),
            status: TopicStatus::Active,
            created_at: Utc::now(),
        };

        self.save(&topic)?;
        Ok(topic)
    }

    pub fn get_notes(&self, topic_id: &str) -> Result<String> {
        let path = self.notes_path(topic_id);
        if !path.exists() {
            return Ok(String::new());
        }
        Ok(std::fs::read_to_string(&path)?)
    }

    pub fn set_notes(&self, topic_id: &str, notes: &str) -> Result<()> {
        let dir = self.topic_dir(topic_id);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(self.notes_path(topic_id), notes)?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Topic>> {
        if !self.base_path.exists() {
            return Ok(Vec::new());
        }

        let mut topics = Vec::new();
        for entry in std::fs::read_dir(&self.base_path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let json_path = entry.path().join("topic.json");
                if json_path.exists() {
                    let content = std::fs::read_to_string(&json_path)?;
                    if let Ok(topic) = serde_json::from_str::<Topic>(&content) {
                        topics.push(topic);
                    }
                }
            }
        }

        topics.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(topics)
    }

    /// Find which topic a change belongs to, if any. Supports prefix matching.
    pub fn find_topic_for_change(&self, change_id: &str) -> Result<Option<String>> {
        for topic in self.list()? {
            if resolve_change_in_set(&topic.changes, change_id).is_some() {
                return Ok(Some(topic.id.clone()));
            }
        }
        Ok(None)
    }

    /// Add changes to a topic, enforcing single-topic-per-change.
    /// Change IDs should already be resolved to full IDs by the caller.
    pub fn add_changes(&self, topic_id: &str, change_ids: &[String]) -> Result<Topic> {
        let mut topic = self
            .get(topic_id)?
            .ok_or_else(|| anyhow::anyhow!("Topic not found: {}", topic_id))?;

        // Check that none of these changes belong to another topic
        for change_id in change_ids {
            if let Some(existing_topic) = self.find_topic_for_change(change_id)? {
                if existing_topic != topic_id {
                    anyhow::bail!(
                        "Change {} already belongs to topic '{}'",
                        change_id,
                        existing_topic
                    );
                }
            }
            topic.changes.insert(change_id.clone());
        }

        self.save(&topic)?;
        Ok(topic)
    }

    /// Remove changes from a topic. Supports prefix matching against stored IDs.
    pub fn remove_changes(&self, topic_id: &str, change_ids: &[String]) -> Result<Topic> {
        let mut topic = self
            .get(topic_id)?
            .ok_or_else(|| anyhow::anyhow!("Topic not found: {}", topic_id))?;

        for change_id in change_ids {
            let full_id = resolve_change_in_set(&topic.changes, change_id)
                .ok_or_else(|| anyhow::anyhow!("Change {} not found in topic '{}'", change_id, topic_id))?;
            topic.changes.remove(&full_id);
        }

        self.save(&topic)?;
        Ok(topic)
    }

    /// Set topic status to Finished.
    pub fn finish(&self, topic_id: &str) -> Result<Topic> {
        let mut topic = self
            .get(topic_id)?
            .ok_or_else(|| anyhow::anyhow!("Topic not found: {}", topic_id))?;

        topic.status = TopicStatus::Finished;
        self.save(&topic)?;
        Ok(topic)
    }
}

/// Resolve a possibly-short change ID against a set of full IDs.
/// Returns the matching full ID, or None if not found.
/// Errors are not returned here — ambiguous matches return None (callers can provide better errors).
fn resolve_change_in_set(changes: &HashSet<String>, prefix: &str) -> Option<String> {
    // Exact match first
    if changes.contains(prefix) {
        return Some(prefix.to_string());
    }
    // Prefix match
    let matches: Vec<_> = changes.iter().filter(|id| id.starts_with(prefix)).collect();
    if matches.len() == 1 {
        Some(matches[0].clone())
    } else {
        None
    }
}

/// Generate a slug from a human-readable name.
pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, TopicStore) {
        let dir = TempDir::new().unwrap();
        let store = TopicStore::new(dir.path());
        store.init().unwrap();
        (dir, store)
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Fix auth flow"), "fix-auth-flow");
        assert_eq!(slugify("  Multiple   Spaces  "), "multiple-spaces");
        assert_eq!(slugify("kebab-case"), "kebab-case");
    }

    #[test]
    fn test_create_and_get_topic() {
        let (_dir, store) = setup();
        let topic = store.create("auth-flow", "Fix auth flow", "base123").unwrap();
        assert_eq!(topic.id, "auth-flow");
        assert_eq!(topic.name, "Fix auth flow");
        assert_eq!(topic.base, "base123");
        assert!(topic.changes.is_empty());
        assert_eq!(topic.status, TopicStatus::Active);

        let fetched = store.get("auth-flow").unwrap().unwrap();
        assert_eq!(fetched.id, "auth-flow");
    }

    #[test]
    fn test_create_duplicate_fails() {
        let (_dir, store) = setup();
        store.create("auth-flow", "Fix auth flow", "base123").unwrap();
        assert!(store.create("auth-flow", "Fix auth flow", "base123").is_err());
    }

    #[test]
    fn test_add_and_remove_changes() {
        let (_dir, store) = setup();
        store.create("auth-flow", "Fix auth flow", "base123").unwrap();

        let topic = store.add_changes("auth-flow", &["change1".to_string(), "change2".to_string()]).unwrap();
        assert_eq!(topic.changes.len(), 2);
        assert!(topic.changes.contains("change1"));

        let topic = store.remove_changes("auth-flow", &["change1".to_string()]).unwrap();
        assert_eq!(topic.changes.len(), 1);
        assert!(!topic.changes.contains("change1"));
    }

    #[test]
    fn test_single_topic_per_change_enforcement() {
        let (_dir, store) = setup();
        store.create("topic-a", "Topic A", "base1").unwrap();
        store.create("topic-b", "Topic B", "base2").unwrap();

        store.add_changes("topic-a", &["change1".to_string()]).unwrap();

        // Adding the same change to topic-b should fail
        let result = store.add_changes("topic-b", &["change1".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already belongs to topic"));
    }

    #[test]
    fn test_notes() {
        let (_dir, store) = setup();
        store.create("auth-flow", "Fix auth flow", "base123").unwrap();

        assert_eq!(store.get_notes("auth-flow").unwrap(), "");

        store.set_notes("auth-flow", "# Plan\n- Step 1\n- Step 2").unwrap();
        assert_eq!(store.get_notes("auth-flow").unwrap(), "# Plan\n- Step 1\n- Step 2");
    }

    #[test]
    fn test_finish_topic() {
        let (_dir, store) = setup();
        store.create("auth-flow", "Fix auth flow", "base123").unwrap();

        let topic = store.finish("auth-flow").unwrap();
        assert_eq!(topic.status, TopicStatus::Finished);
    }

    #[test]
    fn test_list_topics() {
        let (_dir, store) = setup();
        store.create("topic-a", "Topic A", "base1").unwrap();
        store.create("topic-b", "Topic B", "base2").unwrap();

        let topics = store.list().unwrap();
        assert_eq!(topics.len(), 2);
    }

    #[test]
    fn test_find_topic_for_change() {
        let (_dir, store) = setup();
        store.create("auth-flow", "Fix auth flow", "base123").unwrap();
        store.add_changes("auth-flow", &["abcdef123456".to_string()]).unwrap();

        // Exact match
        assert_eq!(store.find_topic_for_change("abcdef123456").unwrap(), Some("auth-flow".to_string()));
        // Prefix match
        assert_eq!(store.find_topic_for_change("abcdef").unwrap(), Some("auth-flow".to_string()));
        // No match
        assert_eq!(store.find_topic_for_change("unknown").unwrap(), None);
    }

    #[test]
    fn test_remove_changes_by_prefix() {
        let (_dir, store) = setup();
        store.create("auth-flow", "Fix auth flow", "base123").unwrap();
        store.add_changes("auth-flow", &["abcdef123456".to_string(), "xyz789000000".to_string()]).unwrap();

        // Remove by prefix
        let topic = store.remove_changes("auth-flow", &["abcdef".to_string()]).unwrap();
        assert_eq!(topic.changes.len(), 1);
        assert!(!topic.changes.contains("abcdef123456"));
        assert!(topic.changes.contains("xyz789000000"));
    }

    #[test]
    fn test_remove_nonexistent_change_fails() {
        let (_dir, store) = setup();
        store.create("auth-flow", "Fix auth flow", "base123").unwrap();

        let result = store.remove_changes("auth-flow", &["nonexistent".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found in topic"));
    }

    #[test]
    fn test_single_topic_enforcement_with_prefix() {
        let (_dir, store) = setup();
        store.create("topic-a", "Topic A", "base1").unwrap();
        store.create("topic-b", "Topic B", "base2").unwrap();

        store.add_changes("topic-a", &["abcdef123456".to_string()]).unwrap();

        // Adding the same full ID to topic-b should fail
        let result = store.add_changes("topic-b", &["abcdef123456".to_string()]);
        assert!(result.is_err());
    }
}
