//! MCP metadata and bounded encoding for retained review reports.
use super::{MAX_OUTPUT, rpc};
use canonic_mcp_kit::{
    ReadResourceResult, ResourceCatalog, ResourceContents, ResourceError, ResourceTemplate,
    RpcError,
    stdio::{Context, Outcome},
};
use serde_json::Value;

pub(super) fn catalog() -> Result<ResourceCatalog, ResourceError> {
    let mut template =
        ResourceTemplate::new(crate::review::reports::TEMPLATE, "Policy review reports");
    template.description = Some("Read the comparison or evidence of a retained proposal using its returned report_resources URI. Inspection grants no authority.".into());
    template.mime_type = Some("application/json".into());
    ResourceCatalog::new(vec![], vec![template])
}

pub(super) fn read(review: &crate::review::Context, params: &Value, context: &Context) -> Outcome {
    let uri = params["uri"].as_str().unwrap_or_default();
    let report = match crate::review::reports::read(review, uri, &|| {
        context
            .check()
            .map_err(|_| crate::policy::Error::Interrupted)
    }) {
        Ok(report) => report,
        Err(crate::review::Error::Missing) => return Outcome::ResourceNotFound(uri.into()),
        Err(crate::review::Error::Policy(crate::policy::Error::Interrupted)) => {
            return Outcome::Method(Err(RpcError::new(
                rpc::REQUEST_CANCELLED,
                "Request cancelled",
            )));
        }
        Err(_) => {
            return Outcome::Method(Err(RpcError::new(
                rpc::INTERNAL_ERROR,
                "Review report unavailable",
            )));
        }
    };
    let result = canonic_mcp_kit::json::encode_checked(&report, MAX_OUTPUT, &|| context.check())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| ResourceContents::text(uri, Some("application/json".into()), text).ok())
        .and_then(|content| ReadResourceResult::new(vec![content]).ok());
    match result {
        Some(result) => Outcome::Resource(result),
        None => Outcome::Method(Err(RpcError::new(
            rpc::INTERNAL_ERROR,
            "Review report exceeds output bounds or was interrupted",
        ))),
    }
}
