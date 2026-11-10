//! Exact V1 tool fields to shared operations, without execution or policy.
use crate::agents::text;
use serde_json::{Value, json};
use std::path::Path;

fn fields(input: &Value, allowed: &[&str]) -> Option<()> {
    input
        .as_object()?
        .keys()
        .all(|key| allowed.contains(&key.as_str()))
        .then_some(())
}
fn path(value: &Value, cwd: &str) -> Option<String> {
    let path = Path::new(text(value, 4096)?);
    if path.is_absolute() {
        Some(path.to_str()?.into())
    } else {
        Some(Path::new(cwd).join(path).to_str()?.into())
    }
}
fn file(path: &str, write: bool, recursive: bool) -> Value {
    json!({"kind":"file","action":if write{"write"}else{"read"},"path":path,"recursive":recursive})
}

pub(super) fn operation(
    tool: &str,
    input: &Value,
    cwd: &str,
    shell: Option<&str>,
) -> Option<Value> {
    Some(match tool {
        "bash" => {
            fields(input, &["command", "description", "timeout", "workdir"])?;
            text(&input["description"], 4096)?;
            if input
                .get("timeout")
                .is_some_and(|timeout| timeout.as_f64().is_none_or(|timeout| timeout < 0.0))
            {
                return None;
            }
            let operation = json!({"kind":"shell","source":text(&input["command"],65_536)?,"dialect":if shell==Some("powershell"){"powershell"}else{"bash"}});
            json!({"kind":"batch","operations":[operation],"unmodeled":true})
        }
        "read" => {
            fields(input, &["filePath", "offset", "limit"])?;
            if ["offset", "limit"]
                .into_iter()
                .any(|key| input.get(key).is_some_and(|value| !value.is_number()))
            {
                return None;
            }
            file(&path(&input["filePath"], cwd)?, false, true)
        }
        "write" => {
            fields(input, &["filePath", "content"])?;
            input["content"].as_str()?;
            file(&path(&input["filePath"], cwd)?, true, false)
        }
        "edit" => {
            fields(input, &["filePath", "oldString", "newString", "replaceAll"])?;
            input["oldString"].as_str()?;
            input["newString"].as_str()?;
            if input
                .get("replaceAll")
                .is_some_and(|value| !value.is_boolean())
            {
                return None;
            }
            file(&path(&input["filePath"], cwd)?, true, false)
        }
        "glob" | "grep" => {
            let pattern = text(&input["pattern"], 8192)?;
            fields(
                input,
                if tool == "glob" {
                    &["pattern", "path"]
                } else {
                    &["pattern", "path", "include"]
                },
            )?;
            if tool == "glob"
                && (Path::new(pattern).is_absolute()
                    || pattern.contains(['(', ')', '{', '}'])
                    || pattern.split(['/', '\\']).any(|part| part == ".."))
            {
                return None;
            }
            let root = match input.get("path") {
                Some(value) => path(value, cwd)?,
                None => cwd.into(),
            };
            file(&root, false, true)
        }
        "list" => {
            fields(input, &["path", "ignore"])?;
            let root = match input.get("path") {
                Some(value) => path(value, cwd)?,
                None => cwd.into(),
            };
            file(&root, false, true)
        }
        "webfetch" => {
            fields(input, &["url", "format", "timeout"])?;
            json!({"kind":"web","action":"fetch","url":text(&input["url"],8192)?,"query":null})
        }
        "websearch" => {
            fields(
                input,
                &[
                    "query",
                    "numResults",
                    "livecrawl",
                    "type",
                    "contextMaxCharacters",
                ],
            )?;
            json!({"kind":"web","action":"search","query":text(&input["query"],8192)?,"url":null})
        }
        _ => return None,
    })
}
