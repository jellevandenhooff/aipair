use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;
use uuid::Uuid;

const REVIEWS_DIR: &str = ".aipair/reviews";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../web/src/types/")]
pub struct Review {
    pub change_id: String,
    pub base: String,
    pub created_at: DateTime<Utc>,
    pub threads: Vec<Thread>,
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

    pub fn save(&self, review: &Review) -> Result<()> {
        self.init()?;
        let path = self.review_path(&review.change_id);
        let content = serde_json::to_string_pretty(review)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn get_or_create(&self, change_id: &str, base: &str) -> Result<Review> {
        if let Some(review) = self.get(change_id)? {
            return Ok(review);
        }

        let review = Review {
            change_id: change_id.to_string(),
            base: base.to_string(),
            created_at: Utc::now(),
            threads: Vec::new(),
        };

        self.save(&review)?;
        Ok(review)
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
    ) -> Result<(Review, String)> {
        let mut review = self
            .get(change_id)?
            .ok_or_else(|| anyhow::anyhow!("Review not found for change: {}", change_id))?;

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
                });
                id
            }
        };

        self.save(&review)?;
        Ok((review, thread_id))
    }

    pub fn reply_to_thread(
        &self,
        change_id: &str,
        thread_id: &str,
        author: Author,
        text: &str,
    ) -> Result<Review> {
        let mut review = self
            .get(change_id)?
            .ok_or_else(|| anyhow::anyhow!("Review not found for change: {}", change_id))?;

        let thread = review
            .threads
            .iter_mut()
            .find(|t| t.id == thread_id)
            .ok_or_else(|| anyhow::anyhow!("Thread not found: {}", thread_id))?;

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
            .get(change_id)?
            .ok_or_else(|| anyhow::anyhow!("Review not found for change: {}", change_id))?;

        let thread = review
            .threads
            .iter_mut()
            .find(|t| t.id == thread_id)
            .ok_or_else(|| anyhow::anyhow!("Thread not found: {}", thread_id))?;

        thread.status = ThreadStatus::Resolved;
        self.save(&review)?;
        Ok(review)
    }

    pub fn reopen_thread(&self, change_id: &str, thread_id: &str) -> Result<Review> {
        let mut review = self
            .get(change_id)?
            .ok_or_else(|| anyhow::anyhow!("Review not found for change: {}", change_id))?;

        let thread = review
            .threads
            .iter_mut()
            .find(|t| t.id == thread_id)
            .ok_or_else(|| anyhow::anyhow!("Thread not found: {}", thread_id))?;

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

        let review = store.get_or_create("abc123", "@-").unwrap();
        assert_eq!(review.change_id, "abc123");
        assert_eq!(review.base, "@-");
        assert!(review.threads.is_empty());

        let fetched = store.get("abc123").unwrap().unwrap();
        assert_eq!(fetched.change_id, "abc123");
    }

    #[test]
    fn test_add_comment_creates_thread() {
        let (_dir, store) = setup();

        store.get_or_create("abc123", "@-").unwrap();
        let (review, thread_id) = store
            .add_comment(
                "abc123",
                "src/main.rs",
                10,
                15,
                Author::User,
                "This looks wrong",
            )
            .unwrap();

        assert_eq!(review.threads.len(), 1);
        assert_eq!(review.threads[0].id, thread_id);
        assert_eq!(review.threads[0].comments.len(), 1);
        assert_eq!(review.threads[0].comments[0].text, "This looks wrong");
    }

    #[test]
    fn test_reply_to_thread() {
        let (_dir, store) = setup();

        store.get_or_create("abc123", "@-").unwrap();
        let (_, thread_id) = store
            .add_comment(
                "abc123",
                "src/main.rs",
                10,
                15,
                Author::User,
                "This looks wrong",
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

        store.get_or_create("abc123", "@-").unwrap();
        let (_, thread_id) = store
            .add_comment(
                "abc123",
                "src/main.rs",
                10,
                15,
                Author::User,
                "This looks wrong",
            )
            .unwrap();

        let review = store.resolve_thread("abc123", &thread_id).unwrap();
        assert_eq!(review.threads[0].status, ThreadStatus::Resolved);
    }
}
