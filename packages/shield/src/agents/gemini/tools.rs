//! Gemini tool arguments to shared operations. No policy decisions or file I/O.
use super::{text, web};
use crate::agents::ToolMapping;
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn absolute(value: &Value, base: &str) -> Option<String> {
    let value = text(value, 4096)?;
    let path = Path::new(value);
    if path.is_absolute() {
        Some(value.into())
    } else {
        Path::new(base).join(path).to_str().map(str::to_owned)
    }
}

fn file(path: &str, write: bool, recursive: bool) -> Value {
    json!({"kind":"file","action":if write {"write"} else {"read"},"path":path,"recursive":recursive})
}
fn directory(input: &Value, base: &str) -> Option<String> {
    match input.get("dir_path") {
        None => Some(base.into()),
        Some(value) => absolute(value, base),
    }
}

fn glob(pattern: &str, base: &str, names: bool) -> Option<Value> {
    // node-glob extglobs cannot be represented by our shared glob grammar.
    // In particular @(..) and +(..) can name a parent without a literal `..` part.
    if pattern.contains(['(', ')']) {
        return None;
    }
    let mut anchor = if Path::new(pattern).is_absolute() {
        std::path::PathBuf::new()
    } else {
        Path::new(base).to_path_buf()
    };
    let mut filters = Vec::new();
    let mut wildcard = false;
    for part in Path::new(pattern).components() {
        let value = part.as_os_str().to_str()?;
        if value.contains('{')
            && value
                .split(['{', '}', ','])
                .any(|alternative| alternative == "..")
        {
            return None;
        }
        wildcard |= value.contains(['*', '?', '[', ']', '{', '}']);
        if wildcard {
            if value == ".." {
                return None;
            }
            filters.push(value);
        } else {
            anchor.push(part.as_os_str());
        }
    }
    let mut value = file(anchor.to_str()?, false, wildcard);
    if wildcard {
        value["search"] = json!({"names_only":names,"globs":[filters.join("/")],"types":[]});
    }
    Some(value)
}

pub(super) fn operation(tool: &str, input: &Value, base: &str) -> Option<ToolMapping> {
    let explicit = |key, write, recursive| {
        Some(ToolMapping::Operation(file(
            &absolute(&input[key], base)?,
            write,
            recursive,
        )))
    };
    match tool {
        "run_shell_command" => Some(ToolMapping::Operation(
            json!({"kind":"shell","source":text(&input["command"],65536)?,"dialect":if cfg!(windows) {"powershell"} else {"bash"}}),
        )),
        "read_file" => explicit("file_path", false, false),
        "write_file" | "replace" => explicit("file_path", true, false),
        "list_directory" => explicit("dir_path", false, true),
        "glob" => {
            let directory = directory(input, base)?;
            let value = glob(text(&input["pattern"], 4096)?, &directory, true)?;
            Some(ToolMapping::Operation(if input.get("dir_path").is_none() {
                json!({"kind":"batch","operations":[value],"unmodeled":true})
            } else {
                value
            }))
        }
        "grep_search" => {
            text(&input["pattern"], 8192)?;
            let directory = directory(input, base)?;
            let mut value = file(&directory, false, true);
            let includes = match input.get("include_pattern") {
                None => vec![],
                Some(value) => vec![text(value, 4096)?],
            };
            value["search"] = json!({"names_only":input["names_only"].as_bool().unwrap_or(false),"globs":includes,"types":[]});
            Some(ToolMapping::Operation(if input.get("dir_path").is_none() {
                json!({"kind":"batch","operations":[value],"unmodeled":true})
            } else {
                value
            }))
        }
        "read_many_files" => {
            let patterns = input["include"].as_array()?;
            if patterns.is_empty() || patterns.len() > 64 {
                return None;
            }
            let mut operations = vec![];
            let mut unmodeled = false;
            for pattern in patterns {
                let pattern = text(pattern, 4096)?;
                unmodeled |= !Path::new(pattern).is_absolute();
                operations.push(glob(pattern, base, false)?);
            }
            Some(ToolMapping::Operation(
                json!({"kind":"batch","operations":operations,"unmodeled":unmodeled}),
            ))
        }
        "google_web_search" => Some(ToolMapping::Operation(
            json!({"kind":"web","action":"search","query":text(&input["query"],8192)?,"url":null}),
        )),
        "web_fetch" => Some(ToolMapping::Operation(web::operation(input)?)),
        // Current Gemini writes memory through mapped file tools. Do not let
        // the legacy tool bypass protection of the global instruction file.
        "save_memory" => None,
        _ => Some(ToolMapping::Unmapped),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gemini_globs_refuse_extended_parent_patterns() {
        let base = std::env::temp_dir();
        let base = base.to_str().unwrap();
        for pattern in [
            "@(..)/secret",
            "+(..)/secret",
            "**/@(..)/secret",
            "{..,safe}/secret",
        ] {
            assert!(operation("glob", &json!({"pattern":pattern,"dir_path":base}), base).is_none());
            assert!(operation("read_many_files", &json!({"include":[pattern]}), base).is_none());
        }
        assert!(
            operation(
                "glob",
                &json!({"pattern":"src/**/*.rs","dir_path":base}),
                base
            )
            .is_some()
        );
    }
}
