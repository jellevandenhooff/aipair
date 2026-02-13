use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const TIMELINE_FILE: &str = ".aipair/timeline.jsonl";
const IMPORT_STATE_FILE: &str = ".aipair/timeline-import-state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub data: TimelineEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TimelineEventData {
    ReviewComment {
        change_id: String,
        thread_id: String,
        file: String,
        line_start: usize,
        line_end: usize,
        text: String,
    },
    ReviewReply {
        change_id: String,
        thread_id: String,
        author: String,
        text: String,
    },
    ChatMessage {
        session_id: String,
        author: ChatAuthor,
        text: String,
    },
    CodeSnapshot {
        change_id: String,
        commit_id: String,
        description: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChatAuthor {
    User,
    Claude,
}

pub struct TimelineStore {
    base_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelineFilter {
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub change_id: Option<String>,
    pub event_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImportStats {
    pub sessions_scanned: usize,
    pub messages_imported: usize,
}

/// Tracks per-session import progress
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ImportState {
    /// Maps session file name → last byte offset read
    sessions: HashMap<String, u64>,
}

impl TimelineStore {
    pub fn new(repo_path: impl AsRef<Path>) -> Self {
        Self {
            base_path: repo_path.as_ref().to_path_buf(),
        }
    }

    fn timeline_path(&self) -> PathBuf {
        self.base_path.join(TIMELINE_FILE)
    }

    fn import_state_path(&self) -> PathBuf {
        self.base_path.join(IMPORT_STATE_FILE)
    }

    pub fn append(&self, entry: &TimelineEntry) -> Result<()> {
        let path = self.timeline_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open timeline: {}", path.display()))?;

        let line = serde_json::to_string(entry)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    pub fn read(&self, filter: Option<&TimelineFilter>) -> Result<Vec<TimelineEntry>> {
        let path = self.timeline_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&path)
            .with_context(|| format!("Failed to open timeline: {}", path.display()))?;
        let reader = BufReader::new(file);

        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<TimelineEntry>(&line) {
                Ok(entry) => {
                    if let Some(f) = filter {
                        if !matches_filter(&entry, f) {
                            continue;
                        }
                    }
                    entries.push(entry);
                }
                Err(e) => {
                    tracing::warn!("Skipping malformed timeline entry: {}", e);
                }
            }
        }

        entries.sort_by_key(|e| e.timestamp);
        Ok(entries)
    }

    pub fn import_claude_sessions(&self, project_path: &Path) -> Result<ImportStats> {
        let claude_dir = claude_project_dir(project_path)?;
        if !claude_dir.exists() {
            return Ok(ImportStats::default());
        }

        let mut state = self.load_import_state()?;
        let mut stats = ImportStats::default();

        let entries = std::fs::read_dir(&claude_dir)
            .with_context(|| format!("Failed to read Claude dir: {}", claude_dir.display()))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                let file_name = entry.file_name().to_string_lossy().to_string();
                let session_id = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                stats.sessions_scanned += 1;
                let last_offset = state.sessions.get(&file_name).copied().unwrap_or(0);

                match self.import_session_file(&path, &session_id, last_offset) {
                    Ok((count, new_offset)) => {
                        stats.messages_imported += count;
                        state.sessions.insert(file_name, new_offset);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to import session {}: {}", session_id, e);
                    }
                }
            }
        }

        self.save_import_state(&state)?;
        Ok(stats)
    }

    fn import_session_file(
        &self,
        path: &Path,
        session_id: &str,
        last_offset: u64,
    ) -> Result<(usize, u64)> {
        let mut file = std::fs::File::open(path)?;
        let file_len = file.metadata()?.len();

        if last_offset >= file_len {
            return Ok((0, last_offset));
        }

        file.seek(SeekFrom::Start(last_offset))?;
        let reader = BufReader::new(file);
        let mut count = 0;
        let mut current_offset = last_offset;

        for line in reader.lines() {
            let line = line?;
            current_offset += line.len() as u64 + 1; // +1 for newline

            if line.trim().is_empty() {
                continue;
            }

            if let Some(entry) = parse_claude_session_line(&line, session_id) {
                self.append(&entry)?;
                count += 1;
            }
        }

        Ok((count, current_offset))
    }

    fn load_import_state(&self) -> Result<ImportState> {
        let path = self.import_state_path();
        if !path.exists() {
            return Ok(ImportState::default());
        }
        let content = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content)?)
    }

    fn save_import_state(&self, state: &ImportState) -> Result<()> {
        let path = self.import_state_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(state)?;
        std::fs::write(&path, content)?;
        Ok(())
    }
}

