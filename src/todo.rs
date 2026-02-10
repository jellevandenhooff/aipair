use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use ts_rs::TS;

const TODOS_FILE: &str = ".aipair/todos.json";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    pub checked: bool,
    pub children: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct TodoTree {
    pub root_ids: Vec<String>,
    pub items: HashMap<String, TodoItem>,
}

impl Default for TodoTree {
    fn default() -> Self {
        Self {
            root_ids: Vec::new(),
            items: HashMap::new(),
        }
    }
}

pub struct TodoStore {
    file_path: PathBuf,
}

impl TodoStore {
    pub fn new(repo_path: impl AsRef<Path>) -> Self {
        Self {
            file_path: repo_path.as_ref().join(TODOS_FILE),
        }
    }

    pub fn load(&self) -> Result<TodoTree> {
        if !self.file_path.exists() {
            return Ok(TodoTree::default());
        }
        let content = std::fs::read_to_string(&self.file_path)?;
        let tree: TodoTree = serde_json::from_str(&content)?;
        Ok(tree)
    }

    pub fn save(&self, tree: &TodoTree) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(tree)?;
        std::fs::write(&self.file_path, content)?;
        Ok(())
    }

    pub fn add_item(
        &self,
        tree: &mut TodoTree,
        text: String,
        parent_id: Option<&str>,
        after_id: Option<&str>,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();

        let item = TodoItem {
            id: id.clone(),
            text,
            checked: false,
            children: Vec::new(),
            topic_id: None,
            created_at: Utc::now(),
        };

        tree.items.insert(id.clone(), item);

        // Insert into parent's children or root_ids
        let siblings = match parent_id {
            Some(pid) => {
                let parent = tree
                    .items
                    .get_mut(pid)
                    .ok_or_else(|| anyhow::anyhow!("Parent not found: {}", pid))?;
                &mut parent.children
            }
            None => &mut tree.root_ids,
        };

        match after_id {
            Some(aid) => {
                if let Some(pos) = siblings.iter().position(|s| s == aid) {
                    siblings.insert(pos + 1, id.clone());
                } else {
                    siblings.push(id.clone());
                }
            }
            None => {
                // Insert at the beginning
                siblings.insert(0, id.clone());
            }
        }

        self.save(tree)?;
        Ok(id)
    }

    pub fn update_item(
        &self,
        tree: &mut TodoTree,
        id: &str,
        text: Option<String>,
        checked: Option<bool>,
    ) -> Result<()> {
        let item = tree
            .items
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Item not found: {}", id))?;

        if let Some(t) = text {
            item.text = t;
        }
        if let Some(c) = checked {
            item.checked = c;
        }

        self.save(tree)?;
        Ok(())
    }

    pub fn toggle_item(&self, tree: &mut TodoTree, id: &str) -> Result<bool> {
        let item = tree
            .items
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Item not found: {}", id))?;

        item.checked = !item.checked;
        let new_state = item.checked;

        self.save(tree)?;
        Ok(new_state)
    }

    pub fn delete_item(&self, tree: &mut TodoTree, id: &str) -> Result<()> {
        // Collect all descendant ids to remove
        let mut to_remove = vec![id.to_string()];
        let mut i = 0;
        while i < to_remove.len() {
            if let Some(item) = tree.items.get(&to_remove[i]) {
                to_remove.extend(item.children.clone());
            }
            i += 1;
        }

        // Remove from parent's children or root_ids
        tree.root_ids.retain(|r| r != id);
        for item in tree.items.values_mut() {
            item.children.retain(|c| c != id);
        }

        // Remove all descendants
        for rid in &to_remove {
            tree.items.remove(rid);
        }

        self.save(tree)?;
        Ok(())
    }

    pub fn move_item(
        &self,
        tree: &mut TodoTree,
        id: &str,
        new_parent_id: Option<&str>,
        after_id: Option<&str>,
    ) -> Result<()> {
        // Verify item exists
        if !tree.items.contains_key(id) {
            anyhow::bail!("Item not found: {}", id);
        }

        // Remove from current location
        tree.root_ids.retain(|r| r != id);
        // Need to collect keys first to avoid borrow issues
        let keys: Vec<String> = tree.items.keys().cloned().collect();
        for key in &keys {
            if key != id {
                if let Some(item) = tree.items.get_mut(key) {
                    item.children.retain(|c| c != id);
                }
            }
        }

        // Insert into new location
        let siblings = match new_parent_id {
            Some(pid) => {
                let parent = tree
                    .items
                    .get_mut(pid)
                    .ok_or_else(|| anyhow::anyhow!("Parent not found: {}", pid))?;
                &mut parent.children
            }
            None => &mut tree.root_ids,
        };

        match after_id {
            Some(aid) => {
                if let Some(pos) = siblings.iter().position(|s| s == aid) {
                    siblings.insert(pos + 1, id.to_string());
                } else {
                    siblings.push(id.to_string());
                }
            }
            None => {
                siblings.insert(0, id.to_string());
            }
        }

        self.save(tree)?;
        Ok(())
    }

    /// Sync topic items: ensures each active topic has a corresponding todo item,
    /// and auto-checks items for finished topics.
    pub fn sync_topics(
        &self,
        tree: &mut TodoTree,
        topics: &[crate::topic::Topic],
    ) -> Result<bool> {
        let mut changed = false;

        for topic in topics {
            // Find existing item for this topic
            let existing_id = tree
                .items
                .iter()
                .find(|(_, item)| item.topic_id.as_deref() == Some(&topic.id))
                .map(|(id, _)| id.clone());

            match existing_id {
                Some(id) => {
                    // Update checked state based on topic status
                    let is_finished = topic.status == crate::topic::TopicStatus::Finished;
                    let item = tree.items.get_mut(&id).unwrap();
                    if item.checked != is_finished {
                        item.checked = is_finished;
                        changed = true;
                    }
                    // Update name if it changed
                    if item.text != topic.name {
                        item.text = topic.name.clone();
                        changed = true;
                    }
                }
                None => {
                    // Create new item for this topic
                    let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
                    let is_finished = topic.status == crate::topic::TopicStatus::Finished;
                    let item = TodoItem {
                        id: id.clone(),
                        text: topic.name.clone(),
                        checked: is_finished,
                        children: Vec::new(),
                        topic_id: Some(topic.id.clone()),
                        created_at: Utc::now(),
                    };
                    tree.items.insert(id.clone(), item);
                    tree.root_ids.push(id);
                    changed = true;
                }
            }
        }

        if changed {
            self.save(tree)?;
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, TodoStore) {
        let dir = TempDir::new().unwrap();
        // Create the .aipair directory
        std::fs::create_dir_all(dir.path().join(".aipair")).unwrap();
        let store = TodoStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn test_load_empty() {
        let (_dir, store) = setup();
        let tree = store.load().unwrap();
        assert!(tree.root_ids.is_empty());
        assert!(tree.items.is_empty());
    }

    #[test]
    fn test_add_item() {
        let (_dir, store) = setup();
        let mut tree = store.load().unwrap();

        let id = store.add_item(&mut tree, "First task".to_string(), None, None).unwrap();
        assert_eq!(tree.root_ids.len(), 1);
        assert_eq!(tree.items[&id].text, "First task");
        assert!(!tree.items[&id].checked);
    }

    #[test]
    fn test_add_child_item() {
        let (_dir, store) = setup();
        let mut tree = store.load().unwrap();

        let parent_id = store.add_item(&mut tree, "Parent".to_string(), None, None).unwrap();
        let child_id = store
            .add_item(&mut tree, "Child".to_string(), Some(&parent_id), None)
            .unwrap();

        assert_eq!(tree.root_ids.len(), 1);
        assert_eq!(tree.items[&parent_id].children, vec![child_id.clone()]);
        assert_eq!(tree.items[&child_id].text, "Child");
    }

    #[test]
    fn test_add_item_after() {
        let (_dir, store) = setup();
        let mut tree = store.load().unwrap();

        let id1 = store.add_item(&mut tree, "First".to_string(), None, None).unwrap();
        let id2 = store.add_item(&mut tree, "Third".to_string(), None, Some(&id1)).unwrap();
        let id3 = store.add_item(&mut tree, "Second".to_string(), None, Some(&id1)).unwrap();

        assert_eq!(tree.root_ids, vec![id1, id3, id2]);
    }

    #[test]
    fn test_update_item() {
        let (_dir, store) = setup();
        let mut tree = store.load().unwrap();

        let id = store.add_item(&mut tree, "Original".to_string(), None, None).unwrap();
        store.update_item(&mut tree, &id, Some("Updated".to_string()), None).unwrap();

        assert_eq!(tree.items[&id].text, "Updated");
    }

    #[test]
    fn test_toggle_item() {
        let (_dir, store) = setup();
        let mut tree = store.load().unwrap();

        let id = store.add_item(&mut tree, "Task".to_string(), None, None).unwrap();
        assert!(!tree.items[&id].checked);

        let checked = store.toggle_item(&mut tree, &id).unwrap();
        assert!(checked);
        assert!(tree.items[&id].checked);

        let checked = store.toggle_item(&mut tree, &id).unwrap();
        assert!(!checked);
    }

    #[test]
    fn test_delete_item_with_children() {
        let (_dir, store) = setup();
        let mut tree = store.load().unwrap();

        let parent_id = store.add_item(&mut tree, "Parent".to_string(), None, None).unwrap();
        let child_id = store
            .add_item(&mut tree, "Child".to_string(), Some(&parent_id), None)
            .unwrap();

        store.delete_item(&mut tree, &parent_id).unwrap();
        assert!(tree.root_ids.is_empty());
        assert!(!tree.items.contains_key(&parent_id));
        assert!(!tree.items.contains_key(&child_id));
    }

    #[test]
    fn test_move_item() {
        let (_dir, store) = setup();
        let mut tree = store.load().unwrap();

        let id1 = store.add_item(&mut tree, "Item 1".to_string(), None, None).unwrap();
        let id2 = store.add_item(&mut tree, "Item 2".to_string(), None, Some(&id1)).unwrap();

        // Move id2 under id1 as a child
        store.move_item(&mut tree, &id2, Some(&id1), None).unwrap();
        assert_eq!(tree.root_ids, vec![id1.clone()]);
        assert_eq!(tree.items[&id1].children, vec![id2]);
    }

    #[test]
    fn test_persistence() {
        let (_dir, store) = setup();
        let mut tree = store.load().unwrap();

        let id = store.add_item(&mut tree, "Persisted".to_string(), None, None).unwrap();

        // Load again from disk
        let tree2 = store.load().unwrap();
        assert_eq!(tree2.root_ids.len(), 1);
        assert_eq!(tree2.items[&id].text, "Persisted");
    }

    #[test]
    fn test_sync_topics() {
        let (_dir, store) = setup();
        let mut tree = store.load().unwrap();

        let topics = vec![crate::topic::Topic {
            id: "auth-flow".to_string(),
            name: "Fix auth flow".to_string(),
            base: "base123".to_string(),
            changes: std::collections::HashSet::new(),
            status: crate::topic::TopicStatus::Active,
            created_at: Utc::now(),
        }];

        let changed = store.sync_topics(&mut tree, &topics).unwrap();
        assert!(changed);
        assert_eq!(tree.root_ids.len(), 1);

        let topic_item = tree.items.values().find(|i| i.topic_id.as_deref() == Some("auth-flow")).unwrap();
        assert_eq!(topic_item.text, "Fix auth flow");
        assert!(!topic_item.checked);

        // Sync again with finished topic
        let topics = vec![crate::topic::Topic {
            id: "auth-flow".to_string(),
            name: "Fix auth flow".to_string(),
            base: "base123".to_string(),
            changes: std::collections::HashSet::new(),
            status: crate::topic::TopicStatus::Finished,
            created_at: Utc::now(),
        }];

        let changed = store.sync_topics(&mut tree, &topics).unwrap();
        assert!(changed);

        let topic_item = tree.items.values().find(|i| i.topic_id.as_deref() == Some("auth-flow")).unwrap();
        assert!(topic_item.checked);
    }
}
