use rmcp::{
    handler::server::tool::ToolRouter,
    model::{
        CallToolResult, Content, ErrorData as McpError, Implementation, ProtocolVersion,
        ServerCapabilities, ServerInfo,
    },
    tool, tool_handler, tool_router, ServerHandler,
};

#[derive(Debug, Clone)]
pub struct ReviewService {
    tool_router: ToolRouter<ReviewService>,
}

#[tool_router]
impl ReviewService {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Get pending review feedback for your changes")]
    async fn get_pending_feedback(&self) -> Result<CallToolResult, McpError> {
        // For now, return a simple message
        Ok(CallToolResult::success(vec![Content::text(
            "No pending feedback at this time.",
        )]))
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

pub async fn run_mcp_server() -> anyhow::Result<()> {
    use rmcp::{transport::stdio, ServiceExt};

    let service = ReviewService::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
