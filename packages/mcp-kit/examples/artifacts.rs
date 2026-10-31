//! Return an unlisted report link and an embedded summary through stdio.

use canonic_mcp_kit::{
    Annotations, Catalog, Client, Content, ModernOptions, ReadResourceResult, Resource,
    ResourceCatalog, ResourceContents, ResourceError, RpcError, ServerInfo, Tool, ToolResult, rpc,
    stdio::{self, Context, Handler, Outcome, Request},
};
use serde_json::json;
use std::{io, sync::Arc};

const REPORT_URI: &str = "acme://reports/latest";
const REPORT: &str = "All checks completed.\n";

struct Acme;

fn artifacts() -> Result<ToolResult, ResourceError> {
    let mut report = Resource::new(REPORT_URI, "Report");
    report.mime_type = Some("text/plain".into());
    report.size = Some(REPORT.len() as u64);
    Ok(ToolResult::success(vec![
        Content::text("Checks passed. Read the report for details."),
        Content::resource_link(report)?,
        Content::resource(ResourceContents::text(
            "acme://summary",
            Some("text/plain".into()),
            "Passed",
        )?),
        Content::resource(ResourceContents::blob(
            "acme://evidence",
            Some("application/octet-stream".into()),
            &[0, 255, 42],
        )?),
    ]))
}

fn internal(error: &ResourceError) -> Outcome {
    Outcome::Method(Err(RpcError::new(rpc::INTERNAL_ERROR, error.to_string())))
}

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1"))
    }

    fn call(&self, request: Request, _: &Context) -> Outcome {
        match request {
            Request::Tool(_) => artifacts().map_or_else(|error| internal(&error), Outcome::Tool),
            Request::Method { method, params } if method == "resources/read" => {
                let uri = params["uri"].as_str().expect("validated URI");
                // A real resolver checks caller authorization and artifact lifetime here.
                if uri != REPORT_URI {
                    return Outcome::ResourceNotFound(uri.into());
                }
                ResourceContents::text(uri, Some("text/plain".into()), REPORT)
                    .and_then(|content| ReadResourceResult::new(vec![content]))
                    .map_or_else(|error| internal(&error), Outcome::Resource)
            }
            _ => Outcome::Method(Err(RpcError::new(rpc::METHOD_NOT_FOUND, "Unknown method"))),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::new(vec![
        Tool::new(
            "acme_report",
            "Return the latest report link and summary.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        )
        .with_annotations(Annotations {
            read_only_hint: Some(true),
            ..Annotations::default()
        }),
    ])?;
    let mut options = stdio::Options::default();
    // Empty discovery is valid: generated artifacts need not appear in resources/list.
    options.session.resources = Some(Arc::new(ResourceCatalog::new(vec![], vec![])?));
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
