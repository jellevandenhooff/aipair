use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, ErrorCode, ErrorData as McpError, Implementation, ProtocolVersion,
        ServerCapabilities, ServerInfo,
    },
    tool, tool_handler, tool_router, ServerHandler,
};
use serde::Deserialize;
use std::borrow::Cow;

use crate::jj::Jj;
use crate::review::{Author, ReviewStore, ThreadStatus};
use crate::topic::{slugify, TopicStore};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RespondRequest {
    #[schemars(description = "The change ID containing the thread")]
    pub change_id: String,
    #[schemars(description = "The thread ID to respond to")]
    pub thread_id: String,
    #[schemars(description = "Your response message")]
    pub message: String,
    #[schemars(description = "Whether to resolve the thread after responding")]
    #[serde(default)]
    pub resolve: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecordRevisionRequest {
    #[schemars(description = "The change ID to record a revision for")]
    pub change_id: String,
    #[schemars(description = "Brief summary of what was addressed in this revision")]
    pub description: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetPendingFeedbackRequest {
    #[schemars(description = "Optional topic slug to filter feedback to only changes in that topic")]
    pub topic: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateTopicRequest {
    #[schemars(description = "Short slug for the topic, max ~20 chars (e.g. 'fix-auth', 'dag-viz')")]
    pub name: String,
    #[schemars(description = "Optional freeform markdown plan/notes")]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateTopicRequest {
    #[schemars(description = "The topic ID (slug) to update")]
    pub topic_id: String,
    #[schemars(description = "Change IDs to add to this topic")]
    pub add_changes: Option<Vec<String>>,
    #[schemars(description = "Change IDs to remove from this topic")]
    pub remove_changes: Option<Vec<String>>,
    #[schemars(description = "Overwrite the topic's notes.md with this content")]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FinishTopicRequest {
    #[schemars(description = "The topic ID (slug) to finish")]
    pub topic_id: String,
    #[schemars(description = "Force finish even with open review threads")]
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ReviewService {
    tool_router: ToolRouter<ReviewService>,
}

fn mcp_error(msg: impl Into<Cow<'static, str>>) -> McpError {
    McpError {
        code: ErrorCode::INTERNAL_ERROR,
        message: msg.into(),
        data: None,
    }
}

#[tool_router]
impl ReviewService {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Respond to a review thread and optionally resolve it")]
    async fn respond_to_thread(
        &self,
        params: Parameters<RespondRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = &params.0;
        let jj = Jj::discover().map_err(|e| mcp_error(e.to_string()))?;
        let store = ReviewStore::new(jj.repo_path());

        store
            .reply_to_thread(&req.change_id, &req.thread_id, Author::Claude, &req.message)
            .map_err(|e| mcp_error(e.to_string()))?;

        if req.resolve {
            store
                .resolve_thread(&req.change_id, &req.thread_id)
                .map_err(|e| mcp_error(e.to_string()))?;
        }

        let status = if req.resolve { " and resolved" } else { "" };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Responded to thread {}{}.",
            req.thread_id, status
        ))]))
    }

    #[tool(description = "Record a new revision after addressing feedback. Call this after making changes to create a snapshot that reviewers can compare against.")]
    async fn record_revision(
        &self,
        params: Parameters<RecordRevisionRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = &params.0;
        let jj = Jj::discover().map_err(|e| mcp_error(e.to_string()))?;
        let store = ReviewStore::new(jj.repo_path());

        // Get current commit_id for this change
        let changes = jj.log(100).map_err(|e| mcp_error(e.to_string()))?;
        let change = changes
            .iter()
            .find(|c| c.change_id.starts_with(&req.change_id) || req.change_id.starts_with(&c.change_id))
            .ok_or_else(|| mcp_error(format!("Change not found: {}", req.change_id)))?;

        // Check if there are actual changes since the last revision
        if let Ok(Some(review)) = store.get_by_prefix(&change.change_id) {
            if let Some(last_rev) = review.revisions.last() {
                if last_rev.commit_id == change.commit_id {
                    return Err(mcp_error(format!(
                        "No changes since last revision (v{}). Make changes before recording a new revision.",
                        last_rev.number
                    )));
                }
            }
        }

        let (_, revision_number) = store
            .record_revision(&change.change_id, &change.commit_id, Some(req.description.clone()))
            .map_err(|e| mcp_error(e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Recorded revision {} for change {}. Summary: {}",
            revision_number,
            &change.change_id[..8.min(change.change_id.len())],
            req.description
        ))]))
    }

    #[tool(description = "Get pending review feedback for your changes. Optionally filter by topic.")]
    async fn get_pending_feedback(
        &self,
        params: Parameters<GetPendingFeedbackRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = &params.0;
        let jj = Jj::discover().map_err(|e| mcp_error(e.to_string()))?;
        let store = ReviewStore::new(jj.repo_path());

        // If topic filter specified, get the set of change IDs in that topic
        let topic_changes = if let Some(ref topic_id) = req.topic {
            let topic_store = TopicStore::new(jj.repo_path());
            let topic = topic_store
                .get(topic_id)
                .map_err(|e| mcp_error(e.to_string()))?
                .ok_or_else(|| mcp_error(format!("Topic '{}' not found", topic_id)))?;
            Some(topic.changes)
        } else {
            None
        };

        let reviews = store
            .list_with_open_threads(topic_changes.as_ref())
            .map_err(|e| mcp_error(e.to_string()))?;

        if reviews.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No pending feedback.",
            )]));
        }

        let output = super::mcp::format_pending_feedback(&jj, reviews);
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(description = "Create a new topic (named scope for a stack of changes). Creates the topic and sets it as the active topic. Add changes to it with update_topic.")]
    async fn create_topic(
        &self,
        params: Parameters<CreateTopicRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = &params.0;
        let jj = Jj::discover().map_err(|e| mcp_error(e.to_string()))?;
        let topic_store = TopicStore::new(jj.repo_path());
        topic_store.init().map_err(|e| mcp_error(e.to_string()))?;

        let id = slugify(&req.name);
        if id.is_empty() {
            return Err(mcp_error("Topic name must contain alphanumeric characters"));
        }
        if id != req.name {
            return Err(mcp_error(format!(
                "Topic name must be a valid slug (lowercase, alphanumeric, hyphens). Got '{}', expected '{}'",
                req.name, id
            )));
        }
        if id.len() > 20 {
            return Err(mcp_error(format!(
                "Topic name too long ({} chars, max 20). Try a shorter slug.",
                id.len()
            )));
        }

        // Get current change as the base
        let base_change = jj.get_change("@").map_err(|e| mcp_error(e.to_string()))?;

        // Create the topic — slug is the name
        topic_store
            .create(&id, &req.name, &base_change.change_id)
            .map_err(|e| mcp_error(e.to_string()))?;

        // Set notes if provided
        if let Some(ref notes) = req.notes {
            topic_store
                .set_notes(&id, notes)
                .map_err(|e| mcp_error(e.to_string()))?;
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Created topic '{}' (id: {}). Base: {}. Use update_topic to add changes.",
            req.name,
            id,
            &base_change.change_id[..8.min(base_change.change_id.len())]
        ))]))
    }

    #[tool(description = "Update a topic's change list or notes. Use this to keep the topic in sync after creating/squashing/splitting changes. Change IDs can be short prefixes.")]
    async fn update_topic(
        &self,
        params: Parameters<UpdateTopicRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = &params.0;
        let jj = Jj::discover().map_err(|e| mcp_error(e.to_string()))?;
        let topic_store = TopicStore::new(jj.repo_path());

        // Verify topic exists first
        topic_store
            .get(&req.topic_id)
            .map_err(|e| mcp_error(e.to_string()))?
            .ok_or_else(|| mcp_error(format!("Topic not found: {}", req.topic_id)))?;

        let mut updates = Vec::new();

        if let Some(ref add) = req.add_changes {
            // Resolve short IDs and validate each change exists in jj
            let mut resolved = Vec::new();
            for id in add {
                let change = jj
                    .get_change(id)
                    .map_err(|_| mcp_error(format!("Change not found: {}", id)))?;
                resolved.push(change.change_id);
            }
            topic_store
                .add_changes(&req.topic_id, &resolved)
                .map_err(|e| mcp_error(e.to_string()))?;
            updates.push(format!("added {} change(s)", resolved.len()));
        }

        if let Some(ref remove) = req.remove_changes {
            // remove_changes handles prefix matching against stored IDs
            topic_store
                .remove_changes(&req.topic_id, remove)
                .map_err(|e| mcp_error(e.to_string()))?;
            updates.push(format!("removed {} change(s)", remove.len()));
        }

        if let Some(ref notes) = req.notes {
            topic_store
                .set_notes(&req.topic_id, notes)
                .map_err(|e| mcp_error(e.to_string()))?;
            updates.push("updated notes".to_string());
        }

        let topic = topic_store
            .get(&req.topic_id)
            .map_err(|e| mcp_error(e.to_string()))?
            .ok_or_else(|| mcp_error(format!("Topic not found: {}", req.topic_id)))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updated topic '{}': {}. Total changes: {}.",
            topic.name,
            if updates.is_empty() { "no changes".to_string() } else { updates.join(", ") },
            topic.changes.len()
        ))]))
    }

    #[tool(description = "List all topics with their changes, status, and review info. Shows changes in topological order from the jj DAG.")]
    async fn get_topics(&self) -> Result<CallToolResult, McpError> {
        let jj = Jj::discover().map_err(|e| mcp_error(e.to_string()))?;
        let topic_store = TopicStore::new(jj.repo_path());
        let review_store = ReviewStore::new(jj.repo_path());

        let topics = topic_store.list().map_err(|e| mcp_error(e.to_string()))?;

        if topics.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No topics found.",
            )]));
        }

        // Get all changes for ordering
        let all_changes = jj.log(200).map_err(|e| mcp_error(e.to_string()))?;

        let mut output = String::new();

        for topic in &topics {
            let status_str = match topic.status {
                crate::topic::TopicStatus::Active => "",
                crate::topic::TopicStatus::Finished => " (finished)",
            };

            output.push_str(&format!(
                "## {} (id: {}){} — {} change(s)\n",
                topic.name, topic.id, status_str, topic.changes.len()
            ));

            // Sort topic's changes in topological order (by position in the DAG log)
            let mut ordered_changes: Vec<_> = all_changes
                .iter()
                .filter(|c| topic.changes.contains(&c.change_id))
                .collect();
            // jj log returns newest first; reverse to show base-to-tip
            ordered_changes.reverse();

            for change in &ordered_changes {
                // Get open thread count from review
                let open_threads = review_store
                    .get(&change.change_id)
                    .ok()
                    .flatten()
                    .map(|r| {
                        r.threads
                            .iter()
                            .filter(|t| t.status == ThreadStatus::Open)
                            .count()
                    })
                    .unwrap_or(0);

                let thread_info = if open_threads > 0 {
                    format!("  ({} open)", open_threads)
                } else {
                    String::new()
                };

                let first_line = change.description.lines().next().unwrap_or("");
                let truncated = change.description.contains('\n');
                let desc = if first_line.is_empty() {
                    "(no description)".to_string()
                } else if truncated {
                    format!("{} [...]", first_line)
                } else {
                    first_line.to_string()
                };

                output.push_str(&format!(
                    "  {}  {}{}\n",
                    &change.change_id[..8.min(change.change_id.len())],
                    desc,
                    thread_info
                ));
            }

            // Show notes if present
            {
                let notes = topic_store
                    .get_notes(&topic.id)
                    .map_err(|e| mcp_error(e.to_string()))?;
                if !notes.is_empty() {
                    output.push_str(&format!("\n### Notes\n{}\n", notes));
                }
            }

            output.push('\n');
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(description = "Finish a topic: validate the stack and move main bookmark to the tip. All changes must have descriptions and (unless force=true) no open review threads.")]
    async fn finish_topic(
        &self,
        params: Parameters<FinishTopicRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = &params.0;
        let jj = Jj::discover().map_err(|e| mcp_error(e.to_string()))?;
        let topic_store = TopicStore::new(jj.repo_path());
        let review_store = ReviewStore::new(jj.repo_path());

        let topic = topic_store
            .get(&req.topic_id)
            .map_err(|e| mcp_error(e.to_string()))?
            .ok_or_else(|| mcp_error(format!("Topic not found: {}", req.topic_id)))?;

        if topic.changes.is_empty() {
            return Err(mcp_error("Topic has no changes"));
        }

        // Get all changes for ordering
        let all_changes = jj.log(200).map_err(|e| mcp_error(e.to_string()))?;

        // Find topic's changes in topological order
        let ordered_changes: Vec<_> = all_changes
            .iter()
            .filter(|c| topic.changes.contains(&c.change_id))
            .collect();

        if ordered_changes.len() != topic.changes.len() {
            let found: std::collections::HashSet<_> =
                ordered_changes.iter().map(|c| &c.change_id).collect();
            let missing: Vec<_> = topic
                .changes
                .iter()
                .filter(|id| !found.contains(id))
                .collect();
            return Err(mcp_error(format!(
                "Some changes not found in jj log: {:?}",
                missing
            )));
        }

        // Validate: all changes must have descriptions
        let empty_desc: Vec<_> = ordered_changes
            .iter()
            .filter(|c| c.description.trim().is_empty())
            .map(|c| c.change_id[..8.min(c.change_id.len())].to_string())
            .collect();
        if !empty_desc.is_empty() {
            return Err(mcp_error(format!(
                "Changes with empty descriptions: {}",
                empty_desc.join(", ")
            )));
        }

        // Validate: no open review threads (unless force)
        if !req.force {
            let mut total_open = 0;
            for change in &ordered_changes {
                if let Ok(Some(review)) = review_store.get(&change.change_id) {
                    total_open += review
                        .threads
                        .iter()
                        .filter(|t| t.status == ThreadStatus::Open)
                        .count();
                }
            }
            if total_open > 0 {
                return Err(mcp_error(format!(
                    "Topic has {} open review thread(s). Use force=true to override.",
                    total_open
                )));
            }
        }

        // The tip is the first in ordered_changes (newest first from jj log)
        let tip = &ordered_changes[0];

        // Move main bookmark to tip
        jj.move_bookmark("main", &tip.change_id)
            .map_err(|e| mcp_error(e.to_string()))?;

        // Mark topic as finished
        topic_store
            .finish(&req.topic_id)
            .map_err(|e| mcp_error(e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Finished topic '{}'. main bookmark moved to {}. {} change(s) merged.",
            topic.name,
            &tip.change_id[..8.min(tip.change_id.len())],
            ordered_changes.len()
        ))]))
    }
}

