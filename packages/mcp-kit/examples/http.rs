//! Serve an Acme tool over Streamable HTTP on a loopback address.
//!
//! Run with `cargo run --example http --features http-server`, then POST to
//! `http://127.0.0.1:8080/mcp`. Without bearer token protection (see the
//! `authorization` example) the server authenticates no one, so it binds
//! loopback only. It serves until the process ends.
use canonic_mcp_kit::{
    Catalog, Client, Content, ModernOptions, RpcError, ServerInfo, Tool, ToolResult,
    http::{self, HttpOptions, Shutdown},
    stdio::{self, Context, Handler, Outcome, Request},
};
use serde_json::json;
use std::{net::TcpListener, sync::Arc};

struct Acme;

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        // HTTP serves the modern revision only and never initializes.
        Ok(ServerInfo::new("acme", "1"))
    }

    fn call(&self, request: Request, context: &Context) -> Outcome {
        let Request::Tool(call) = request else {
            return Outcome::Done;
        };
        let region = call.arguments["region"].as_str().unwrap_or("default");
        // Reported only when the client asked for progress; it then gets SSE.
        let _ = context.progress(1.0, Some(1.0), Some("Status read"));
        Outcome::Tool(ToolResult::success(vec![Content::text(format!(
            "Acme {region} is healthy."
        ))]))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The region is mirrored into an `Mcp-Param-Region` header, so HTTP
    // intermediaries can route on it without reading the body.
    let catalog = Catalog::new(vec![Tool::new(
        "acme_status",
        "Read the Acme service status for one region.",
        json!({
            "type": "object",
            "properties": {"region": {"type": "string", "x-mcp-header": "Region"}},
            "additionalProperties": false
        }),
    )])?;
    let mut options = stdio::Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    let listener = TcpListener::bind("127.0.0.1:8080")?;
    http::serve(
        Arc::new(Acme),
        Arc::new(catalog),
        listener,
        &HttpOptions::default(),
        &options,
        &Shutdown::new(),
    )?;
    Ok(())
}
