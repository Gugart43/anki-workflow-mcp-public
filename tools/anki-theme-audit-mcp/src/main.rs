use anyhow::Result;
use rmcp::{ServiceExt, service::ServerInitializeError};

mod handler;
mod state;
mod tools;

use handler::McpHandler;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "version" | "--version" | "-V" => {
                println!("anki-theme-audit-mcp {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "health" => {
                println!(
                    "{{\"status\":\"ok\",\"version\":\"{}\",\"service\":\"anki-theme-audit-mcp\"}}",
                    env!("CARGO_PKG_VERSION")
                );
                return Ok(());
            }
            _ => {}
        }
    }

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let handler = McpHandler::new();
    let service = match handler.serve(rmcp::transport::stdio()).await {
        Ok(service) => service,
        Err(ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
        Err(e) => return Err(anyhow::anyhow!("mcp initialize error: {e}")),
    };

    tokio::select! {
        res = service.waiting() => {
            res.map_err(|e| anyhow::anyhow!("mcp service join error: {e}"))?;
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received ctrl-c signal, shutting down anki-theme-audit-mcp");
        }
    }

    Ok(())
}
