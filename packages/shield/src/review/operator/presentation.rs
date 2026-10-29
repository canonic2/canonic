//! Concise human output for operator commands and confirmation reviews.
//! JSON remains the complete structured interface; these views lead with the
//! outcome and summarize, following the CLI output contract.
use crate::cli::{
    PublicCommand,
    public::{HistoryCommand, PackCommand, RegistryCommand, RequestCommand, RuleCommand},
};
use serde_json::Value;
use std::fmt::Write;

mod activity;
pub(super) mod errors;
mod packs;
mod policy;
pub(super) use policy::{Card, card};
mod registries;
mod setup;

/// Display text for an untrusted value, with terminal control effects escaped.
fn label(value: &Value) -> String {
    if value.is_null() {
        return "unknown".into();
    }
    value.as_str().map_or_else(
        || value.to_string(),
        |s| {
            s.chars()
                .map(|c| {
                    if c.is_control()
                        || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                    {
                        c.escape_default().to_string()
                    } else {
                        c.to_string()
                    }
                })
                .collect()
        },
    )
}

/// The first 12 hexadecimal characters of a digest; JSON keeps it whole.
fn short(digest: &Value) -> String {
    let text = label(digest);
    let text = text.rsplit(':').next().unwrap_or_default();
    text.get(..12).unwrap_or(text).to_owned()
}

/// `publisher/name` from an identity that may carry a source prefix.
fn pack_name(id: &Value) -> String {
    let id = label(id);
    let parts: Vec<_> = id.rsplitn(3, '/').collect();
    match parts.as_slice() {
        [name, publisher, ..] => format!("{publisher}/{name}"),
        _ => id,
    }
}

/// A literal request as the command line it describes.
fn command_line(request: &Value) -> String {
    let mut text = label(&request["executable"]);
    for argument in request["arguments"].as_array().into_iter().flatten() {
        let argument = label(argument);
        if argument.is_empty() || argument.contains(char::is_whitespace) {
            let _ = write!(text, " '{argument}'");
        } else {
            let _ = write!(text, " {argument}");
        }
    }
    text
}

/// A UTC time from epoch milliseconds.
fn time_ms(value: &Value) -> String {
    value.as_u64().map_or_else(
        || "unknown".into(),
        |ms| crate::evidence::format_utc(ms / 1000),
    )
}

fn plural(count: usize, one: &str, many: &str) -> String {
    match count {
        0 => format!("no {many}"),
        1 => format!("1 {one}"),
        _ => format!("{count} {many}"),
    }
}

/// The review shown in the operator terminal before a change is confirmed.
pub(super) fn review(value: &Value) -> String {
    let mut value = value.clone();
    crate::review::model::redact(&mut value);
    setup::review(&value)
        .or_else(|| policy::review(&value))
        .or_else(|| registries::review(&value))
        .or_else(|| activity::review(&value))
        .unwrap_or_else(|| fallback(&value))
}

/// The progress line of an update, naming the pinned packs it skips.
pub(super) fn update_progress(skipped: &[Value]) -> String {
    if skipped.is_empty() {
        "Checking for newer releases…".into()
    } else {
        format!(
            "Checking for newer releases; keeping pinned {}…",
            pinned_list(skipped)
        )
    }
}

/// Pinned packs by name and version, the first eight and a count of the rest.
fn pinned_list(skipped: &[Value]) -> String {
    let mut text = skipped
        .iter()
        .take(8)
        .map(|row| format!("{} {}", pack_name(&row["id"]), label(&row["version"])))
        .collect::<Vec<_>>()
        .join(", ");
    if skipped.len() > 8 {
        let _ = write!(text, " and {} more", skipped.len() - 8);
    }
    text
}

/// Prints a command's concise human result.
pub(super) fn print(command: &PublicCommand, value: &Value) {
    print!("{}", result(command, value));
}