#[tool_handler]
impl ServerHandler for ReviewService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Code review and topic management service.\n\n\
                 ## Topics\n\
                 Use get_topics to see active work. When working on a topic:\n\
                 - After creating a new jj change, call update_topic to add it\n\
                 - After squashing/splitting changes, call update_topic to sync the change list\n\
                 - Keep change descriptions meaningful — update them as content evolves\n\
                 - Default to creating new changes (squashing is easy, splitting is hard)\n\
                 - Stay in the current change for small follow-ups\n\n\
                 ## Reviews\n\
                 Use get_pending_feedback to check for review comments on your changes."
                    .to_string(),
            ),
        }
    }
}

/// Format pending feedback for a list of reviews with open threads.
/// This is the core logic of get_pending_feedback, extracted for testability.
fn format_pending_feedback(jj: &Jj, reviews: Vec<crate::review::Review>) -> String {
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
                    crate::review::Author::User => "User",
                    crate::review::Author::Claude => "Claude",
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

/// Create an MCP service for HTTP transport, returning the router with the service nested
pub fn create_mcp_router() -> axum::Router {
    use rmcp::transport::streamable_http_server::{StreamableHttpService, StreamableHttpServerConfig, session::local::LocalSessionManager};

    let config = StreamableHttpServerConfig {
        stateful_mode: false,  // Disable session requirements
        ..Default::default()
    };

    let service = StreamableHttpService::new(
        || Ok(ReviewService::new()),
        LocalSessionManager::default().into(),
        config,
    );

    axum::Router::new().nest_service("/mcp", service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{Author, ReviewStore};
    use std::process::Command;
    use tempfile::TempDir;

    fn make_jj_repo() -> (TempDir, Jj) {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        Command::new("jj")
            .args(["git", "init"])
            .current_dir(path)
            .output()
            .expect("jj git init failed");
        let jj = Jj::new(path);
        (dir, jj)
    }

    fn jj_cmd(dir: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("jj")
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
        let result = extract_nearby_hunks(diff, 53, 53, 5);
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
        let result = extract_nearby_hunks(diff, 100, 100, 5);
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
        let result = extract_nearby_hunks(diff, 11, 11, 2);
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
        let result = extract_nearby_hunks(&diff, 25, 26, 3);
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
        let output = format_pending_feedback(&jj, reviews);

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
        let output = format_pending_feedback(&jj, reviews);

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
        let output = format_pending_feedback(&jj, reviews);

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
