mod dispatch;
mod host;
mod tools;

use crate::host::{HostBridge, HostSelection};
use crate::tools::{ToolDef, ToolSpec, build_tools};
use clap::Parser;
use riffra_control::ErrorCode;
use riffra_control::ProtocolError;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Parser)]
#[command(
    name = "riffra-mcp",
    version,
    about = "Expose a running Riffra Host through the Model Context Protocol",
    long_about = "Expose a running Riffra Host through the Model Context Protocol.\n\n\
Start Riffra Desktop or `riffra --data-root <path> serve` first. The server talks to exactly \
one discovered Host over stdio and writes logs only to stderr."
)]
struct Cli {
    /// Select one Host instance by its id when more than one Riffra Host is running
    #[arg(long, value_name = "INSTANCE_ID")]
    host: Option<String>,
}

struct RiffraMcp {
    bridge: Arc<HostBridge>,
    tools: Vec<ToolSpec>,
}

impl RiffraMcp {
    fn new(bridge: Arc<HostBridge>) -> Result<Self, String> {
        let tools = build_tools()?;
        Ok(Self { bridge, tools })
    }

    fn tool(&self, name: &str) -> Option<&ToolSpec> {
        self.tools.iter().find(|tool| tool.tool.name == name)
    }
}

impl ServerHandler for RiffraMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("riffra-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Controls a running Riffra Host. Use session_read.inspect for lightweight \
                 orientation, musical coordinates for arrangement-absolute notes, and \
                 session_edit.apply for atomic multi-command edits. Start Render or analysis \
                 jobs and follow them with job.wait.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        let tools: Vec<Tool> = self.tools.iter().map(|tool| tool.tool.clone()).collect();
        Ok(ListToolsResult::with_all_items(tools)
            .with_ttl_ms(86_400_000)
            .with_cache_scope(CacheScope::Public))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        let tool = self.tool(&request.name).ok_or_else(|| {
            rmcp::ErrorData::invalid_params(format!("unknown tool: {}", request.name), None)
        })?;
        let arguments = request.arguments.unwrap_or_default();
        let input = Value::Object(arguments.clone());
        if let Some(error) = tool.validator.iter_errors(&input).next() {
            let details = serde_json::json!({
                "path": error.instance_path().to_string(),
                "message": error.to_string(),
            });
            return Ok(dispatch::tool_error(
                ProtocolError::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid arguments for {}: {error}", tool.tool.name),
                )
                .with_details(details),
            )
            .into());
        }
        let tool_definition: &'static ToolDef = tool.definition;
        let bridge = Arc::clone(&self.bridge);
        Ok(
            dispatch::execute(bridge, tool_definition, arguments, context.ct)
                .await
                .into(),
        )
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .init();
    let cli = Cli::parse();
    let selection = match host::connect(cli.host.as_deref()) {
        Ok(selection) => selection,
        Err(error) => {
            eprintln!("riffra-mcp: {error}");
            std::process::exit(1);
        }
    };
    let selected: HostSelection = selection;
    let bridge = Arc::new(HostBridge::new(selected));
    tracing::info!(
        instance_id = bridge.instance_id(),
        "connected to Riffra Host"
    );
    let server = match RiffraMcp::new(Arc::clone(&bridge)) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("riffra-mcp: {error}");
            std::process::exit(1);
        }
    };
    let service = match server.serve(rmcp::transport::stdio()).await {
        Ok(service) => service,
        Err(error) => {
            eprintln!("riffra-mcp: MCP session could not start: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = service.waiting().await {
        eprintln!("riffra-mcp: MCP session failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_control_surface_is_fixed_and_complete() {
        // Act
        let tools = build_tools().unwrap();

        // Assert
        assert_eq!(tools.len(), 48);
        assert!(
            tools
                .iter()
                .all(|tool| tool.definition.name == tool.tool.name)
        );
    }

    #[test]
    fn every_tool_has_one_definition() {
        // Arrange
        let names = tools::TOOLS
            .iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();

        // Act
        let unique = names
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();

        // Assert
        assert_eq!(unique.len(), names.len());
    }
}