fn result(command: &PublicCommand, value: &Value) -> String {
    match command {
        PublicCommand::Init { .. } => setup::initialized(value),
        PublicCommand::Mcp(_) if value["dry_run"] == true => {
            format!("{}Dry run; nothing was written.\n", review(value))
        }
        PublicCommand::Mcp(_) => setup::configured(value),
        PublicCommand::Protection(_) => setup::protection(value),
        PublicCommand::Status => activity::status(value, false),
        PublicCommand::Doctor => activity::status(value, true),
        PublicCommand::Cache { .. } => activity::cache(value),
        PublicCommand::Policy(_) => policy::policy(value),
        PublicCommand::Rule(rule) => match rule.command {
            RuleCommand::List => policy::rules(value),
            _ => policy::receipt(value),
        },
        PublicCommand::Pack(pack) => match &pack.command {
            PackCommand::List => packs::list(value),
            PackCommand::Show { .. } => packs::show(value),
            PackCommand::Search { .. } => packs::search(value),
            PackCommand::Info { .. } => packs::info(value),
            PackCommand::Add { .. }
            | PackCommand::Remove { .. }
            | PackCommand::Configure { .. } => policy::receipt(value),
            PackCommand::Update { .. } => {
                let skipped = value["skipped_pinned"]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice);
                let mut output = policy::receipt(value);
                if !skipped.is_empty() {
                    let _ = writeln!(output, "Kept pinned {}.", pinned_list(skipped));
                }
                output
            }
        },
        PublicCommand::Registry(registry) => match registry.command {
            RegistryCommand::List => registries::list(value),
            _ => registries::receipt(value),
        },
        PublicCommand::Request(request) => match request.command {
            RequestCommand::List => activity::approvals(value),
            RequestCommand::Show { .. } => activity::request(value),
            RequestCommand::Approve { .. } | RequestCommand::Deny { .. } => {
                activity::decided(value)
            }
        },
        PublicCommand::History(history) => match history.command {
            HistoryCommand::List { .. } => activity::history(value),
            HistoryCommand::Show { .. } => activity::entry(value),
        },
        PublicCommand::Internal(_) => fallback(value),
    }
}

/// Top-level facts of a result without a dedicated view. Nested data stays in
/// `--json`.
fn fallback(value: &Value) -> String {
    let mut output = String::new();
    let mut nested = false;
    for (key, field) in value.as_object().into_iter().flatten() {
        if key == "schema_version" {
            continue;
        }
        if field.is_object() || field.is_array() {
            nested = true;
        } else {
            let _ = writeln!(
                output,
                "{}: {}",
                label(&Value::String(key.replace('_', " "))),
                label(field)
            );
        }
    }
    if nested {
        output.push_str("Use --json for the complete result.\n");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn untrusted_text_has_terminal_controls_escaped() {
        let text = review(&json!({"operation":"approve once",
            "request":{"request":{"executable":"/bin/echo\u{1b}[2J","arguments":["café.txt","a\u{202e}txt"],"cwd":"/srv/acme"}},
            "scope":"one use"}));
        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains('\u{202e}'));
        assert!(text.contains("café.txt"));
    }

    #[test]
    fn shared_helpers_shorten_identities_digests_and_commands() {
        assert_eq!(short(&json!("blake3-v1:0123456789abcdef")), "0123456789ab");
        assert_eq!(pack_name(&json!("3f2a/acme/git")), "acme/git");
        assert_eq!(pack_name(&json!("acme/git")), "acme/git");
        assert_eq!(
            command_line(&json!({"executable":"git","arguments":["commit","-m","Acme change"]})),
            "git commit -m 'Acme change'"
        );
    }

    #[test]
    fn results_without_a_view_show_only_top_level_facts() {
        let text = fallback(&json!({"schema_version":1,"status":"ready","detail":{"nested":true}}));
        assert_eq!(text, "status: ready\nUse --json for the complete result.\n");
    }
}
