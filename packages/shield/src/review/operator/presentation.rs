//! Concise human output for operator commands and confirmation reviews.
//! JSON remains the complete structured interface; these views lead with the
//! outcome and summarize, following the CLI output contract.
use crate::cli::{
    PublicCommand,
    public::{
        HistoryCommand, PackCommand, PolicyCommand, RegistryCommand, RequestCommand, RuleCommand,
    },
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
    if let Some(message) = value["session_grant_review"].as_str() {
        return format!(
            "{}\n",
            message
                .lines()
                .map(|line| label(&Value::String(line.into())))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
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

fn shows_pack_updates(command: &PublicCommand) -> bool {
    matches!(
        command,
        PublicCommand::Status
            | PublicCommand::Pack(crate::cli::public::Pack {
                command: PackCommand::List
            })
    )
}

pub(super) fn pack_update_notice(command: &PublicCommand, notice: &Value) -> String {
    if shows_pack_updates(command) {
        crate::review::pack_updates::CLI_NEXT_STEP.into()
    } else {
        notice["message"].as_str().unwrap_or_default().to_owned()
    }
}

fn result(command: &PublicCommand, value: &Value) -> String {
    let mut output = primary_result(command, value);
    if shows_pack_updates(command)
        && let Some(updates) = value.get("pack_updates")
    {
        let _ = writeln!(
            output,
            "{}",
            crate::review::pack_updates::inspection_summary(updates)
        );
    }
    output
}

fn primary_result(command: &PublicCommand, value: &Value) -> String {
    match command {
        PublicCommand::Init { .. } => setup::initialized(value),
        PublicCommand::Mcp(_) if value["dry_run"] == true => {
            format!("{}Dry run; nothing was written.\n", review(value))
        }
        PublicCommand::Mcp(_) => setup::configured(value),
        PublicCommand::Sdk(_) => format!(
            "Registered the reviewed read-only SDK profile.\nBinary SHA-256: {}\nAdapter mapping SHA-256: {}\nPolicy lineage: {}\nSave the reviewed binary SHA-256 as binaryDigest in trusted application configuration. Policy permissions are unchanged.\n",
            label(&value["binary_digest"]),
            label(&value["mapping_digest"]),
            label(&value["policy_lineage"])
        ),
        PublicCommand::Protection(_) => setup::protection(value),
        PublicCommand::Enable | PublicCommand::Disable => {
            let mut output = format!(
                "Shield is {} for this project.\n",
                if value["enabled"] == true {
                    "enabled"
                } else {
                    "disabled; protected controls remain guarded"
                }
            );
            if value["restart_required"] == true {
                output.push_str("Restart connected agents to refresh cached permission rules.\n");
            }
            if value["repair_needed"] == true {
                output.push_str("Permission rules need repair: run shield mcp setup.\n");
            }
            output
        }
        PublicCommand::SessionGrant(_) => session_grant(value),
        PublicCommand::Status => activity::status(value, false),
        PublicCommand::Doctor => activity::status(value, true),
        PublicCommand::Docs(_) => documentation(value),
        PublicCommand::Policy(input) => match input.command {
            PolicyCommand::Evaluate { .. } => format!(
                "Decision: {}\nPolicy: {}\nPreview only; no action authorized.",
                label(&value["decision"]),
                short(&value["policy_revision"])
            ),
            PolicyCommand::Compare { .. } => format!(
                "Policy comparison: {}\n{}\nPreview only; no action authorized.",
                label(&value["relation"]),
                label(&value["coverage"])
            ),
            _ => policy::policy(value),
        },
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
            HistoryCommand::Configure { .. } => fallback(value),
        },
        PublicCommand::Internal(_) => fallback(value),
    }
}

fn documentation(value: &Value) -> String {
    if let Some(content) = value["content"].as_str() {
        return content
            .lines()
            .map(|line| label(&Value::String(line.into())))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(rows) = value["documents"].as_array() {
        return rows
            .iter()
            .map(|row| {
                format!(
                    "{} · {} · {} lines",
                    label(&row["path"]),
                    label(&row["title"]),
                    label(&row["total_lines"])
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(rows) = value["matches"].as_array() {
        if rows.is_empty() {
            return "No documentation matches.".into();
        }
        return rows
            .iter()
            .map(|row| {
                format!(
                    "{}:{}  {}",
                    label(&row["path"]),
                    label(&row["line"]),
                    label(&row["text"])
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    fallback(value)
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

fn session_grant(value: &Value) -> String {
    let status = |row: &Value| {
        label(if row["grant_status"].is_null() {
            &row["proposal_status"]
        } else {
            &row["grant_status"]
        })
    };
    if let Some(rows) = value["records"].as_array() {
        let mut text = format!("{} session grant records\n", rows.len());
        for row in rows {
            let id = if row["grant_id"].is_null() {
                &row["proposal_id"]
            } else {
                &row["grant_id"]
            };
            let _ = writeln!(
                text,
                "{} · {} · {}",
                label(id),
                status(row),
                label(&row["target"]["agent"])
            );
        }
        text
    } else {
        format!(
            "Session grant: {}\nProposal: {}\nGrant: {}\nIssued: {}\nExpires: {}\n{}\n{}\n",
            status(value),
            label(&value["proposal_id"]),
            label(&value["grant_id"]),
            label(&value["issued_at"]),
            label(&value["expires_at"]),
            value["review"].as_str().map_or_else(String::new, |s| s
                .lines()
                .map(|line| label(&Value::String(line.into())))
                .collect::<Vec<_>>()
                .join("\n")),
            label(&value["next_action"])
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pack_updates_fresh_list_and_notice_do_not_repeat_the_release_list() {
        let command = PublicCommand::Pack(crate::cli::public::Pack {
            command: PackCommand::List,
        });
        let value = json!({"packs":[{"id":"acme/node","version":"1.0.0","profile":"recommended"}],
        "pack_updates":{"availability":"available","fresh_total":1,"items":[{
            "reference":"main/acme/node","installed_version":"1.0.0","available_version":"2.0.0","freshness":"fresh"
        }]}});
        let notice = json!({"message":"Pack updates available: main/acme/node 1.0.0 → 2.0.0. Run shield pack update to review compatible updates."});
        let rendered = format!(
            "stdout:\n{}stderr:\n{}\n",
            result(&command, &value),
            pack_update_notice(&command, &notice)
        );
        insta::assert_snapshot!(rendered, @"
        stdout:
        acme/node 1.0.0 · recommended
        Pack updates available: main/acme/node 1.0.0 → 2.0.0.
        stderr:
        Run shield pack update to review compatible updates.
        ");
        assert_eq!(
            pack_update_notice(&PublicCommand::Status, &notice),
            pack_update_notice(&command, &notice)
        );
    }

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
