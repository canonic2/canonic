//! Publish a catalog change to an explicitly subscribed modern host.
use canonic_mcp_kit::{
    Catalog, Change, Client, Content, ModernOptions, RpcError, ServerInfo, SubscriptionOptions,
    Tool, ToolResult,
    stdio::{self, ChangeSender, Context, Handler, Outcome, Request},
};
use serde_json::json;
use std::{
    io,
    sync::{Arc, Mutex},
};

struct Acme(Mutex<Option<ChangeSender>>);

fn catalog(expanded: bool) -> Arc<Catalog> {
    let mut tools = vec![Tool::new(
        "acme_refresh",
        "Publish the expanded catalog.",
        json!({"type":"object","additionalProperties":false}),
    )];
    if expanded {
        tools.push(Tool::new(
            "acme_status",
            "Read Acme status.",
            json!({"type":"object","additionalProperties":false}),
        ));
    }
    Arc::new(Catalog::new(tools).expect("valid Acme catalog"))
}

impl Handler for Acme {
    fn connected(&self, sender: ChangeSender) {
        *self.0.lock().expect("sender lock") = Some(sender);
    }
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1"))
    }
    fn call(&self, request: Request, _: &Context) -> Outcome {
        let Request::Tool(call) = request else {
            return Outcome::Done;
        };
        if call.name == "acme_refresh" {
            let sender = self.0.lock().expect("sender lock").clone();
            if sender.is_none_or(|sender| sender.send(Change::Tools(catalog(true))).is_err()) {
                return Outcome::Tool(ToolResult::error(vec![Content::text(
                    "Catalog change could not be queued.",
                )]));
            }
        }
        Outcome::Tool(ToolResult::success(vec![Content::text("Ready.")]))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = stdio::Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    options.session.subscriptions = Some(SubscriptionOptions::default());
    stdio::serve(
        Arc::new(Acme(Mutex::new(None))),
        catalog(false),
        io::BufReader::new(io::stdin()),
        io::stdout(),
        &options,
    )?;
    Ok(())
}
