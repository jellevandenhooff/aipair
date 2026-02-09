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
pub struct CreateTopicRequest {
    #[schemars(description = "Human-readable name for the topic, e.g. 'Fix auth flow'")]
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

    #[tool(description = "Get pending review feedback for your changes")]
    async fn get_pending_feedback(&self) -> Result<CallToolResult, McpError> {
        let jj = Jj::discover().map_err(|e| mcp_error(e.to_string()))?;
        let store = ReviewStore::new(jj.repo_path());

        let reviews = store
            .list_with_open_threads()
            .map_err(|e| mcp_error(e.to_string()))?;

        if reviews.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No pending feedback.",
            )]));
        }

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
            let target_commit = review.working_commit_id.clone()
                .or_else(|| jj.get_change(&review.change_id).ok().map(|c| c.commit_id));

            if let Some(ref target) = target_commit {
                let mapped = crate::line_mapper::map_all_threads(&jj, &review.threads, target);
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

            output.push_str(&format!("## Change: {}\n\n", &review.change_id[..8.min(review.change_id.len())]));

            for thread in open_threads {
                // Use mapped positions for display
                let display_start = thread.display_line_start.unwrap_or(thread.line_start);
                let display_end = thread.display_line_end.unwrap_or(thread.line_end);

                // Show header with original position info if displaced
                if thread.is_displaced || thread.is_deleted {
                    output.push_str(&format!(
                        "### Thread {} - {}:{}-{} (originally :{}−{} in revision {})\n\n",
                        &thread.id[..8.min(thread.id.len())],
                        thread.file,
                        display_start,
                        display_end,
                        thread.line_start,
                        thread.line_end,
                        thread.created_at_revision.map(|n| format!("v{}", n)).unwrap_or_else(|| "?".to_string()),
                    ));
                } else {
                    output.push_str(&format!(
                        "### Thread {} - {}:{}-{}\n\n",
                        &thread.id[..8.min(thread.id.len())], thread.file, display_start, display_end
                    ));
                }

                if thread.is_deleted {
                    output.push_str("**Note:** The commented lines have been deleted.\n\n");
                }

                // Show code context at mapped position
                if !thread.is_deleted {
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
                                output.push_str(&format!("{} {:4} | {}\n", marker, line_num, line));
                            }
                        }
                        output.push_str("```\n\n");
                    }
                }

                // Show original revision info if available
                if let Some(rev_num) = thread.created_at_revision {
                    if let Some(ref commit) = thread.created_at_commit {
                        output.push_str(&format!(
                            "**Original revision:** v{} (commit {})\n",
                            rev_num,
                            &commit[..8.min(commit.len())]
                        ));
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

        // Get current change as the base
        let base_change = jj.get_change("@").map_err(|e| mcp_error(e.to_string()))?;

        // Create the topic
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
                "## {}{} — {} change(s)\n",
                topic.name, status_str, topic.changes.len()
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

                let desc = if change.description.is_empty() {
                    "(no description)"
                } else {
                    &change.description
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
