use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;
use uuid::Uuid;

const REVIEWS_DIR: &str = ".aipair/reviews";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct Revision {
    pub number: u32,
    pub commit_id: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub is_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct Review {
    pub change_id: String,
    pub base: String,
    pub created_at: DateTime<Utc>,
    pub threads: Vec<Thread>,
    #[serde(default)]
    pub revisions: Vec<Revision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_commit_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct Thread {
    pub id: String,
    pub file: String,
    pub line_start: usize,
    pub line_end: usize,
    pub status: ThreadStatus,
    pub comments: Vec<Comment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_revision: Option<u32>,
    /// Display position after mapping through diffs (not persisted)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_line_start: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_line_end: Option<usize>,
    #[serde(default)]
    pub is_displaced: bool,
    #[serde(default)]
    pub is_deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../web/src/types/")]
#[serde(rename_all = "lowercase")]
pub enum ThreadStatus {
    Open,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct Comment {
    pub author: Author,
    pub text: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../web/src/types/")]
#[serde(rename_all = "lowercase")]
pub enum Author {
    User,
    Claude,
}

pub struct ReviewStore {
    base_path: PathBuf,
}

impl ReviewStore {
    pub fn new(repo_path: impl AsRef<Path>) -> Self {
        Self {
            base_path: repo_path.as_ref().join(REVIEWS_DIR),
        }
    }

    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.base_path)?;
        Ok(())
    }

    fn review_path(&self, change_id: &str) -> PathBuf {
        self.base_path.join(format!("{change_id}.json"))
    }

    pub fn get(&self, change_id: &str) -> Result<Option<Review>> {
        let path = self.review_path(change_id);
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read review file: {}", path.display()))?;
        let review: Review = serde_json::from_str(&content)?;
        Ok(Some(review))
    }

    /// Get a review by change_id prefix (supports short IDs like "zwlsqumm")
    pub fn get_by_prefix(&self, prefix: &str) -> Result<Option<Review>> {
        // First try exact match
        if let Some(review) = self.get(prefix)? {
            return Ok(Some(review));
        }

        // Search for files matching the prefix
        let entries = std::fs::read_dir(&self.base_path)?;
        let mut matches = Vec::new();

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(prefix) && name_str.ends_with(".json") {
                matches.push(entry.path());
            }
        }

        match matches.len() {
            0 => Ok(None),
            1 => {
                let content = std::fs::read_to_string(&matches[0])?;
                let review: Review = serde_json::from_str(&content)?;
                Ok(Some(review))
            }
            _ => anyhow::bail!("Ambiguous change_id prefix '{}': matches {} reviews", prefix, matches.len()),
        }
    }

    pub fn save(&self, review: &Review) -> Result<()> {
        self.init()?;
        let path = self.review_path(&review.change_id);
        let content = serde_json::to_string_pretty(review)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn get_or_create(&self, change_id: &str, base: &str, commit_id: &str) -> Result<Review> {
        if let Some(mut review) = self.get(change_id)? {
            // Update working_commit_id if not set (migration for old reviews)
            if review.working_commit_id.is_none() {
                review.working_commit_id = Some(commit_id.to_string());
                self.save(&review)?;
            }
            return Ok(review);
        }

        let review = Review {
            change_id: change_id.to_string(),
            base: base.to_string(),
            created_at: Utc::now(),
            threads: Vec::new(),
            revisions: Vec::new(),
            working_commit_id: Some(commit_id.to_string()),
        };

        self.save(&review)?;
        Ok(review)
    }

    pub fn record_revision(
        &self,
        change_id: &str,
        commit_id: &str,
        description: Option<String>,
    ) -> Result<(Review, u32)> {
        let mut review = self
            .get(change_id)?
            .ok_or_else(|| anyhow::anyhow!("Review not found for change: {}", change_id))?;

        let number = review.revisions.len() as u32 + 1;
        review.revisions.push(Revision {
            number,
            commit_id: commit_id.to_string(),
            created_at: Utc::now(),
            description,
            is_pending: false,
        });
        review.working_commit_id = Some(commit_id.to_string());

        self.save(&review)?;
        Ok((review, number))
    }

    pub fn list(&self) -> Result<Vec<Review>> {
        if !self.base_path.exists() {
            return Ok(Vec::new());
        }

        let mut reviews = Vec::new();
        for entry in std::fs::read_dir(&self.base_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let content = std::fs::read_to_string(&path)?;
                if let Ok(review) = serde_json::from_str::<Review>(&content) {
                    reviews.push(review);
                }
            }
        }

        // Sort by created_at descending
        reviews.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(reviews)
    }

    pub fn list_with_open_threads(&self) -> Result<Vec<Review>> {
        let reviews = self.list()?;
        Ok(reviews
            .into_iter()
            .filter(|r| r.threads.iter().any(|t| t.status == ThreadStatus::Open))
            .collect())
    }

    pub fn add_comment(
        &self,
        change_id: &str,
        file: &str,
        line_start: usize,
        line_end: usize,
        author: Author,
        text: &str,
        commit_id: &str,
    ) -> Result<(Review, String)> {
        let mut review = self
            .get(change_id)?
            .ok_or_else(|| anyhow::anyhow!("Review not found for change: {}", change_id))?;

        // Auto-create revision if commit differs from last revision (or no revisions yet)
        let last_revision_commit = review.revisions.last().map(|r| r.commit_id.as_str());
        if last_revision_commit != Some(commit_id) {
            let number = review.revisions.len() as u32 + 1;
            review.revisions.push(Revision {
                number,
                commit_id: commit_id.to_string(),
                created_at: Utc::now(),
                description: None,
                is_pending: false,
            });
        }
        review.working_commit_id = Some(commit_id.to_string());

        // Get the current revision number for tagging the thread
        let current_revision = review.revisions.last().map(|r| r.number);

        // Find existing thread or create new one
        let thread_id = review
            .threads
            .iter()
            .find(|t| t.file == file && t.line_start == line_start && t.line_end == line_end)
            .map(|t| t.id.clone());

        let thread_id = match thread_id {
            Some(id) => {
                let thread = review.threads.iter_mut().find(|t| t.id == id).unwrap();
                thread.comments.push(Comment {
                    author,
                    text: text.to_string(),
                    timestamp: Utc::now(),
                });
                id
            }
            None => {
                let id = Uuid::new_v4().to_string()[..8].to_string();
                review.threads.push(Thread {
                    id: id.clone(),
                    file: file.to_string(),
                    line_start,
                    line_end,
                    status: ThreadStatus::Open,
                    comments: vec![Comment {
                        author,
                        text: text.to_string(),
                        timestamp: Utc::now(),
                    }],
                    created_at_commit: Some(commit_id.to_string()),
                    created_at_revision: current_revision,
                    display_line_start: None,
                    display_line_end: None,
                    is_displaced: false,
                    is_deleted: false,
                });
                id
            }
        };

        self.save(&review)?;
        Ok((review, thread_id))
    }

    /// Find a thread by ID or prefix in a review
    fn find_thread_mut<'a>(threads: &'a mut [Thread], thread_id_prefix: &str) -> Result<&'a mut Thread> {
        // Try exact match first
        if let Some(idx) = threads.iter().position(|t| t.id == thread_id_prefix) {
            return Ok(&mut threads[idx]);
        }

        // Try prefix match
        let matches: Vec<_> = threads.iter().enumerate()
            .filter(|(_, t)| t.id.starts_with(thread_id_prefix))
            .map(|(i, _)| i)
            .collect();

        match matches.len() {
            0 => anyhow::bail!("Thread not found: {}", thread_id_prefix),
            1 => Ok(&mut threads[matches[0]]),
            _ => anyhow::bail!("Ambiguous thread_id prefix '{}': matches {} threads", thread_id_prefix, matches.len()),
        }
    }

    pub fn reply_to_thread(
        &self,
        change_id: &str,
        thread_id: &str,
        author: Author,
        text: &str,
    ) -> Result<Review> {
        let mut review = self
            .get_by_prefix(change_id)?
            .ok_or_else(|| anyhow::anyhow!("Review not found for change: {}", change_id))?;

        let thread = Self::find_thread_mut(&mut review.threads, thread_id)?;

        thread.comments.push(Comment {
            author,
            text: text.to_string(),
            timestamp: Utc::now(),
        });

        self.save(&review)?;
        Ok(review)
    }

    pub fn resolve_thread(&self, change_id: &str, thread_id: &str) -> Result<Review> {
        let mut review = self
            .get_by_prefix(change_id)?
            .ok_or_else(|| anyhow::anyhow!("Review not found for change: {}", change_id))?;

        let thread = Self::find_thread_mut(&mut review.threads, thread_id)?;

        thread.status = ThreadStatus::Resolved;
        self.save(&review)?;
        Ok(review)
    }

    pub fn reopen_thread(&self, change_id: &str, thread_id: &str) -> Result<Review> {
        let mut review = self
            .get_by_prefix(change_id)?
            .ok_or_else(|| anyhow::anyhow!("Review not found for change: {}", change_id))?;

        let thread = Self::find_thread_mut(&mut review.threads, thread_id)?;

        thread.status = ThreadStatus::Open;
        self.save(&review)?;
        Ok(review)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, ReviewStore) {
        let dir = TempDir::new().unwrap();
        let store = ReviewStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn test_create_and_get_review() {
        let (_dir, store) = setup();

        let review = store.get_or_create("abc123", "@-", "commit1").unwrap();
        assert_eq!(review.change_id, "abc123");
        assert_eq!(review.base, "@-");
        assert!(review.threads.is_empty());
        assert_eq!(review.working_commit_id, Some("commit1".to_string()));

        let fetched = store.get("abc123").unwrap().unwrap();
        assert_eq!(fetched.change_id, "abc123");
    }

    #[test]
    fn test_add_comment_creates_thread_and_revision() {
        let (_dir, store) = setup();

        store.get_or_create("abc123", "@-", "commit1").unwrap();
        let (review, thread_id) = store
            .add_comment(
                "abc123",
                "src/main.rs",
                10,
                15,
                Author::User,
                "This looks wrong",
                "commit1",
            )
            .unwrap();

        assert_eq!(review.threads.len(), 1);
        assert_eq!(review.threads[0].id, thread_id);
        assert_eq!(review.threads[0].comments.len(), 1);
        assert_eq!(review.threads[0].comments[0].text, "This looks wrong");
        assert_eq!(review.threads[0].created_at_commit, Some("commit1".to_string()));
        assert_eq!(review.threads[0].created_at_revision, Some(1));
        // Auto-creates revision since none existed
        assert_eq!(review.revisions.len(), 1);
        assert_eq!(review.revisions[0].number, 1);
        assert_eq!(review.revisions[0].commit_id, "commit1");
    }

    #[test]
    fn test_add_comment_new_commit_creates_revision() {
        let (_dir, store) = setup();

        store.get_or_create("abc123", "@-", "commit1").unwrap();
        store
            .add_comment("abc123", "src/main.rs", 10, 15, Author::User, "First comment", "commit1")
            .unwrap();
        let (review, _) = store
            .add_comment("abc123", "src/other.rs", 5, 5, Author::User, "Second comment", "commit2")
            .unwrap();

        // Should have two revisions now
        assert_eq!(review.revisions.len(), 2);
        assert_eq!(review.revisions[1].commit_id, "commit2");
    }

    #[test]
    fn test_reply_to_thread() {
        let (_dir, store) = setup();

        store.get_or_create("abc123", "@-", "commit1").unwrap();
        let (_, thread_id) = store
            .add_comment(
                "abc123",
                "src/main.rs",
                10,
                15,
                Author::User,
                "This looks wrong",
                "commit1",
            )
            .unwrap();

        let review = store
            .reply_to_thread("abc123", &thread_id, Author::Claude, "Fixed it!")
            .unwrap();

        assert_eq!(review.threads[0].comments.len(), 2);
        assert_eq!(review.threads[0].comments[1].author, Author::Claude);
    }

    #[test]
    fn test_resolve_thread() {
        let (_dir, store) = setup();

        store.get_or_create("abc123", "@-", "commit1").unwrap();
        let (_, thread_id) = store
            .add_comment(
                "abc123",
                "src/main.rs",
                10,
                15,
                Author::User,
                "This looks wrong",
                "commit1",
            )
            .unwrap();

        let review = store.resolve_thread("abc123", &thread_id).unwrap();
        assert_eq!(review.threads[0].status, ThreadStatus::Resolved);
    }

    #[test]
    fn test_record_revision() {
        let (_dir, store) = setup();

        store.get_or_create("abc123", "@-", "commit1").unwrap();
        let (review, number) = store
            .record_revision("abc123", "commit2", Some("Addressed feedback".to_string()))
            .unwrap();

        assert_eq!(number, 1);
        assert_eq!(review.revisions.len(), 1);
        assert_eq!(review.revisions[0].commit_id, "commit2");
        assert_eq!(review.revisions[0].description, Some("Addressed feedback".to_string()));
        assert_eq!(review.working_commit_id, Some("commit2".to_string()));
    }
}
