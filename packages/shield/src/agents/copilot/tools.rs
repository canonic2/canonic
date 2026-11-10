//! Copilot CLI tool arguments, captured from its native tool schema.
use super::native_absolute;
use crate::agents::{ToolMapping, text};
use serde_json::{Value, json};
use std::path::Path;

fn fields(input: &Value, allowed: &[&str]) -> Option<()> {
    input
        .as_object()?
        .keys()
        .all(|key| allowed.contains(&key.as_str()))
        .then_some(())
}

fn file(path: &str, write: bool, recursive: bool) -> Value {
    json!({"kind":"file","action":if write {"write"} else {"read"},"path":path,"recursive":recursive})
}

fn shell(tool: &str, input: &Value) -> Option<Value> {
    fields(
        input,
        &[
            "command",
            "description",
            "shellId",
            "mode",
            "detach",
            "initial_wait",
        ],
    )?;
    text(&input["description"], 4096)?;
    if input
        .get("mode")
        .is_some_and(|value| !matches!(value.as_str(), Some("sync" | "async")))
        || input
            .get("shellId")
            .is_some_and(|value| text(value, 4096).is_none())
        || input.get("detach").is_some_and(|value| !value.is_boolean())
        || input
            .get("initial_wait")
            .is_some_and(|value| !value.is_number())
    {
        return None;
    }
    Some(
        json!({"kind":"shell","source":text(&input["command"],65_536)?,"dialect":if tool=="bash" {"bash"} else {"powershell"}}),
    )
}

pub(super) fn operation(tool: &str, input: &Value, cwd: &str) -> Option<ToolMapping> {
    let operation = match tool {
        "bash" | "powershell" => {
            let operation = shell(tool, input)?;
            if input.get("shellId").is_some() {
                json!({"kind":"batch","operations":[operation],"unmodeled":true})
            } else {
                operation
            }
        }
        "view" | "create" | "edit" => {
            fields(
                input,
                match tool {
                    "view" => &["path", "view_range", "forceReadLargeFiles"],
                    "create" => &["path", "file_text"],
                    _ => &["path", "old_str", "new_str"],
                },
            )?;
            if tool == "create" && !input["file_text"].is_string() {
                return None;
            }
            if ["old_str", "new_str"]
                .into_iter()
                .any(|key| input.get(key).is_some_and(|value| !value.is_string()))
            {
                return None;
            }
            if input.get("view_range").is_some_and(|value| {
                value
                    .as_array()
                    .is_none_or(|range| range.len() != 2 || range.iter().any(|line| !line.is_i64()))
            }) || input
                .get("forceReadLargeFiles")
                .is_some_and(|value| !value.is_boolean())
            {
                return None;
            }
            // view can read directory listings; conservatively include the tree.
            file(
                native_absolute(&input["path"])?,
                tool != "view",
                tool == "view",
            )
        }
        "grep" | "glob" => {
            let pattern = text(&input["pattern"], 8192)?;
            if tool == "glob"
                && (Path::new(pattern).is_absolute()
                    || pattern.contains(['(', ')', '{', '}'])
                    || pattern.split(['/', '\\']).any(|part| part == ".."))
            {
                return None;
            }
            fields(
                input,
                if tool == "glob" {
                    &["pattern", "paths"]
                } else {
                    &[
                        "pattern",
                        "paths",
                        "output_mode",
                        "glob",
                        "type",
                        "-i",
                        "-A",
                        "-B",
                        "-C",
                        "-n",
                        "head_limit",
                        "multiline",
                    ]
                },
            )?;
            let paths = match input.get("paths") {
                None => vec![cwd],
                Some(value @ Value::String(_)) => vec![text(value, 4096)?],
                Some(Value::Array(paths)) if !paths.is_empty() && paths.len() <= 64 => paths
                    .iter()
                    .map(|path| text(path, 4096))
                    .collect::<Option<Vec<_>>>()?,
                _ => return None,
            };
            let operations = paths
                .into_iter()
                .map(|path| {
                    let path = if Path::new(path).is_absolute() {
                        Path::new(path).to_path_buf()
                    } else {
                        Path::new(cwd).join(path)
                    };
                    Some(file(path.to_str()?, false, true))
                })
                .collect::<Option<Vec<_>>>()?;
            json!({"kind":"batch","operations":operations})
        }
        "report_intent" | "task_complete" => return Some(ToolMapping::Unmapped),
        // Includes ambiguous flattened MCP names and unsupported editor schemas.
        _ => return None,
    };
    Some(ToolMapping::Operation(operation))
}
