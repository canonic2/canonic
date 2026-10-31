//! Shield's bundled guides, served through canonic-mcp-kit's shared
//! documentation tools, and their mapping into Shield's result envelope.
use super::{Checkpoint, Error};
use canonic_mcp_kit::{
    Interrupted,
    docs::{Bundle, DocsError, Documentation},
};
use serde_json::Value;
use std::sync::LazyLock;

static GUIDES: Bundle = canonic_mcp_kit::include_docs!();
static DOCS: LazyLock<Documentation> =
    LazyLock::new(|| Documentation::new(&GUIDES, "shield", "Shield", env!("CARGO_PKG_VERSION")));

/// Whether `name` is one of the documentation tools.
pub(super) fn handles(name: &str) -> bool {
    DOCS.handles(name)
}

/// The documentation tool definitions, as listing entries. Building them does
/// not read the bundle.
pub(super) fn tools() -> Vec<Value> {
    DOCS.tools()
        .iter()
        .map(|tool| serde_json::to_value(tool).expect("tool definitions serialize"))
        .collect()
}

/// Runs a call to one of the bundled documentation tools.
pub(super) fn call(
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, DocsError> {
    call_with(&DOCS, name, args, checkpoint)
}

/// Runs a documentation call, polling Shield's request checkpoint.
pub(super) fn call_with(
    docs: &Documentation,
    name: &str,
    args: &Value,
    checkpoint: Checkpoint<'_>,
) -> Result<Value, DocsError> {
    let check = || checkpoint().map_err(|_| Interrupted);
    docs.call(name, args, &check).unwrap_or_else(|| {
        Err(DocsError::InvalidArguments(
            "Unknown documentation tool.".into(),
        ))
    })
}

/// The code and recovery-bearing message of a documentation failure.
pub(super) fn error_fields(error: &DocsError) -> (&'static str, String) {
    match error {
        DocsError::InvalidArguments(message) => ("invalid_input", message.clone()),
        DocsError::NotFound { .. } => ("not_found", error.to_string()),
        DocsError::Cancelled => super::error_fields(&Error::Cancelled),
        DocsError::Unavailable => ("documentation_unavailable", error.to_string()),
    }
}
