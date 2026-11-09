//! Builds MCP tool results from complete evidence: a short summary, the
//! selector text and the evidence as structured content.
use super::{MAX_OUTPUT, messages};
use canonic_mcp_kit::{
    Content, Interrupted, ToolResult,
    json::{self, EncodeError},
};
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

/// Optional cached guidance is appended only after the primary operation has
/// succeeded. It is discarded before essential data on budget/cancellation or
/// unavailable presentation storage.
pub(super) fn with_pack_notice(
    review: &crate::review::Context,
    name: &str,
    value: Value,
    failed: bool,
    check: &dyn Fn() -> Result<(), Interrupted>,
) -> ToolResult {
    if !failed
        && matches!(name, "status" | "pack_list")
        && let Some(pending) =
            crate::review::pack_updates::pending(review, "mcp", &value["data"]["pack_updates"])
    {
        let mut offered = value.clone();
        offered["data"]["pack_update_notice"] = pending.value.clone();
        if crate::json::fits(&offered, MAX_OUTPUT) {
            let result = tool_result(name, review.agent.as_deref(), offered, failed, check);
            if !result.content.is_empty()
                && check().is_ok()
                && result
                    .structured_content
                    .as_ref()
                    .is_some_and(|v| v["data"]["pack_update_notice"].is_object())
                && json::measure(&result, MAX_OUTPUT, check).is_ok()
                && pending.commit()
            {
                return result;
            }
        }
    }
    tool_result(name, review.agent.as_deref(), value, failed, check)
}

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
    let mut content = vec![
        Content::text(messages::summary(name, agent, &value)),
        Content::text(text),
    ];
    for resource in value["data"]["report_resources"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Ok(resource) = serde_json::from_value::<canonic_mcp_kit::Resource>(resource.clone())
            && let Ok(link) = Content::resource_link(resource)
        {
            content.push(link);
        }
    }
    ToolResult {
        meta: None,
        content,
        structured_content: Some(value),
        is_error: failed,
    }
}

fn oversized(value: Value, failed: bool) -> ToolResult {
    ToolResult {
        meta: None,
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
