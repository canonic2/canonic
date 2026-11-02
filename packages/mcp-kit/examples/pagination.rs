//! Publish a tool catalog large enough to list across multiple pages.
use canonic_mcp_kit::{
    Catalog, Client, Content, ModernOptions, RpcError, ServerInfo, Tool, ToolResult,
    stdio::{self, Context, Handler, Outcome, Request},
};
use serde_json::json;
use std::{io, sync::Arc};

struct Acme;
impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1"))
    }
    fn call(&self, request: Request, _: &Context) -> Outcome {
        let Request::Tool(call) = request else {
            return Outcome::Done;
        };
        Outcome::Tool(ToolResult::success(vec![Content::text(format!(
            "Status for {}: ready",
            call.name
        ))]))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::new(
        (0..130)
            .map(|i| {
                Tool::new(
                    format!("acme_status_{i}"),
                    format!("Read status for service {i}."),
                    json!({"type":"object","additionalProperties":false}),
                )
            })
            .collect(),
    )?;
    let mut options = stdio::Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    stdio::serve(
        Arc::new(Acme),
        Arc::new(catalog),
        io::BufReader::new(io::stdin()),
        io::stdout(),
        &options,
    )?;
    Ok(())
}
