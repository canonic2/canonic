//! Map supplied QA facts into shared settled-document blocks without probing resources.
use canonic_tui_kit::output::{Block, Document};
use serde_json::Value;

pub(super) fn result(title: &str, value: &Value) -> Document {
    let state = if value["ok"] == false {
        Some("failed")
    } else {
        value["state"].as_str().or_else(|| value["phase"].as_str())
    };
    let mut doc = Document(vec![Block::Heading(
        state.map_or_else(|| title.to_owned(), |state| format!("{title}: {state}")),
    )]);
    let signing = &value["runnerSetup"]["signing"];
    if signing["source"] == "saved"
        && let Some(team) = signing["runnerTeamId"].as_str()
    {
        doc.0
            .push(Block::Text(format!("Using saved signing team {team}")));
    }
    append(&mut doc, "", value);
    if has_metadata(value) {
        doc.0.push(Block::Text(
            "Use --json for complete capabilities, protocol schemas and timing metadata.".into(),
        ));
    }
    doc
}

pub(super) fn append(doc: &mut Document, name: &str, value: &Value) {
    if name == "recoveryCommand"
        && let Some(command) = command_line(value)
    {
        doc.0.push(Block::Heading("Recovery command".into()));
        doc.0.push(Block::Text(command));
        return;
    }
    match value {
        Value::Object(object) => {
            if !name.is_empty() {
                doc.0.push(Block::Heading(label(name)));
            }
            let fields: Vec<_> = object
                .iter()
                .filter(|(key, value)| !metadata(key) && !value.is_object() && !value.is_array())
                .map(|(key, value)| {
                    (
                        label(key),
                        scalar(
                            if key == "data"
                                && object.get("type").and_then(Value::as_str) == Some("image")
                            {
                                "base64"
                            } else {
                                key
                            },
                            value,
                        ),
                    )
                })
                .collect();
            if !fields.is_empty() {
                doc.0.push(Block::Fields(fields));
            }
            for (key, value) in object
                .iter()
                .filter(|(key, value)| !metadata(key) && (value.is_object() || value.is_array()))
            {
                append(doc, key, value);
            }
            if object.is_empty() {
                doc.0.push(Block::Text("None".into()));
            }
        }
        Value::Array(rows) => append_array(doc, name, rows),
        _ => doc
            .0
            .push(Block::Fields(vec![(label(name), scalar(name, value))])),
    }
}

fn append_array(doc: &mut Document, name: &str, rows: &[Value]) {
    if !name.is_empty() {
        doc.0
            .push(Block::Heading(format!("{} · {}", label(name), rows.len())));
    }
    if rows.is_empty() {
        doc.0.push(Block::Text("None".into()));
        return;
    }
    let columns = columns(name);
    if !columns.is_empty() && rows.iter().all(Value::is_object) {
        let present: Vec<_> = columns
            .iter()
            .copied()
            .filter(|key| rows.iter().any(|row| row.get(*key).is_some()))
            .collect();
        if !present.is_empty() {
            doc.0.push(Block::Table {
                headers: present.iter().map(|key| label(key)).collect(),
                rows: rows
                    .iter()
                    .map(|row| present.iter().map(|key| scalar(key, &row[*key])).collect())
                    .collect(),
            });
            for (index, row) in rows.iter().enumerate() {
                if matches!(name, "tools" | "missing") {
                    continue;
                }
                let rest: serde_json::Map<_, _> = row
                    .as_object()
                    .unwrap()
                    .iter()
                    .filter(|(key, value)| {
                        !metadata(key)
                            && (!present.contains(&key.as_str())
                                || value.is_object()
                                || value.is_array())
                    })
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                if !rest.is_empty() {
                    append(
                        doc,
                        &format!("{} {} details", label(name), index + 1),
                        &Value::Object(rest),
                    );
                }
            }
            if name == "tools" {
                doc.0.push(Block::Text(
                    "Complete tool schemas: testrun mcp serve --check --json".into(),
                ));
            }
            return;
        }
    }
    for (index, row) in rows.iter().enumerate() {
        if row.is_object() || row.is_array() {
            append(doc, &format!("{} {}", label(name), index + 1), row);
        } else {
            doc.0.push(Block::Text(scalar(name, row)));
        }
    }
}

fn command_line(value: &Value) -> Option<String> {
    let program = value["program"].as_str()?;
    let args = value["args"].as_array()?;
    let words: Option<Vec<_>> = std::iter::once(Some(program))
        .chain(args.iter().map(Value::as_str))
        .map(|word| {
            word.map(|word| {
                if !word.is_empty()
                    && word
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_./:-".contains(c))
                {
                    word.to_owned()
                } else {
                    format!("'{}'", word.replace('\'', "'\\''"))
                }
            })
        })
        .collect();
    words.map(|words| words.join(" "))
}

