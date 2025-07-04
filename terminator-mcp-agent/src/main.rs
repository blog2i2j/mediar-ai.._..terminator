use crate::utils::init_logging;
use anyhow::Result;
use clap::{Parser, ValueEnum};
use rmcp::{
    transport::sse_server::SseServer,
    transport::stdio,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpService,
    },
    ServiceExt,
};
use std::net::SocketAddr;

pub mod server;
pub mod utils;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Terminator MCP Server - Desktop automation via Model Context Protocol"
)]
struct Args {
    /// Transport mode to use
    #[arg(short, long, value_enum, default_value = "stdio")]
    transport: TransportMode,

    /// Port to listen on (only used for SSE and HTTP transports)
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Host to bind to (only used for SSE and HTTP transports)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum TransportMode {
    /// Standard I/O transport (default)
    Stdio,
    /// Server-Sent Events transport for web integrations
    Sse,
    /// Streamable HTTP transport for HTTP-based clients
    Http,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    init_logging()?;

    tracing::info!("Initializing Terminator MCP server...");
    tracing::info!("Transport mode: {:?}", args.transport);

    match args.transport {
        TransportMode::Stdio => {
            tracing::info!("Starting stdio transport...");
            let desktop = server::DesktopWrapper::new().await?;
            let service = desktop.serve(stdio()).await.inspect_err(|e| {
                tracing::error!("Serving error: {:?}", e);
            })?;

            service.waiting().await?;
        }
        TransportMode::Sse => {
            let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
            tracing::info!("Starting SSE server on http://{}", addr);

            let desktop = server::DesktopWrapper::new().await?;
            let ct = SseServer::serve(addr)
                .await?
                .with_service(move || desktop.clone());

            println!("SSE server running on http://{}", addr);
            println!("Connect your MCP client to:");
            println!("  SSE endpoint: http://{}/sse", addr);
            println!("  Message endpoint: http://{}/message", addr);
            println!("Press Ctrl+C to stop");

            tokio::signal::ctrl_c().await?;
            ct.cancel();
            tracing::info!("Shutting down SSE server");
        }
        TransportMode::Http => {
            let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
            tracing::info!("Starting streamable HTTP server on http://{}", addr);

            let desktop = server::DesktopWrapper::new().await?;
            let service = StreamableHttpService::new(
                move || Ok(desktop.clone()),
                LocalSessionManager::default().into(),
                Default::default(),
            );

            let router = axum::Router::new()
                .route("/health", axum::routing::get(health_check))
                .nest_service("/mcp", service);
            let tcp_listener = tokio::net::TcpListener::bind(addr).await?;

            println!("Streamable HTTP server running on http://{}", addr);
            println!("Connect your MCP client to: http://{}/mcp", addr);
            println!("Health check available at: http://{}/health", addr);
            println!("Press Ctrl+C to stop");

            axum::serve(tcp_listener, router)
                .with_graceful_shutdown(async {
                    tokio::signal::ctrl_c().await.ok();
                })
                .await?;

            tracing::info!("Shutting down HTTP server");
        }
    }

    Ok(())
}

async fn health_check() -> impl axum::response::IntoResponse {
    (
        axum::http::StatusCode::OK,
        axum::Json(serde_json::json!({"status": "ok"})),
    )
}
