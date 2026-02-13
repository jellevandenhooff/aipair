use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    #[serde(default)]
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
    #[serde(default)]
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
    #[serde(default)]
    pub created_at_commit: Option<String>,
    #[serde(default)]
    pub created_at_revision: Option<u32>,
    /// Display position after mapping through diffs (not persisted)
    #[serde(default)]
    pub display_line_start: Option<usize>,
    #[serde(default)]
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

    /// List reviews that have open threads.
    /// If `change_ids` is Some, only include reviews for those changes.
    pub fn list_with_open_threads(&self, change_ids: Option<&HashSet<String>>) -> Result<Vec<Review>> {
        let reviews = self.list()?;
        Ok(reviews
            .into_iter()
            .filter(|r| {
                if let Some(ids) = change_ids {
                    if !ids.contains(&r.change_id) {
                        return false;
                    }
                }
                r.threads.iter().any(|t| t.status == ThreadStatus::Open)
            })
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

/// Format pending feedback for a list of reviews with open threads.
/// This is the core logic used by the `feedback` CLI command.
pub(crate) fn format_pending_feedback(jj: &crate::jj::Jj, reviews: Vec<Review>) -> String {
    let mut output = String::new();

    for mut review in reviews {
        let open_threads: Vec<_> = review
            .threads
            .iter()
            .filter(|t| t.status == ThreadStatus::Open)
            .collect();

        if open_threads.is_empty() {
            continue;
        }

        // Map thread positions to current commit
        let target_commit = jj.get_change(&review.change_id).ok().map(|c| c.commit_id);

        if let Some(ref target) = target_commit {
            let mapped = crate::line_mapper::map_all_threads(jj, &review.threads, target);
            for thread in &mut review.threads {
                if let Some(pos) = mapped.get(&thread.id) {
                    thread.display_line_start = Some(pos.line_start);
                    thread.display_line_end = Some(pos.line_end);
                    thread.is_deleted = pos.is_deleted;
                    thread.is_displaced = pos.line_start != thread.line_start
                        || pos.line_end != thread.line_end;
                }
            }
        }

        // Re-filter after mutation
        let open_threads: Vec<_> = review
            .threads
            .iter()
            .filter(|t| t.status == ThreadStatus::Open)
            .collect();

        let current_commit = target_commit.as_deref().unwrap_or("unknown");
        output.push_str(&format!(
            "## Change: {}\n\n",
            &review.change_id[..8.min(review.change_id.len())]
        ));

        // Cache file diffs (change vs parent) to avoid redundant jj calls
        let mut file_diffs: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let base_rev = format!("{}-", review.change_id);

        for thread in open_threads {
            // Use mapped positions for display
            let display_start = thread.display_line_start.unwrap_or(thread.line_start);
            let display_end = thread.display_line_end.unwrap_or(thread.line_end);

            // Thread header
            output.push_str(&format!(
                "### Thread {} — {}\n",
                &thread.id[..8.min(thread.id.len())],
                thread.file,
            ));

            // Original position (where the comment was created)
            let orig_commit = thread.created_at_commit.as_deref().unwrap_or("unknown");
            output.push_str(&format!(
                "**Originally:** lines {}-{} in {}\n",
                thread.line_start,
                thread.line_end,
                &orig_commit[..12.min(orig_commit.len())],
            ));

            // Current position (where the comment maps to now)
            if thread.is_deleted {
                output.push_str(&format!(
                    "**Now:** lines deleted (nearest: {}-{} in {})\n\n",
                    display_start,
                    display_end,
                    &current_commit[..12.min(current_commit.len())],
                ));
            } else {
                output.push_str(&format!(
                    "**Now:** lines {}-{} in {}\n\n",
                    display_start,
                    display_end,
                    &current_commit[..12.min(current_commit.len())],
                ));
            }

            // Show a brief diff around the comment position
            if !thread.is_deleted {
                let diff_text = file_diffs
                    .entry(thread.file.clone())
                    .or_insert_with(|| {
                        jj.diff_raw_between_ctx(&base_rev, &review.change_id, &thread.file, Some(10))
                            .unwrap_or_default()
                    });

                let nearby = extract_nearby_hunks(diff_text, display_start, display_end, 5);
                if !nearby.is_empty() {
                    output.push_str("```diff\n");
                    output.push_str(&nearby);
                    output.push_str("```\n\n");
                } else {
                    // Thread on unchanged code — show raw content as fallback
                    if let Ok(file_content) = jj.show_file(&review.change_id, &thread.file) {
                        let lines: Vec<&str> = file_content.lines().collect();
                        let start = display_start.saturating_sub(3).max(1);
                        let end = (display_end + 3).min(lines.len());

                        output.push_str("```\n");
                        for (i, line) in lines.iter().enumerate() {
                            let line_num = i + 1;
                            if line_num >= start && line_num <= end {
                                let marker = if line_num >= display_start
                                    && line_num <= display_end
                                {
                                    ">"
                                } else {
                                    " "
                                };
                                output.push_str(&format!(
                                    "{} {:4} | {}\n",
                                    marker, line_num, line
                                ));
                            }
                        }
                        output.push_str("```\n\n");
                    }
                }
            }

            // Show comments
            output.push_str("**Comments:**\n");
            for comment in &thread.comments {
                let author = match comment.author {
                    Author::User => "User",
                    Author::Claude => "Claude",
                };
                output.push_str(&format!("- **{}**: {}\n", author, comment.text));
            }
            output.push_str("\n");
        }
    }

    output
}

/// Extract diff lines near a line range in the new file.
/// Tracks new-file line numbers through the diff and only emits lines
/// whose new-file position falls within [line_start - padding, line_end + padding].
fn extract_nearby_hunks(diff_text: &str, line_start: usize, line_end: usize, padding: usize) -> String {
    let target_start = line_start.saturating_sub(padding);
    let target_end = line_end + padding;

    // First pass: collect all diff lines with their new-file line numbers
    struct DiffLine {
        text: String,
        new_line: Option<usize>, // None for deleted lines
    }

    let mut diff_lines: Vec<DiffLine> = Vec::new();
    let mut new_pos: usize = 0;

    for line in diff_text.lines() {
        // Skip file header lines
        if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
        {
            continue;
        }

        if line.starts_with("@@") {
            if let Some((start, _)) = parse_new_file_range(line) {
                new_pos = start;
            }
            diff_lines.push(DiffLine {
                text: line.to_string(),
                new_line: Some(new_pos),
            });
        } else if line.starts_with('-') {
            // Deleted lines don't have a new-file position
            diff_lines.push(DiffLine {
                text: line.to_string(),
                new_line: None,
            });
        } else {
            // Context lines and added lines occupy a new-file line
            diff_lines.push(DiffLine {
                text: line.to_string(),
                new_line: Some(new_pos),
            });
            new_pos += 1;
        }
    }

    // Second pass: emit lines in the target window.
    // Include deleted lines if they're adjacent to included context/add lines.
    let mut result = String::new();
    let mut last_was_included = false;

    for dl in &diff_lines {
        let include = match dl.new_line {
            Some(n) => n >= target_start && n <= target_end,
            // Include deleted lines that are adjacent to included lines
            None => last_was_included,
        };

        // Always include @@ headers for included regions
        if dl.text.starts_with("@@") {
            // Check if any line in this hunk falls in our window
            if let Some((start, count)) = parse_new_file_range(&dl.text) {
                let hunk_end = start + count;
                if start <= target_end && hunk_end >= target_start {
                    result.push_str(&dl.text);
                    result.push('\n');
                }
            }
            last_was_included = false;
            continue;
        }

        if include {
            result.push_str(&dl.text);
            result.push('\n');
            last_was_included = true;
        } else {
            last_was_included = false;
        }
    }

    result
}

/// Parse the new-file range from a @@ hunk header: @@ -old,count +new,count @@
fn parse_new_file_range(header: &str) -> Option<(usize, usize)> {
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    let new_part = parts[2].trim_start_matches('+');
    if let Some((start, count)) = new_part.split_once(',') {
        Some((start.parse().ok()?, count.parse().ok()?))
    } else {
        Some((new_part.parse().ok()?, 1))
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

    #[test]
    fn test_extract_nearby_hunks_filters_to_relevant_hunk() {
        let diff = "\
diff --git a/src/foo.rs b/src/foo.rs
--- a/src/foo.rs
+++ b/src/foo.rs
@@ -5,3 +5,5 @@ fn early() {
 line 5
+new a
+new b
 line 6
 line 7
@@ -50,3 +52,4 @@ fn late() {
 line 50
+inserted
 line 51
 line 52
";
        // Thread at line 53 (near second hunk) — should only get second hunk
        let result = super::extract_nearby_hunks(diff, 53, 53, 5);
        assert!(result.contains("fn late()"), "should contain second hunk header");
        assert!(result.contains("+inserted"), "should contain second hunk content");
        assert!(!result.contains("fn early()"), "should NOT contain first hunk");
        assert!(!result.contains("+new a"), "should NOT contain first hunk content");
    }

    #[test]
    fn test_extract_nearby_hunks_returns_empty_for_distant_lines() {
        let diff = "\
diff --git a/src/foo.rs b/src/foo.rs
--- a/src/foo.rs
+++ b/src/foo.rs
@@ -5,3 +5,5 @@
 line 5
+new a
+new b
 line 6
 line 7
";
        // Thread at line 100 — far from hunk at 5-10
        let result = super::extract_nearby_hunks(diff, 100, 100, 5);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_nearby_hunks_includes_overlapping_hunk() {
        let diff = "\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -10,3 +10,5 @@
 context
+added line
+another
 context end
 more
";
        // Thread right at line 11 (inside the hunk 10-15)
        let result = super::extract_nearby_hunks(diff, 11, 11, 2);
        assert!(result.contains("+added line"));
    }

    #[test]
    fn test_extract_nearby_hunks_trims_large_hunks() {
        // Simulate a large hunk (new file with 50 lines)
        let mut diff = String::from(
            "diff --git a/big.rs b/big.rs\n--- /dev/null\n+++ b/big.rs\n@@ -0,0 +1,50 @@\n",
        );
        for i in 1..=50 {
            diff.push_str(&format!("+line {}\n", i));
        }

        // Thread at lines 25-26 with padding 3 → should show lines 22-29
        let result = super::extract_nearby_hunks(&diff, 25, 26, 3);
        assert!(
            result.contains("+line 25"),
            "should contain the commented line. Result:\n{result}"
        );
        assert!(
            result.contains("+line 22"),
            "should contain padding before. Result:\n{result}"
        );
        assert!(
            result.contains("+line 29"),
            "should contain padding after. Result:\n{result}"
        );
        assert!(
            !result.contains("+line 1\n"),
            "should NOT contain distant start. Result:\n{result}"
        );
        assert!(
            !result.contains("+line 50"),
            "should NOT contain distant end. Result:\n{result}"
        );

        // Count the content lines (excluding the @@ header)
        let content_lines: Vec<&str> = result.lines().filter(|l| !l.starts_with("@@")).collect();
        assert!(
            content_lines.len() <= 10,
            "should show at most ~10 lines of context, got {}. Result:\n{result}",
            content_lines.len()
        );
    }

    // --- format_pending_feedback tests (require jj repo) ---

    fn make_jj_repo() -> (TempDir, crate::jj::Jj) {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        std::process::Command::new("jj")
            .args(["git", "init"])
            .current_dir(path)
            .output()
            .expect("jj git init failed");
        let jj = crate::jj::Jj::new(path);
        (dir, jj)
    }

    fn jj_cmd(dir: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("jj")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        if !output.status.success() {
            panic!(
                "jj {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        String::from_utf8(output.stdout).unwrap()
    }

    fn get_commit_id(dir: &std::path::Path) -> String {
        jj_cmd(dir, &["log", "--no-graph", "-r", "@", "-T", "commit_id"])
            .trim()
            .to_string()
    }

    fn get_change_id(dir: &std::path::Path) -> String {
        jj_cmd(dir, &["log", "--no-graph", "-r", "@", "-T", "change_id"])
            .trim()
            .to_string()
    }

    #[test]
    fn test_format_feedback_shows_positions_and_diff() {
        let (dir, jj) = make_jj_repo();
        let path = dir.path();

        // Create initial file with 20 lines
        let content: String = (1..=20).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(path.join("test.rs"), &content).unwrap();
        jj_cmd(path, &["describe", "-m", "base"]);

        // Create a new change that modifies lines 10-11
        jj_cmd(path, &["new", "-m", "modify middle"]);
        let new_content: String = (1..=20)
            .map(|i| {
                if i == 10 {
                    "modified 10\n".to_string()
                } else if i == 11 {
                    "modified 11\n".to_string()
                } else {
                    format!("line {}\n", i)
                }
            })
            .collect();
        std::fs::write(path.join("test.rs"), &new_content).unwrap();

        let change_id = get_change_id(path);
        let commit1 = get_commit_id(path);

        // Create a review and comment on the modified lines
        let store = ReviewStore::new(jj.repo_path());
        store.init().unwrap();
        store
            .get_or_create(&change_id, &format!("{}-", change_id), &commit1)
            .unwrap();
        store
            .add_comment(
                &change_id,
                "test.rs",
                10,
                11,
                Author::User,
                "Fix this logic",
                &commit1,
            )
            .unwrap();

        let reviews = store.list_with_open_threads(None).unwrap();
        let output = super::format_pending_feedback(&jj, reviews);

        // Check structure
        assert!(output.contains("## Change:"), "should have change header");
        assert!(output.contains("test.rs"), "should reference the file");

        // Check positions with commit SHAs
        assert!(
            output.contains("**Originally:** lines 10-11"),
            "should show original lines. Output:\n{output}"
        );
        assert!(
            output.contains(&commit1[..12]),
            "should include original commit SHA. Output:\n{output}"
        );
        assert!(
            output.contains("**Now:** lines 10-11"),
            "should show current lines. Output:\n{output}"
        );

        // Check diff shows the changed lines
        assert!(
            output.contains("+modified 10"),
            "should show added lines in diff. Output:\n{output}"
        );
        assert!(
            output.contains("-line 10"),
            "should show removed lines in diff. Output:\n{output}"
        );

        // Check diff is trimmed — should NOT show distant lines
        assert!(
            !output.contains("line 1\n"),
            "should NOT show line 1 (too far). Output:\n{output}"
        );
        assert!(
            !output.contains("line 20"),
            "should NOT show line 20 (too far). Output:\n{output}"
        );

        // Check comment
        assert!(output.contains("Fix this logic"), "should show comment text");
        assert!(output.contains("**User**"), "should show author");
    }

    #[test]
    fn test_format_feedback_deleted_lines() {
        let (dir, jj) = make_jj_repo();
        let path = dir.path();

        let content: String = (1..=10).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(path.join("test.rs"), &content).unwrap();
        jj_cmd(path, &["describe", "-m", "initial"]);

        let change_id = get_change_id(path);
        let commit1 = get_commit_id(path);

        let store = ReviewStore::new(jj.repo_path());
        store.init().unwrap();
        store.get_or_create(&change_id, &format!("{}-", change_id), &commit1).unwrap();
        store
            .add_comment(&change_id, "test.rs", 5, 5, Author::User, "Remove this", &commit1)
            .unwrap();

        // Delete line 5
        let new_content: String = (1..=10)
            .filter(|i| *i != 5)
            .map(|i| format!("line {}\n", i))
            .collect();
        std::fs::write(path.join("test.rs"), &new_content).unwrap();

        let reviews = store.list_with_open_threads(None).unwrap();
        let output = super::format_pending_feedback(&jj, reviews);

        assert!(
            output.contains("lines deleted"),
            "should indicate lines were deleted. Output:\n{output}"
        );
        assert!(
            output.contains("Remove this"),
            "should still show the comment"
        );
    }

    #[test]
    fn test_format_feedback_unchanged_code_fallback() {
        let (dir, jj) = make_jj_repo();
        let path = dir.path();

        // Create two files
        std::fs::write(path.join("changed.rs"), "line 1\nline 2\n").unwrap();
        std::fs::write(path.join("stable.rs"), "stable 1\nstable 2\nstable 3\n").unwrap();
        jj_cmd(path, &["describe", "-m", "initial"]);

        let change_id = get_change_id(path);
        let commit1 = get_commit_id(path);

        let store = ReviewStore::new(jj.repo_path());
        store.init().unwrap();
        store.get_or_create(&change_id, &format!("{}-", change_id), &commit1).unwrap();

        // Comment on the file that won't change in this commit
        store
            .add_comment(&change_id, "stable.rs", 2, 2, Author::User, "Rename this", &commit1)
            .unwrap();

        // Only modify changed.rs (stable.rs stays the same)
        std::fs::write(path.join("changed.rs"), "line 1\nnew line\nline 2\n").unwrap();

        let reviews = store.list_with_open_threads(None).unwrap();
        let output = super::format_pending_feedback(&jj, reviews);

        // Since stable.rs has no diff, should fall back to showing raw file content
        assert!(
            output.contains("stable 2"),
            "should show raw file content as fallback. Output:\n{output}"
        );
        assert!(
            output.contains("Rename this"),
            "should show comment text"
        );
    }
}
