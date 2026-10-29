//! Permission scope from retained rule predicates, including both sides of edits.
use super::{arguments, text};
use serde_json::Value;
use std::fmt::Write as _;
#[cfg(test)]
mod tests;

pub(super) fn rule_lines(change: &Value, local: &str) -> Vec<String> {
    let rule = &change["rule"];
    let mut lines = vec![headline(change, local)];
    if change["change"] == "edited" {
        lines.push(format!("Before: {}.", outcome(&change["before"])));
        lines.extend(details(&change["before"]));
        lines.push(format!("After: {}.", outcome(rule)));
    }
    lines.extend(details(rule));
    lines.push(format!(
        "Rule ID: {} ({}).",
        text(&rule["id"]),
        text(&change["change"])
    ));
    if lines
        .iter()
        .any(|line| line.contains('…') || line.contains("not shown"))
    {
        lines.push("Some rule constraints are shortened or omitted above; this is not a complete scope description.".into());
    }
    lines
}

fn headline(change: &Value, local: &str) -> String {
    let rule = &change["rule"];
    let conditions = &rule["conditions"];
    if change["change"] != "removed"
        && let Some(command) = closed_command(rule)
    {
        return format!("{} {command}{local}.", text(&rule["decision"]));
    }
    if conditions["domain"] == "argv"
        && conditions["executable"].is_string()
        && conditions["arguments"]["kind"] == "exact"
    {
        let mut words = vec![literal(&conditions["executable"])];
        words.extend(
            conditions["arguments"]["values"]
                .as_array()
                .into_iter()
                .flatten()
                .take(8)
                .map(literal),
        );
        if conditions["arguments"]["values"]
            .as_array()
            .is_some_and(|values| values.len() > 8)
        {
            words.push("…".into());
        }
        let removed = change["change"] == "removed";
        let action = match (removed, rule["decision"].as_str()) {
            (true, Some("allow")) => "Remove the rule allowing",
            (true, Some("ask")) => "Remove the rule asking before running",
            (true, Some("deny")) => "Remove the rule blocking",
            (false, Some("allow")) => "Allow",
            (false, Some("ask")) => "Ask before running",
            (false, Some("deny")) => "Block",
            _ => "Change permission for",
        };
        let suffix = if rule["decision"] == "allow" {
            " without asking"
        } else {
            ""
        };
        return format!("{action} `{}`{suffix}{local}.", words.join(" "));
    }
    // A fetch rule leads with its hosts, which a short host prompt can show.
    if conditions["domain"] == "web"
        && conditions["action"] == "fetch"
        && change["change"] != "removed"
        && let Some(hosts) = conditions["url"]["hosts"].as_array()
    {
        let action = match rule["decision"].as_str() {
            Some("allow") => "Allow fetches from",
            Some("ask") => "Ask before fetches from",
            Some("deny") => "Block fetches from",
            _ => "Change permission for fetches from",
        };
        let ports = match conditions["url"]["ports"].as_array() {
            None => " on any port".to_owned(),
            Some(ports) => format!(" on port {}", text(&Value::Array(ports.clone()))),
        };
        return format!(
            "{action} {}{ports}{local}.",
            text(&Value::Array(hosts.clone()))
        );
    }
    format!("{} — {}{local}.", text(&change["change"]), outcome(rule))
}