fn columns(name: &str) -> &'static [&'static str] {
    match name {
        "targets" => &["name", "targetId", "platform", "state", "readiness"],
        "apps" => &["name", "appId", "version", "build"],
        "sessions" => &["sessionId", "name", "state", "targetId"],
        "browsers" => &["engine", "state", "hostSupported", "downloadBytes"],
        "blockers" => &["kind", "sessionId", "targetId", "reason"],
        "elements" => &["elementId", "role", "label", "identifier"],
        "tools" => &["name", "description"],
        "missing" => &["id", "bytes"],
        _ => &[],
    }
}
fn scalar(key: &str, value: &Value) -> String {
    match value {
        Value::Null => "unknown".into(),
        Value::Bool(value) => if *value { "yes" } else { "no" }.into(),
        Value::String(text)
            if key.to_ascii_lowercase().contains("base64") || text.starts_with("data:image/") =>
        {
            format!(
                "Inline attachment · {} encoded bytes · use --json for data",
                text.len()
            )
        }
        Value::String(text) if text.is_empty() => "empty".into(),
        Value::String(text) if matches!(key, "state" | "phase" | "outcome" | "status") => {
            text.replace('_', " ")
        }
        Value::String(text) => text.clone(),
        Value::Object(_) | Value::Array(_) => "See details".into(),
        _ => value.to_string(),
    }
}

fn metadata(key: &str) -> bool {
    matches!(
        key,
        "capabilities"
            | "captureTimingsMs"
            | "runnerTimingsMs"
            | "timingsMs"
            | "inputSchema"
            | "outputSchema"
    )
}
fn has_metadata(value: &Value) -> bool {
    match value {
        Value::Object(object) => object
            .iter()
            .any(|(key, value)| metadata(key) || has_metadata(value)),
        Value::Array(values) => values.iter().any(has_metadata),
        _ => false,
    }
}
fn label(key: &str) -> String {
    // Identity maps contain team IDs, target addresses and paths, not field names.
    if key.chars().next().is_some_and(char::is_uppercase) || key.contains([':', '/', '.']) {
        return key.to_owned();
    }
    match key {
        "runnerTeamId" => "Signing team ID".into(),
        "targetId" => "Target ID".into(),
        "appId" => "App ID".into(),
        "nextAction" => "Next action".into(),
        "manualAction" => "Recovery".into(),
        "recoveryCommand" => "Recovery command".into(),
        _ => {
            let mut text = String::new();
            for (index, c) in key.chars().enumerate() {
                if c == '_' || c == '-' {
                    text.push(' ');
                } else if index == 0 {
                    text.extend(c.to_uppercase());
                } else if c.is_uppercase() {
                    text.push(' ');
                    text.extend(c.to_lowercase());
                } else {
                    text.push(c);
                }
            }
            text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn human_lists_keep_identities_nested_readiness_and_failures() {
        for (name, id) in [
            ("targets", "targetId"),
            ("apps", "appId"),
            ("sessions", "sessionId"),
            ("elements", "elementId"),
            ("blockers", "targetId"),
        ] {
            let value = json!({name:[{id:"Acme-123", "name":"Acme", "state":"partial", "readiness":{"reason":"Developer Mode required"}}], "unavailable":[{"kind":"device", "error":{"code":"HOST_IO", "message":"USB service unavailable"}}]});
            let wide = result("Inventory", &value).render(100);
            assert!(
                wide.contains("Acme-123")
                    && wide.contains("Developer Mode required")
                    && wide.contains("USB service unavailable"),
                "{wide}"
            );
            let narrow = result("Inventory", &value).render(24);
            assert!(
                narrow.contains("Acme-123") && narrow.contains("partial"),
                "{narrow}"
            );
        }
    }
    #[test]
    fn recovery_commands_are_copyable_and_inline_images_are_summarized() {
        let value = json!({"recoveryCommand":{"program":"chmod","args":["700","/tmp/Acme Settings"]},"image":{"type":"image","data":"acme-encoded-pixels","mimeType":"image/png"}});
        let human = result("Recovery", &value).render(100);
        assert!(human.contains("chmod 700 '/tmp/Acme Settings'"), "{human}");
        assert!(human.contains("use --json for data"));
        assert!(!human.contains("acme-encoded-pixels"));
        let choices = result(
            "Signing",
            &json!({"availableTeams":{"ACME000001":"Acme Team"}}),
        )
        .render(100);
        assert!(choices.contains("ACME000001"), "{choices}");
    }
}
