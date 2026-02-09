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
                        thread.display_line_start = pos.line_start;
                        thread.display_line_end = pos.line_end;
                        thread.is_deleted = pos.is_deleted;
                        thread.is_displaced = pos.line_start != Some(thread.line_start)
                            || pos.line_end != Some(thread.line_end);
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
}

#[tool_handler]
impl ServerHandler for ReviewService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Code review feedback service. Use get_pending_feedback to check for review comments on your changes.".to_string(),
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