/// Derive the Claude Code project directory from a repo path.
/// Claude Code uses `~/.claude/projects/{mangled-path}/` where the mangling
/// replaces `/` with `-` and strips the leading `/`.
fn claude_project_dir(project_path: &Path) -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let canonical = project_path
        .canonicalize()
        .unwrap_or_else(|_| project_path.to_path_buf());
    let path_str = canonical.to_string_lossy();
    // Strip leading / and replace remaining / with -
    let mangled = path_str
        .strip_prefix('/')
        .unwrap_or(&path_str)
        .replace('/', "-");
    Ok(PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(format!("-{}", mangled)))
}

/// Parse a single line from a Claude Code session JSONL file.
/// Returns None for lines we skip (tool_use, tool_result, system, thinking, sidechain, etc.)
fn parse_claude_session_line(line: &str, session_id: &str) -> Option<TimelineEntry> {
    let val: serde_json::Value = serde_json::from_str(line).ok()?;

    // Skip sidechain messages
    if val.get("isSidechain").and_then(|v| v.as_bool()).unwrap_or(false) {
        return None;
    }

    let msg_type = val.get("type")?.as_str()?;
    let timestamp = val
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(|t| t.parse::<DateTime<Utc>>().ok())
        .unwrap_or_else(Utc::now);

    match msg_type {
        "user" => {
            let message = val.get("message")?;
            let content = message.get("content")?;

            // Content can be a string or array of content blocks
            let text = if let Some(s) = content.as_str() {
                s.to_string()
            } else if let Some(arr) = content.as_array() {
                // Extract text from content blocks
                arr.iter()
                    .filter_map(|block| {
                        if block.get("type")?.as_str()? == "text" {
                            block.get("text")?.as_str().map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                return None;
            };

            if text.is_empty() {
                return None;
            }

            Some(TimelineEntry {
                timestamp,
                data: TimelineEventData::ChatMessage {
                    session_id: session_id.to_string(),
                    author: ChatAuthor::User,
                    text,
                },
            })
        }
        "assistant" => {
            let message = val.get("message")?;
            let content = message.get("content")?.as_array()?;

            // Extract only text blocks, skip thinking/tool_use
            let text_parts: Vec<String> = content
                .iter()
                .filter_map(|block| {
                    let block_type = block.get("type")?.as_str()?;
                    if block_type == "text" {
                        block.get("text")?.as_str().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect();

            if text_parts.is_empty() {
                return None;
            }

            Some(TimelineEntry {
                timestamp,
                data: TimelineEventData::ChatMessage {
                    session_id: session_id.to_string(),
                    author: ChatAuthor::Claude,
                    text: text_parts.join("\n"),
                },
            })
        }
        // Skip: system, progress, file-history-snapshot, tool_result, etc.
        _ => None,
    }
}

fn matches_filter(entry: &TimelineEntry, filter: &TimelineFilter) -> bool {
    if let Some(since) = filter.since {
        if entry.timestamp < since {
            return false;
        }
    }
    if let Some(until) = filter.until {
        if entry.timestamp > until {
            return false;
        }
    }
    if let Some(ref change_id) = filter.change_id {
        let entry_change_id = match &entry.data {
            TimelineEventData::ReviewComment { change_id, .. } => Some(change_id.as_str()),
            TimelineEventData::ReviewReply { change_id, .. } => Some(change_id.as_str()),
            TimelineEventData::CodeSnapshot { change_id, .. } => Some(change_id.as_str()),
            TimelineEventData::ChatMessage { .. } => None,
        };
        if entry_change_id != Some(change_id.as_str()) {
            return false;
        }
    }
    if let Some(ref event_type) = filter.event_type {
        let entry_type = match &entry.data {
            TimelineEventData::ReviewComment { .. } => "ReviewComment",
            TimelineEventData::ReviewReply { .. } => "ReviewReply",
            TimelineEventData::ChatMessage { .. } => "ChatMessage",
            TimelineEventData::CodeSnapshot { .. } => "CodeSnapshot",
        };
        if entry_type != event_type {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, TimelineStore) {
        let dir = TempDir::new().unwrap();
        let store = TimelineStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn test_append_and_read() {
        let (_dir, store) = setup();

        let entry = TimelineEntry {
            timestamp: Utc::now(),
            data: TimelineEventData::ReviewComment {
                change_id: "abc123".to_string(),
                thread_id: "t1".to_string(),
                file: "src/main.rs".to_string(),
                line_start: 10,
                line_end: 15,
                text: "Fix this".to_string(),
            },
        };

        store.append(&entry).unwrap();
        store.append(&entry).unwrap();

        let entries = store.read(None).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_filter_by_type() {
        let (_dir, store) = setup();

        store
            .append(&TimelineEntry {
                timestamp: Utc::now(),
                data: TimelineEventData::ReviewComment {
                    change_id: "abc".to_string(),
                    thread_id: "t1".to_string(),
                    file: "f.rs".to_string(),
                    line_start: 1,
                    line_end: 1,
                    text: "hi".to_string(),
                },
            })
            .unwrap();

        store
            .append(&TimelineEntry {
                timestamp: Utc::now(),
                data: TimelineEventData::ChatMessage {
                    session_id: "s1".to_string(),
                    author: ChatAuthor::User,
                    text: "hello".to_string(),
                },
            })
            .unwrap();

        let filter = TimelineFilter {
            event_type: Some("ChatMessage".to_string()),
            ..Default::default()
        };
        let entries = store.read(Some(&filter)).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_parse_claude_user_message() {
        let line = r#"{"type":"user","timestamp":"2025-01-15T10:00:00Z","message":{"content":"Hello world"}}"#;
        let entry = parse_claude_session_line(line, "test-session").unwrap();
        match entry.data {
            TimelineEventData::ChatMessage { author, text, .. } => {
                assert_eq!(author, ChatAuthor::User);
                assert_eq!(text, "Hello world");
            }
            _ => panic!("Expected ChatMessage"),
        }
    }

    #[test]
    fn test_parse_claude_assistant_message() {
        let line = r#"{"type":"assistant","timestamp":"2025-01-15T10:01:00Z","message":{"content":[{"type":"text","text":"Sure, I can help."},{"type":"tool_use","name":"Read","input":{}}]}}"#;
        let entry = parse_claude_session_line(line, "test-session").unwrap();
        match entry.data {
            TimelineEventData::ChatMessage { author, text, .. } => {
                assert_eq!(author, ChatAuthor::Claude);
                assert_eq!(text, "Sure, I can help.");
            }
            _ => panic!("Expected ChatMessage"),
        }
    }

    #[test]
    fn test_skip_sidechain() {
        let line = r#"{"type":"user","timestamp":"2025-01-15T10:00:00Z","isSidechain":true,"message":{"content":"test"}}"#;
        assert!(parse_claude_session_line(line, "s").is_none());
    }

    #[test]
    fn test_skip_system_type() {
        let line = r#"{"type":"system","timestamp":"2025-01-15T10:00:00Z","message":"init"}"#;
        assert!(parse_claude_session_line(line, "s").is_none());
    }

    #[test]
    fn test_claude_project_dir() {
        // SAFETY: test runs single-threaded; temporarily overriding HOME for test
        unsafe { std::env::set_var("HOME", "/Users/jelle") };
        let dir = claude_project_dir(Path::new("/Users/jelle/hack/aipair")).unwrap();
        assert_eq!(
            dir,
            PathBuf::from("/Users/jelle/.claude/projects/-Users-jelle-hack-aipair")
        );
    }
}
