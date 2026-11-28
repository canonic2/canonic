//! MCP result envelopes, ordered image selection and evidence attachments.
use crate::error::Result;
use canonic_mcp_kit::{Content, ToolResult};
use serde_json::{Value, json};
use std::{io::Read, time::Duration};

const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;

fn read_image(path: &str) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Image attachment exceeds its byte limit",
        ));
    }
    Ok(bytes)
}

pub(super) fn tool_result(name: &str, result: Result<Value>, elapsed: Duration) -> Value {
    let (value, is_error) = match result {
        Ok(data) => (
            json!({"ok":true,"data":data,"elapsedMs":elapsed.as_millis()}),
            false,
        ),
        Err(error) => (
            json!({"ok":false,"code":error.code,"message":error.message,"retryable":error.retryable,"details":error.details,"elapsedMs":elapsed.as_millis()}),
            true,
        ),
    };
    let mut rendered = ToolResult::structured(value.clone(), is_error);
    let data = if is_error {
        &value["details"]
    } else {
        &value["data"]
    };
    for (label, attachment) in image_attachments(name, data) {
        let Some(path) = attachment["path"].as_str() else {
            continue;
        };
        if let Some(label) = label {
            rendered.content.push(Content::text(label));
        }
        match read_image(path) {
            Ok(bytes) => {
                rendered.content.push(Content::image(
                    &bytes,
                    attachment["mimeType"].as_str().unwrap_or("image/png"),
                ));
            }
            Err(_) => rendered.content.push(Content::text(
                "Image attachment is unavailable; the original evidence artifact remains recorded.",
            )),
        }
    }
    serde_json::to_value(rendered).expect("Tool result serializes")
}

pub(super) fn image_attachments<'a>(
    name: &str,
    data: &'a Value,
) -> Vec<(Option<String>, &'a Value)> {
    if matches!(name, "browser_status" | "browser_send")
        || matches!(name, "act" | "observe" | "status") && data.get("operation").is_some()
    {
        let result = &data["operation"]["result"];
        let mut attachments = vec![(None, &result["attachment"])];
        if let Some(results) = result["results"].as_array() {
            for (index, step) in results.iter().take(8).enumerate() {
                if step["attachment"]["path"].is_string() {
                    attachments.push((
                        Some(format!("Observation after action {index}")),
                        &step["attachment"],
                    ));
                }
            }
        }
        return attachments;
    }
    if name == "connect" {
        return vec![(None, &data["observation"]["attachment"])];
    }
    // A call naming its target returns the bare screenshot or batch instead
    // of the session envelope; both carry their images.
    if name == "observe" {
        let attachment = if data.get("observation").is_some() {
            &data["observation"]["attachment"]
        } else {
            &data["attachment"]
        };
        return vec![(None, attachment)];
    }
    if name == "act" && data.get("steps").is_some() {
        return image_attachments("batch", data);
    }
    if name == "act" {
        let payload = if data["result"].get("lastAction").is_some() {
            &data["result"]
        } else {
            data
        };
        let mut images = image_attachments("batch", &payload["result"]);
        let fresh = &payload["observation"]["attachment"];
        if fresh["path"].is_string()
            && !images
                .iter()
                .any(|(_, image)| image["path"] == fresh["path"])
        {
            images.push((Some("Current screen after interruption".into()), fresh));
        }
        return images;
    }
    if name == "screenshot" {
        return vec![(None, &data["attachment"])];
    }
    if name != "batch" {
        return vec![];
    }
    let mut attachments = Vec::new();
    if let Some(steps) = data["steps"].as_array() {
        for step in steps.iter().take(8) {
            let attachment = &step["observation"]["attachment"];
            if attachment["path"].is_string() {
                attachments.push((
                    Some(format!(
                        "Observation after step {} ({})",
                        step["index"], step["tool"]
                    )),
                    attachment,
                ));
            }
        }
    }
    let final_image = &data["observation"]["attachment"];
    if final_image["path"].is_string()
        && !attachments
            .iter()
            .any(|(_, a)| a["path"] == final_image["path"])
    {
        attachments.push((None, final_image));
    }
    attachments
}
