//! Builds MCP tool results from complete evidence: a short summary, the
//! selector text and the evidence as structured content.
use super::{MAX_OUTPUT, messages};
use canonic_mcp::{
    Content, Interrupted, ToolResult,
    json::{self, EncodeError},
};
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

/// The tool result for `value`, checked against the output limit with `check`
/// at each write. Evidence that cannot fit is returned whole so the driver
/// answers with its output-limit error.
pub(super) fn tool_result(
    name: &str,
    agent: Option<&str>,
    value: Value,
    failed: bool,
    check: &dyn Fn() -> Result<(), Interrupted>,
) -> ToolResult {
    match json::measure(&value, MAX_OUTPUT, check) {
        Ok(_) => {}
        Err(EncodeError::TooLarge { .. }) => return oversized(value, failed),
        Err(_) => return interrupted(name, agent),
    }
    let text = match json::encode_checked(&messages::text_data(name, &value), MAX_OUTPUT, check)
        .map(String::from_utf8)
    {
        Ok(Ok(text)) => text,
        Err(EncodeError::TooLarge { .. }) => return oversized(value, failed),
        Ok(Err(_)) | Err(_) => return interrupted(name, agent),
    };
    ToolResult {
        content: vec![
            Content::text(messages::summary(name, agent, &value)),
            Content::text(text),
        ],
        structured_content: Some(value),
        is_error: failed,
    }
}

fn oversized(value: Value, failed: bool) -> ToolResult {
    ToolResult {
        content: Vec::new(),
        structured_content: Some(value),
        is_error: failed,
    }
}

fn interrupted(name: &str, agent: Option<&str>) -> ToolResult {
    let cancelled = json!({"error":crate::review::mcp::cancelled_error()});
    ToolResult::error(vec![Content::text(messages::summary(
        name, agent, &cancelled,
    ))])
}
