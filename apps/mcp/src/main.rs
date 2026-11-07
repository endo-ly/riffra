mod host;

use crate::host::HostBridge;
use clap::Parser;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Implementation, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt};
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
}

impl ServerHandler for RiffraMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("riffra-mcp", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult::default())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        Err(rmcp::ErrorData::invalid_params(
            format!("unknown tool: {}", request.name),
            None,
        ))
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
    let bridge = Arc::new(HostBridge::new(selection));
    tracing::info!(
        instance_id = bridge.instance_id(),
        "connected to Riffra Host"
    );
    let service = match (RiffraMcp { bridge })
        .serve(rmcp::transport::stdio())
        .await
    {
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