/// Compact scope only for closed option grammars that can be shown completely.
pub(super) fn closed_command(rule: &Value) -> Option<String> {
    let conditions = &rule["conditions"];
    if conditions["domain"] != "argv" {
        return None;
    }
    let executable = conditions["executable"].as_str()?;
    let arguments = &conditions["arguments"];
    if arguments["kind"] != "options"
        || arguments["operands"] != false
        || arguments
            .as_object()?
            .keys()
            .any(|key| !["kind", "prefix", "flags", "values", "operands"].contains(&key.as_str()))
        || !arguments["flags"].as_array()?.is_empty()
        || !arguments["values"].as_array()?.is_empty()
    {
        return None;
    }
    let prefix = arguments["prefix"].as_array()?;
    if prefix.len() > 8 {
        return None;
    }
    let mut words = vec![literal(&conditions["executable"])];
    for selector in prefix {
        if selector.is_string() {
            words.push(literal(selector));
        } else {
            let alternatives = selector["one_of"].as_array()?;
            if selector.as_object()?.len() != 1
                || alternatives.is_empty()
                || alternatives.len() > 8
                || alternatives.iter().any(|value| !value.is_string())
            {
                return None;
            }
            words.push(format!(
                "({})",
                alternatives
                    .iter()
                    .map(literal)
                    .collect::<Vec<_>>()
                    .join("|")
            ));
        }
    }
    let command = words.join(" ");
    if command.chars().count() > 180 || command.contains('…') {
        return None;
    }
    let mut shown = format!("`{command}`; no extra arguments");
    if let Some(cwd) = conditions.get("cwd") {
        let _ = write!(shown, "; working directory: {}", path(cwd));
    }
    if executable == "npm"
        && rule["decision"] == "allow"
        && prefix.first().and_then(Value::as_str).is_some_and(|word| {
            ["run", "run-script", "test", "start", "stop", "restart"].contains(&word)
        })
    {
        shown.push_str("; future script edits and lifecycle hooks inherit this permission");
    }
    Some(shown)
}

