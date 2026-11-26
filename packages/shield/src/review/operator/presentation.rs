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
pub(super) mod admin;
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

/// Prints a command's concise human result.
pub(super) fn print(input: &crate::cli::Cli, value: &Value) {
    let output = result(&input.command, value);
    print!("{}", project_hints(output, input.project.as_deref()));
}

fn project_hints(output: String, project: Option<&std::path::Path>) -> String {
    let Some(project) = project else {
        return output;
    };
    let prefix = format!(
        "{} ",
        cli_command([
            "shield".into(),
            "--project".into(),
            project.display().to_string()
        ])
    );
    output
        .split_inclusive('\n')
        .map(|line| {
            if line.starts_with("Next page: ") || line.starts_with("More ") {
                line.replacen("shield ", &prefix, 1)
            } else {
                line.to_owned()
            }
        })
        .collect()
}

fn shows_pack_updates(command: &PublicCommand) -> bool {
    matches!(
        command,
        PublicCommand::Pack(crate::cli::public::Pack {
            command: PackCommand::List { .. }
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
    let mut output = render(command, value);
    let mut hints = std::collections::BTreeSet::new();
    preview_hints(value, &mut hints);
    for hint in hints {
        if !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&hint);
        output.push('\n');
    }
    if let Some(cursor) = value["next_cursor"].as_str()
        && let Some(mut words) = next_command(command)
    {
        words.extend(["--cursor".into(), cursor.into()]);
        let command = cli_command(words);
        if !output.ends_with('\n') {
            output.push('\n');
        }
        let _ = writeln!(output, "Next page: {command}");
    }
    output
}

fn cli_command(words: impl IntoIterator<Item = String>) -> String {
    words
        .into_iter()
        .map(|word| {
            if word
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:@=".contains(&byte))
            {
                word
            } else {
                format!(
                    "'{}'",
                    if cfg!(windows) {
                        word.replace('\'', "''")
                    } else {
                        word.replace('\'', "'\\''")
                    }
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn preview_hints(value: &Value, hints: &mut std::collections::BTreeSet<String>) {
    if let Some(fields) = value.as_object() {
        for (key, total) in fields {
            let Some(field) = key.strip_suffix("_total") else {
                continue;
            };
            let shown = fields
                .get(field)
                .and_then(|value| {
                    value
                        .as_array()
                        .map(Vec::len)
                        .or_else(|| value.as_object().map(serde_json::Map::len))
                })
                .unwrap_or_default();
            if total.as_u64().is_none_or(|total| total <= shown as u64) {
                continue;
            }
            let Some(route) = fields.get(&format!("{field}_list")) else {
                continue;
            };
            if route["candidate_id"].is_string() {
                continue;
            }
            let Some(tool) = route["tool"].as_str() else {
                continue;
            };
            let command: &[&str] = match tool {
                "status" => &["status"],
                "doctor" => &["doctor"],
                "pack_list" => &["pack", "list"],
                "pack_info" => &["pack", "info"],
                "rule_list" => &["rule", "list"],
                "policy_show" => &["policy", "show"],
                "request_list" => &["request", "list"],
                "history_list" => &["history", "list"],
                "history_show" => &["history", "show"],
                "session_grant_list" => &["session-grant", "list"],
                "policy_draft_list" => &["policy-draft", "list"],
                _ => continue,
            };
            let mut words = vec!["shield".to_owned()];
            words.extend(command.iter().map(|word| (*word).to_owned()));
            for positional in ["reference", "proposal_id", "entry_id"] {
                if let Some(value) = route[positional].as_str() {
                    words.push(value.to_owned());
                }
            }
            for flag in ["view", "section", "session_ref", "pack_selector"] {
                if let Some(value) = route[flag].as_str() {
                    words.extend([format!("--{}", flag.replace('_', "-")), value.to_owned()]);
                }
            }
            words.extend(["--limit".into(), "50".into()]);
            hints.insert(format!("More {field}: {}", cli_command(words)));
        }
        for field in fields.values() {
            preview_hints(field, hints);
        }
    } else if let Some(rows) = value.as_array() {
        for row in rows {
            preview_hints(row, hints);
        }
    }
}

fn next_command(command: &PublicCommand) -> Option<Vec<String>> {
    use crate::cli::public::SessionGrantCommand;
    let (mut words, limit) = match command {
        PublicCommand::Status(args) | PublicCommand::Doctor(args) => {
            let section = args.section.as_ref()?;
            (
                vec![
                    "shield".into(),
                    if matches!(command, PublicCommand::Status(_)) {
                        "status".into()
                    } else {
                        "doctor".into()
                    },
                    "--section".into(),
                    section.clone(),
                ],
                args.page.limit,
            )
        }
        PublicCommand::Pack(pack) => pack_next(&pack.command)?,
        PublicCommand::Suggestion(crate::cli::public::SuggestionCommand::List {
            session_id,
            page,
        }) => {
            let mut words = vec!["shield".into(), "suggestion".into(), "list".into()];
            if let Some(id) = session_id {
                words.extend(["--session-id".into(), id.clone()]);
            }
            (
                words,
                Some(
                    page.limit
                        .unwrap_or(crate::review::page::SUGGESTIONS.default),
                ),
            )
        }
        PublicCommand::PolicyDraft(crate::cli::public::DraftCommand::List { page }) => (
            vec!["shield".into(), "policy-draft".into(), "list".into()],
            page.limit,
        ),
        PublicCommand::Rule(rule) => match &rule.command {
            RuleCommand::List {
                page,
                section,
                pack_selector,
            } => {
                let mut words = vec!["shield".into(), "rule".into(), "list".into()];
                if let Some(section) = section {
                    words.extend(["--section".into(), section.clone()]);
                }
                if let Some(selector) = pack_selector {
                    words.extend(["--pack-selector".into(), selector.clone()]);
                }
                (words, page.limit)
            }
            _ => return None,
        },
        PublicCommand::Registry(registry) => match &registry.command {
            RegistryCommand::List { page } => (
                vec![
                    "shield".into(),
                    "registry".into(),
                    "--scope".into(),
                    if matches!(registry.scope, crate::cli::public::Scope::User) {
                        "user".into()
                    } else {
                        "project".into()
                    },
                    "list".into(),
                ],
                page.limit,
            ),
            _ => return None,
        },
        PublicCommand::Request(request) => match &request.command {
            RequestCommand::List { page } => (
                vec!["shield".into(), "request".into(), "list".into()],
                page.limit,
            ),
            _ => return None,
        },
        PublicCommand::Policy(policy) => policy_next(&policy.command)?,
        PublicCommand::History(history) => history_next(&history.command)?,
        PublicCommand::SessionGrant(grant) => match &grant.command {
            SessionGrantCommand::List {
                session_ref, limit, ..
            } => {
                let mut words = vec!["shield".into(), "session-grant".into(), "list".into()];
                if let Some(value) = session_ref {
                    words.extend(["--session-ref".into(), value.clone()]);
                }
                (words, Some(*limit))
            }
            _ => return None,
        },
        _ => return None,
    };
    words.extend(["--limit".into(), limit.unwrap_or(50).to_string()]);
    Some(words)
}

fn pack_next(command: &PackCommand) -> Option<(Vec<String>, Option<u64>)> {
    Some(match command {
        PackCommand::List { page } => (
            vec!["shield".into(), "pack".into(), "list".into()],
            page.limit,
        ),
        PackCommand::Search { query, page } => (
            vec![
                "shield".into(),
                "pack".into(),
                "search".into(),
                query.clone(),
            ],
            page.limit,
        ),
        PackCommand::Info {
            reference,
            view,
            section,
            page,
        } => {
            let mut words = vec![
                "shield".into(),
                "pack".into(),
                "info".into(),
                reference.clone(),
                "--view".into(),
                view.clone(),
            ];
            if let Some(section) = section {
                words.extend(["--section".into(), section.clone()]);
            }
            (words, page.limit)
        }
        _ => return None,
    })
}

fn policy_next(command: &PolicyCommand) -> Option<(Vec<String>, Option<u64>)> {
    let PolicyCommand::Show {
        proposal_id,
        section,
        page,
    } = command
    else {
        return None;
    };
    let mut words = vec!["shield".into(), "policy".into(), "show".into()];
    if let Some(id) = proposal_id {
        words.push(id.clone());
    }
    if let Some(section) = section {
        words.extend(["--section".into(), section.clone()]);
    }
    Some((words, page.limit))
}

fn history_next(command: &HistoryCommand) -> Option<(Vec<String>, Option<u64>)> {
    Some(match command {
        HistoryCommand::Show {
            entry,
            section,
            page,
        } => {
            let mut words = vec![
                "shield".into(),
                "history".into(),
                "show".into(),
                entry.clone(),
            ];
            if let Some(section) = section {
                words.extend(["--section".into(), section.clone()]);
            }
            (words, page.limit)
        }
        HistoryCommand::List {
            page,
            denied,
            approved,
            agent,
            authorization_kind,
            grant_id,
        } => {
            let mut words = vec!["shield".into(), "history".into(), "list".into()];
            if *denied {
                words.push("--denied".into());
            }
            if *approved {
                words.push("--approved".into());
            }
            for (flag, value) in [
                ("--agent", agent),
                ("--authorization-kind", authorization_kind),
                ("--grant-id", grant_id),
            ] {
                if let Some(value) = value {
                    words.extend([flag.into(), value.clone()]);
                }
            }
            (words, page.limit)
        }
        HistoryCommand::Configure { .. } => return None,
    })
}

fn render(command: &PublicCommand, value: &Value) -> String {
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
        PublicCommand::Setup { .. } => setup::project_setup(value),
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
        PublicCommand::Docker(_) => format!(
            "Docker environment preview\n{}",
            serde_json::to_string_pretty(&value["docker"]).expect("Docker preview")
        ),
        PublicCommand::Status(args) | PublicCommand::Doctor(args) if args.section.is_some() => format!("{}\n",serde_json::to_string_pretty(value).expect("JSON page")),
        PublicCommand::Status(_) => activity::status(value, false),
        PublicCommand::Doctor(_) => activity::status(value, true),
        PublicCommand::Docs(docs) => {
            let (name, args) = super::diagnostics::docs_arguments(&docs.command);
            canonic_mcp_kit::docs::human(
                &format!("shield docs {}", name.trim_start_matches("docs_")),
                &args,
                value,
            )
        }
        PublicCommand::Policy(input) => match input.command {
            PolicyCommand::Show { proposal_id: None, .. } => policy::review(&serde_json::json!({"operation":"pending proposals","proposals":value["proposals"]})).expect("proposal list"),
            PolicyCommand::Show { .. } => format!(
                "{}\n",
                serde_json::to_string_pretty(value).expect("JSON page")
            ),
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
            RuleCommand::List { section:Some(_), .. } => format!("{}\n",serde_json::to_string_pretty(value).expect("JSON page")),
            RuleCommand::List { .. } => policy::rules(value),
            _ => policy::receipt(value),
        },
        PublicCommand::Pack(pack) => pack_result(&pack.command, value),
        PublicCommand::Registry(registry) => match registry.command {
            RegistryCommand::List { .. } => registries::list(value),
            _ => registries::receipt(value),
        },
        PublicCommand::Request(request) => match request.command {
            RequestCommand::List { .. } => activity::approvals(value),
            RequestCommand::Show { .. } => activity::request(value),
            RequestCommand::Approve { .. } | RequestCommand::Deny { .. } => {
                activity::decided(value)
            }
        },
        PublicCommand::History(history) => match history.command {
            HistoryCommand::List { .. } => activity::history(value),
            HistoryCommand::Show { section:Some(_), .. } | HistoryCommand::Configure { .. } => fallback(value),
            HistoryCommand::Show { .. } => activity::entry(value),
        },
        PublicCommand::Suggestion(_) | PublicCommand::PolicyDraft(_) => format!(
            "{}\n",
            serde_json::to_string_pretty(value).expect("JSON page")
        ),
        PublicCommand::Internal(_) => fallback(value),
    }
}

fn pack_result(command: &PackCommand, value: &Value) -> String {
    match command {
        PackCommand::List { .. } => packs::list(value),
        PackCommand::Show { .. } => packs::show(value),
        PackCommand::Restore => {
            let restored = value["fetched"].as_array().map_or(0, Vec::len);
            let failures = value["fetch_failures"].as_array().into_iter().flatten();
            let mut output = format!(
                "Restored {}.\n",
                plural(restored, "locked pack", "locked packs")
            );
            for failure in failures {
                let _ = writeln!(
                    output,
                    "{}: {}",
                    label(&failure["release"]),
                    label(&failure["error"])
                );
            }
            if let Some(action) = value["next_action"].as_str() {
                let _ = writeln!(output, "{action}");
            }
            output
        }
        PackCommand::Search { .. } => packs::search(value),
        PackCommand::Info { view, .. } if view == "settings" => format!(
            "{}\n",
            serde_json::to_string_pretty(value).expect("JSON page")
        ),
        PackCommand::Info { .. } => packs::info(value),
        PackCommand::Add { .. }
        | PackCommand::Remove { .. }
        | PackCommand::Configure { .. }
        | PackCommand::Update { .. } => policy::receipt(value),
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
    fn next_page_command_preserves_project_filter_and_limit() {
        use clap::Parser;
        let input = crate::cli::Cli::try_parse_from([
            "shield",
            "--project",
            "/acme/example folder",
            "history",
            "list",
            "--denied",
            "--agent",
            "codex",
            "--limit",
            "7",
        ])
        .unwrap();
        let output = project_hints(
            result(
                &input.command,
                &json!({"entries":[],"next_cursor":"opaque"}),
            ),
            input.project.as_deref(),
        );
        assert!(output.contains("Next page: shield --project '/acme/example folder' history list --denied --agent codex --limit 7 --cursor opaque"),"{output}");
    }

    #[test]
    fn pack_updates_fresh_list_and_notice_do_not_repeat_the_release_list() {
        let command = PublicCommand::Pack(crate::cli::public::Pack {
            command: PackCommand::List {
                page: crate::cli::public::PageArgs::default(),
            },
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
        let status = PublicCommand::Status(crate::cli::public::InspectionArgs::default());
        assert!(!result(&status, &value).contains("Pack updates"));
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
