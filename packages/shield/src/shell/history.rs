//! Credential projection for retained native shell decisions and completions.

use super::{Dialect, cmd, powershell, structure};
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) fn redact_http_history(details: &mut Value) -> bool {
    let operation = &details["operation"];
    if operation["kind"] != "shell" {
        return false;
    }
    let Some(source) = operation["source"].as_str() else {
        return false;
    };
    if !source.contains("curl") && !source.contains(['\'', '"', '\\']) {
        return false;
    }
    let Ok(dialect) = serde_json::from_value::<Dialect>(operation["dialect"].clone()) else {
        return false;
    };
    let mut spans = Vec::new();
    let mut echoes = BTreeSet::new();
    collect(source, dialect, 0, &mut |span, parts| {
        spans.push(span);
        echoes.extend(parts.map(str::to_owned));
    });
    if spans.is_empty() {
        return false;
    }
    spans.sort_unstable_by_key(|span| (span.start, span.end));
    let mut redacted = String::new();
    let mut start = 0;
    for span in spans {
        if span.start < start {
            continue;
        }
        redacted.push_str(&source[start..span.start]);
        redacted.push_str("[REDACTED]");
        start = span.end;
    }
    redacted.push_str(&source[start..]);
    let source = source.to_owned();
    details["operation"]["source"] = Value::String(redacted.clone());
    for field in ["original_input", "request", "evaluation"] {
        if let Some(value) = details.get_mut(field) {
            redact_echoes(value, &source, &redacted, &echoes);
        }
    }
    true
}

fn collect(
    source: &str,
    dialect: Dialect,
    depth: usize,
    found: &mut dyn FnMut(structure::Span, [&str; 3]),
) {
    if depth > 4 || source.len() > 65_536 {
        return;
    }
    let checkpoint = || Ok(());
    let node = match dialect {
        Dialect::Posix | Dialect::Bash => structure::parse(source, dialect, &checkpoint).ok(),
        Dialect::Powershell => powershell::parse(source, &checkpoint).ok().flatten(),
        Dialect::Cmd => cmd::parse(source, &checkpoint).ok().flatten(),
        Dialect::Unsupported => None,
    };
    let Some(mut node) = node else {
        return;
    };
    structure::evidence::http_history_secrets(&mut node, |span, raw, literal, secret| {
        found(span, [raw, literal, secret]);
    });
    structure::evidence::embedded_http_sources(&mut node, |span, raw, literal| {
        collect(literal, dialect, depth + 1, &mut |_, parts| {
            found(span, [raw, literal, parts[2]]);
        });
    });
}

fn redact_echoes(value: &mut Value, source: &str, redacted: &str, echoes: &BTreeSet<String>) {
    match value {
        Value::String(text) if text == source => *text = redacted.into(),
        Value::String(text) if echoes.iter().any(|secret| text.contains(secret)) => {
            *text = "[REDACTED]".into();
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                redact_echoes(value, source, redacted, echoes);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_echoes(value, source, redacted, echoes);
            }
        }
        _ => {}
    }
}