// Quote literal words so one argument containing spaces cannot look like two.
fn literal(value: &Value) -> String {
    let value = text(value);
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:@%+=,-".contains(c))
    {
        value
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn outcome(rule: &Value) -> String {
    let conditions = &rule["conditions"];
    let target = match conditions["domain"].as_str() {
        Some("argv") => format!(
            "{} ({})",
            if conditions["executable"].get("root").is_some() {
                reference(&conditions["executable"])
            } else {
                text(&conditions["executable"])
            },
            arguments::describe(&conditions["arguments"])
        ),
        Some("file") => format!("{} files", text(&conditions["action"])),
        Some("web") => format!("web {} requests", text(&conditions["action"])),
        Some("mcp") => format!(
            "MCP server {}, tool {}",
            text(&conditions["server"]),
            text(&conditions["tool"])
        ),
        Some(domain) => format!("{domain} requests"),
        None => "matching requests".into(),
    };
    format!("{} for {target}", text(&rule["decision"]))
}

fn details(rule: &Value) -> Vec<String> {
    let conditions = &rule["conditions"];
    let domain = conditions["domain"].as_str().unwrap_or_default();
    let mut lines = vec![format!(
        "Conditions: {}.",
        predicates(conditions, domain, true)
    )];
    if domain == "argv" {
        lines.insert(
            0,
            format!(
                "Arguments: {}.",
                arguments::describe(&conditions["arguments"])
            ),
        );
    }
    lines.push(format!(
        "Requirements: {}.",
        predicates(&rule["requirements"], domain, false)
    ));
    if rule["requirements"]
        .as_object()
        .is_some_and(|v| !v.is_empty())
    {
        lines.push(format!(
            "If requirements fail or cannot be verified: {}.",
            text(&rule["on_unmet"]["decision"])
        ));
    }
    if let Some(expires) = rule.get("expires_at").filter(|v| !v.is_null()) {
        lines.push(format!("Expires: {}.", text(expires)));
    }
    lines
}

fn predicates(value: &Value, domain: &str, conditions: bool) -> String {
    let mut parts = Vec::new();
    for (key, value) in value.as_object().into_iter().flatten() {
        if conditions
            && [
                "domain",
                "executable",
                "action",
                "server",
                "tool",
                "arguments",
            ]
            .contains(&key.as_str())
        {
            // Command arguments are already in the headline. MCP argument
            // predicates still need their own description.
            if key != "arguments" || domain == "argv" {
                continue;
            }
        }
        parts.push(match key.as_str() {
            "url" => url(value),
            "cwd" => format!("working directory: {}", path(value)),
            "destination" => format!("destination: {}", path(value)),
            "sources" => format!("source paths: {}", quantified(value)),
            "targets" => format!("affected paths: {}", targets(value)),
            "arguments" if domain == "argv" => arguments::describe(value),
            "arguments" | "fields" | "request_fields" => {
                format!("request field constraints: {}", text(value))
            }
            "argument_paths" => argument_paths(value),
            "query" => format!("search query: {}", text(value)),
            "method" => format!("HTTP method: {}", text(value)),
            "source" => format!("shell source: {}", text(value)),
            "dialect" => format!("shell dialect: {}", text(value)),
            "dialect_version" => format!("shell version: {}", text(value)),
            "recursive" => format!("recursive access: {}", text(value)),
            "environment" => format!("environment constraints: {}", text(value)),
            "possible_effects" => format!("possible effects: {}", text(value)),
            _ => format!(
                "additional constraint {}: {}",
                text(&Value::String(key.clone())),
                text(value)
            ),
        });
    }
    if parts.is_empty() {
        "none beyond the scope above".into()
    } else {
        parts.join("; ")
    }
}

fn list(value: &Value, describe: fn(&Value) -> String) -> String {
    let values = value.as_array().map_or(&[][..], Vec::as_slice);
    let mut items: Vec<_> = values.iter().take(8).map(describe).collect();
    if values.len() > 8 {
        items.push(format!("{} more entries not shown", values.len() - 8));
    }
    if items.is_empty() {
        "none".into()
    } else {
        items.join(", ")
    }
}

fn url(value: &Value) -> String {
    format!(
        "URL schemes: {}; hosts: {}; ports: {}; URL path: {}",
        list(&value["schemes"], text),
        list(&value["hosts"], text),
        value
            .get("ports")
            .map_or_else(|| "any".into(), |v| list(v, text)),
        value.get("path").map_or_else(|| "any".into(), text)
    )
}

fn reference(value: &Value) -> String {
    let root = match value["root"].as_str() {
        Some("project") => "project directory",
        Some("read") => "approved read folders",
        Some("write") => "approved write folders",
        Some("absolute") => return text(&value["path"]),
        _ => return "unrecognized path root".into(),
    };
    value
        .get("path")
        .map_or_else(|| root.into(), |path| format!("{root} / {}", text(path)))
}

fn path(value: &Value) -> String {
    let mut parts = Vec::new();
    for (key, label) in [
        ("exact", "exactly"),
        ("within", "within"),
        ("outside", "outside"),
    ] {
        if let Some(value) = value.get(key) {
            parts.push(format!("{label} {}", reference(value)));
        }
    }
    if let Some(excluded) = value.get("exclude") {
        parts.push(format!("excluding {}", list(excluded, reference)));
    }
    if let Some(pattern) = value.get("match") {
        parts.push(format!("path matching {}", text(pattern)));
    }
    parts.join("; ")
}

fn quantified(value: &Value) -> String {
    if let Some(all) = value.get("all") {
        format!("all {}", path(all))
    } else {
        format!("at least one {}", path(&value["any"]))
    }
}

fn target(value: &Value) -> String {
    format!("{}: {}", text(&value["action"]), quantified(value))
}

fn targets(value: &Value) -> String {
    if value.is_array() {
        list(value, target)
    } else {
        target(value)
    }
}

fn argument_paths(value: &Value) -> String {
    let mut parts = vec![format!(
        "argument path grammar: {}",
        arguments::describe(&value["grammar"])
    )];
    for (name, selection) in value["options"].as_object().into_iter().flatten().take(8) {
        parts.push(format!(
            "option {} paths: {}; recursive: {}",
            text(&Value::String(name.clone())),
            path(&selection["path"]),
            text(&selection["recursive"])
        ));
    }
    if value["options"].as_object().is_some_and(|v| v.len() > 8) {
        parts.push("additional option path constraints not shown".into());
    }
    if let Some(selection) = value.get("operands") {
        parts.push(format!(
            "operand paths: {}; recursive: {}",
            path(&selection["path"]),
            text(&selection["recursive"])
        ));
    }
    parts.join("; ")
}
