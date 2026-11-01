//! Checked object results for all revisions and an array result for modern hosts.

use canonic_mcp_kit::{
    Annotations, Catalog, Client, ModernOptions, RpcError, ServerInfo, Tool, ToolResult, rpc,
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
            return Outcome::Method(Err(RpcError::new(rpc::METHOD_NOT_FOUND, "Unknown method")));
        };
        let value = if call.name == "acme_names" {
            json!(["Acme"])
        } else {
            json!({"greeting":format!("Hello {}", call.arguments["name"].as_str().expect("validated name"))})
        };
        // The driver checks this result against the captured output schema.
        Outcome::Tool(ToolResult::structured(value, false))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let greeting = Tool::new("acme_greet", "Return a greeting for an Acme name.", json!({
        "type":"object", "$defs":{"name":{"type":"string","pattern":"^Acme","maxLength":64}},
        "properties":{"name":{"$ref":"#/$defs/name","minLength":4}},"required":["name"],"additionalProperties":false
    })).with_output_schema(json!({"type":"object","properties":{"greeting":{"type":"string"}},"required":["greeting"],"additionalProperties":false}));
    let names = Tool::new(
        "acme_names",
        "Return Acme names as an array.",
        json!({"type":"object","additionalProperties":false}),
    )
    .with_output_schema(json!({"type":"array","items":{"type":"string"},"maxItems":8}));
    let read_only = Annotations {
        read_only_hint: Some(true),
        ..Annotations::default()
    };
    let catalog = Catalog::new(vec![
        greeting.with_annotations(read_only.clone()),
        names.with_annotations(read_only),
    ])?;
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
