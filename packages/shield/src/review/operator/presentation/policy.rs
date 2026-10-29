//! Policy views: change reviews with their permission comparison, the active
//! policy, rules, proposals and the receipts of confirmed changes.
use super::{command_line, label, pack_name, plural, short};
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Write};

/// Receipts list at most this many changes before summarizing them.
const MAX_RECEIPT_CHANGES: usize = 3;

pub(super) fn review(value: &Value) -> Option<String> {
    let mut output = String::new();
    match value["operation"].as_str()? {
        "inspect policy" => output = policy(value),
        "change policy" | "adopt manual project edits" => {
            let _ = writeln!(
                output,
                "{}",
                if value["operation"] == "change policy" {
                    "Change policy"
                } else {
                    "Adopt your edits to the .shield project files"
                }
            );
            let lines = changes(&value["changes"], &value["candidate"]);
            if lines.is_empty() {
                let _ = writeln!(output, "  No pack or rule changes.");
            }
            for line in lines {
                let _ = writeln!(output, "  {line}");
            }
            output.push_str(&permissions(&value["comparison"]));
            for line in value["comparison"]["formatting"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let _ = writeln!(output, "{}", label(line));
            }
        }
        "restore project files from active policy" => {
            let _ = writeln!(
                output,
                "Restore the .shield project files from the active policy.\nManual edits that conflict are kept for you to review."
            );
        }
        "reject" => {
            let _ = writeln!(output, "Reject {}", proposal(&value["proposal"]));
        }
        "pending proposals" => {
            let rows: Vec<_> = value["proposals"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["status"] == "pending")
                .collect();
            if rows.is_empty() {
                let _ = writeln!(output, "No proposals are waiting.");
            }
            for row in rows {
                let _ = writeln!(output, "  {}", proposal(row));
            }
        }
        "history settings" => {
            let _ = writeln!(
                output,
                "Request history: {}{}",
                if value["enabled"] == true {
                    format!("on, keeping up to {} entries", label(&value["limit"]))
                } else {
                    "off".into()
                },
                if value["purge"] == true {
                    "; clear the existing history"
                } else {
                    ""
                }
            );
        }
        _ => return None,
    }
    Some(output)
}

/// One line per proposal: identifier, what it changes, who asked and when.
fn proposal(row: &Value) -> String {
    let mut text = format!(
        "{} · {}",
        label(&row["proposal_id"]),
        label(&row["operation"])
    );
    if row["rule_id"].is_string() {
        let _ = write!(text, " {}", label(&row["rule_id"]));
    }
    if row["provenance"]["agent"].is_string() {
        let _ = write!(text, " · from {}", label(&row["provenance"]["agent"]));
    }
    if row["created_at"].is_string() {
        let _ = write!(text, " · {}", label(&row["created_at"]));
    }
    text
}

/// A pending proposal as the operator reviews it: what it changes, why the agent
/// asked, what it was doing, and the complete review behind Details.
pub(in crate::review::operator) struct Card {
    pub(in crate::review::operator) title: String,
    pub(in crate::review::operator) summary: String,
    pub(in crate::review::operator) reason: String,
    pub(in crate::review::operator) resources: Vec<(String, String)>,
    pub(in crate::review::operator) details: String,
    /// Why it can no longer be accepted, when it was made against older policy.
    pub(in crate::review::operator) stale: Option<String>,
}

/// The card for one proposal, from its row, the change review against the
/// active policy and the history entry it came from.
pub(in crate::review::operator) fn card(
    proposal: &Value,
    review_value: &Value,
    requested: Option<&Value>,
    stale: bool,
) -> Card {
    let mut lines = changes(&review_value["changes"], &review_value["candidate"]).into_iter();
    let summary = lines.next().map_or_else(
        || "Change nothing in the active policy".to_owned(),
        // The digest stays in Details; the summary is read and quoted.
        |line| {
            let line = sentence(&line);
            line.split(" · digest ").next().unwrap_or(&line).to_owned()
        },
    );
    let mut resources = Vec::new();
    if let Some(requested) = requested {
        let mut detail = label(&requested["agent"]);
        if let Some(at) = requested["recorded_at"].as_str() {
            let _ = write!(detail, " · {}", at.replace('T', " ").trim_end_matches('Z'));
        }
        resources.push((
            format!("asked after {}", operation(&requested["operation"])),
            detail,
        ));
    }
    for line in lines {
        resources.push((sentence(&line), String::new()));
    }
    let permissions = permissions(&review_value["comparison"]);
    let permissions = permissions
        .trim()
        .trim_start_matches("Permissions: ")
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("; ");
    // The card has no changes "above"; Details holds them.
    let permissions = permissions.replace("check the changes above", "check the details");
    resources.push(("permissions".into(), permissions));
    let mut details = review(review_value).unwrap_or_default();
    for (id, rules) in pack_rules(&review_value["candidate"], &review_value["changes"]) {
        let _ = writeln!(details, "\nRules in {id}");
        for rule in rules {
            let _ = writeln!(details, "  {rule}");
        }
    }
    Card {
        title: label(&proposal["operation"]).replace('_', " "),
        summary,
        reason: proposal["reasons"]["agent_reason"]
            .as_str()
            .map_or_else(|| "The agent gave no reason.".to_owned(), label_text),
        resources,
        details,
        stale: stale
            .then(|| "Made against an older policy; ask the agent to propose it again.".to_owned()),
    }
}

fn label_text(text: &str) -> String {
    label(&Value::String(text.to_owned()))
}

/// A change line as a sentence: `+ rule …` reads `Add rule …`.
fn sentence(line: &str) -> String {
    let (verb, rest) = match line.split_at_checked(2) {
        Some(("+ ", rest)) if rest.starts_with("rule ") => ("Add", rest),
        Some(("+ ", rest)) => ("Install", rest),
        Some(("- ", rest)) => ("Remove", rest),
        Some(("~ ", rest)) => ("Change", rest),
        _ => return line.to_owned(),
    };
    format!("{verb} {rest}")
}

/// What the agent was doing when it asked, in a few words.
fn operation(operation: &Value) -> String {
    let text = match operation["kind"].as_str() {
        Some("shell") => operation["source"].as_str().unwrap_or_default().to_owned(),
        Some("file") => format!(
            "{} {}",
            label(&operation["action"]),
            label(&operation["path"])
        ),
        Some("web") => format!(
            "web {} {}",
            label(&operation["action"]),
            operation["query"]
                .as_str()
                .or(operation["url"].as_str())
                .unwrap_or_default()
        ),
        _ => return "a request".into(),
    };
    let text = label_text(text.lines().next().unwrap_or_default());
    let short: String = text.chars().take(80).collect();
    if short.len() < text.len() {
        format!("`{short}…`")
    } else {
        format!("`{short}`")
    }
}

/// The rules of each pack the change adds or updates, one line each.
fn pack_rules(candidate: &Value, changes: &Value) -> Vec<(String, Vec<String>)> {
    let packs: Vec<String> = changes
        .as_array()
        .into_iter()
        .flatten()
        .filter(|change| change["kind"] == "pack" && change["change"] != "removed")
        .map(|change| label(&change["id"]))
        .collect();
    candidate["packs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|bytes| crate::policy::parse::<Value>(bytes.as_str()?).ok())
        .filter(|pack| packs.contains(&label(&pack["id"])))
        .map(|pack| {
            let rules = pack["rules"]
                .as_array()
                .into_iter()
                .flatten()
                .map(rule)
                .collect();
            (label(&pack["id"]), rules)
        })
        .collect()
}

/// Rule counts and digests of the candidate's pack artifacts, by pack ID.
fn artifacts(candidate: &Value) -> BTreeMap<String, String> {
    candidate["packs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|bytes| crate::policy::parse::<Value>(bytes.as_str()?).ok())
        .map(|pack| (label(&pack["id"]), super::packs::rule_count(&pack)))
        .collect()
}

/// Review lines for each change: `+` added, `-` removed, `~` changed.
fn changes(changes: &Value, candidate: &Value) -> Vec<String> {
    let rules = artifacts(candidate);
    changes
        .as_array()
        .into_iter()
        .flatten()
        .map(|change| {
            let id = label(&change["id"]);
            let details = || {
                let mut text = format!(
                    "{} profile · {}",
                    label(&change["profile"]),
                    rules.get(&id).map_or("0 rules", String::as_str)
                );
                if change["dependency"] == true {
                    text.push_str(" · dependency");
                } else if let Some(mode) = change["mode"].as_str() {
                    let _ = write!(text, " · {mode}");
                }
                let _ = write!(text, " · digest {}", short(&change["digest"]));
                text
            };
            let line = match (change["kind"].as_str(), change["change"].as_str()) {
                (Some("pack"), Some("added")) => {
                    format!("+ {id} {} · {}", label(&change["version"]), details())
                }
                (Some("pack"), Some("removed")) => format!("- {id} {}", label(&change["version"])),
                (Some("pack"), Some("updated")) => format!(
                    "~ {id} {} → {} · {}",
                    label(&change["from"]),
                    label(&change["version"]),
                    details()
                ),
                (Some("pack"), Some("profile")) => format!(
                    "~ {id} profile {} → {}",
                    label(&change["from"]),
                    label(&change["profile"])
                ),
                (Some("pack"), Some("mode")) => format!(
                    "~ {id} {} → {}",
                    label(&change["from"]),
                    label(&change["mode"])
                ),
                (Some("pack"), Some("role")) => format!(
                    "~ {id} {} → {} selection",
                    label(&change["from"]),
                    label(&change["role"])
                ),
                (Some("rule"), Some("added")) => format!("+ rule {}", rule(&change["rule"])),
                (Some("rule"), Some("removed")) => format!("- rule {}", rule(&change["rule"])),
                (Some("rule"), _) => {
                    let (now, was) = (rule(&change["rule"]), rule(&change["before"]));
                    if now == was {
                        format!("~ rule {now} (reason or details changed)")
                    } else {
                        format!(
                            "~ rule {now} (was {})",
                            was.split_once(": ").map_or(was.as_str(), |(_, w)| w)
                        )
                    }
                }
                (Some("access_paths"), _) => {
                    format!("~ access paths: {}", access_paths(&change["after"]))
                }
                (Some("mcp"), _)
                    if change.get("before").is_none() && change.get("after").is_none() =>
                {
                    format!("~ MCP connection {id} changed · terminal setup review required")
                }
                (Some("mcp"), _) => format!(
                    "~ MCP connection {id}: {} → {} · terminal setup review required",
                    label(&change["before"]),
                    label(&change["after"])
                ),
                (Some("variable" | "override"), _) => setting(change),
                (Some("pack"), Some("disabled")) => format!("- {id} (turned off locally)"),
                _ => format!("~ {}", label(&change["kind"])),
            };
            if change["layer"] == "local" {
                format!("{line} · local only")
            } else {
                line
            }
        })
        .collect()
}

/// A variable value or rule override of one pack selection.
fn setting(change: &Value) -> String {
    let what = if change["kind"] == "variable" {
        format!("{} {}", label(&change["id"]), label(&change["name"]))
    } else {
        format!(
            "{} override {}",
            label(&change["id"]),
            label(&change["name"])
        )
    };
    let value = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_owned);
    let mut text = match change["change"].as_str() {
        Some("set") => format!("+ {what} = {}", value(&change["after"])),
        Some("removed") => format!("- {what} (was {})", value(&change["before"])),
        _ => format!(
            "~ {what}: {} → {}",
            value(&change["before"]),
            value(&change["after"])
        ),
    };
    if change["widens"] == true {
        text.push_str(" · widens permissions");
    }
    text
}

pub(super) fn access_paths(paths: &Value) -> String {
    let groups: Vec<_> = paths
        .as_object()
        .into_iter()
        .flatten()
        .map(|(kind, list)| {
            let list: Vec<_> = list.as_array().into_iter().flatten().map(label).collect();
            format!(
                "{kind} {}",
                if list.is_empty() {
                    "none".into()
                } else {
                    list.join(", ")
                }
            )
        })
        .collect();
    if groups.is_empty() {
        "none".into()
    } else {
        groups.join("; ")
    }
}

/// `id: decision summary` for a rule.
pub(super) fn rule(rule: &Value) -> String {
    format!(
        "{}: {} {}",
        label(&rule["id"]),
        decision(&rule["decision"]),
        rule_conditions(rule)
    )
}

pub(super) fn rule_conditions(rule: &Value) -> String {
    let mut text = conditions(&rule["conditions"]);
    if let Some(set) = rule["command_set"].as_str() {
        let _ = write!(text, " · commands in {set}");
    }
    if let Some(group) = rule["invocation"].as_str() {
        let _ = write!(text, " · invocation group {group}");
    }
    text
}

/// A decision, or the decision for each profile.
pub(super) fn decision(value: &Value) -> String {
    match value.as_object() {
        Some(profiles) => profiles
            .iter()
            .map(|(profile, decision)| format!("{profile} {}", label(decision)))
            .collect::<Vec<_>>()
            .join(", "),
        None => label(value),
    }
}

/// What a rule matches, in a few words.
pub(super) fn conditions(conditions: &Value) -> String {
    match conditions["domain"].as_str() {
        Some("argv") => {
            let arguments = &conditions["arguments"];
            let request = serde_json::json!({"executable":conditions["executable"],"arguments":arguments["values"]});
            match arguments["kind"].as_str() {
                Some("exact") => command_line(&request),
                Some("prefix") => format!("{} …", command_line(&request)),
                Some("any") | None => {
                    format!("{} (any arguments)", label(&conditions["executable"]))
                }
                Some(kind) => format!("{} ({kind} arguments)", label(&conditions["executable"])),
            }
        }
        Some("file") => format!("{} files", label(&conditions["action"])),
        Some(domain) => format!("{domain} requests"),
        None => "requests".into(),
    }
}

/// How the change affects command decisions, with one example each way.
fn permissions(comparison: &Value) -> String {
    let example = |witness: &Value| {
        format!(
            "{}: {} → {}",
            command_line(&witness["request"]),
            label(&witness["before"]["decision"]),
            label(&witness["after"]["decision"])
        )
    };
    let (broader, stricter) = (&comparison["expansion"], &comparison["restriction"]);
    let mut text = String::from("  Permissions: ");
    match comparison["relation"].as_str() {
        Some("equivalent") => text.push_str("no command decision changes"),
        Some("restrictive") => {
            let _ = write!(text, "stricter, for example {}", example(stricter));
        }
        Some("expansive") => {
            let _ = write!(text, "broader, for example {}", example(broader));
        }
        Some("mixed") => {
            let _ = write!(
                text,
                "broader for some commands, stricter for others\n    broader: {}\n    stricter: {}",
                example(broader),
                example(stricter)
            );
        }
        _ => {
            text.push_str("not fully compared; check the changes above");
            for (kind, witness) in [("broader", broader), ("stricter", stricter)] {
                if !witness.is_null() {
                    let _ = write!(text, "\n    {kind}: {}", example(witness));
                }
            }
        }
    }
    text.push('\n');
    text.push_str(&codex_prefixes(&comparison["codex_prefixes"]));
    text
}

/// Codex prefixes a change adds or removes. Each added prefix lets Codex run
/// matching commands outside its sandbox without asking.
pub(super) fn codex_prefixes(changes: &Value) -> String {
    let line = |prefix: &Value| {
        let pattern: Vec<String> = prefix["pattern"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|position| {
                let tokens: Vec<&str> = position
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                match tokens.as_slice() {
                    [one] => (*one).to_owned(),
                    many => format!("[{}]", many.join("|")),
                }
            })
            .collect();
        format!(
            "{} ({} {})",
            pattern.join(" "),
            label(&prefix["owner"]),
            label(&prefix["rule"])
        )
    };
    let mut text = String::new();
    for (key, heading) in [
        (
            "added",
            "Codex runs these outside its sandbox without asking",
        ),
        ("removed", "Codex stops running these outside its sandbox"),
    ] {
        let prefixes = changes[key].as_array().cloned().unwrap_or_default();
        if !prefixes.is_empty() {
            let _ = writeln!(text, "  {heading} (.codex/rules/shield.rules):");
            for prefix in &prefixes {
                let _ = writeln!(text, "    {}", line(prefix));
            }
        }
    }
    text
}

/// `shield policy show` or `review`: the active policy view, or the result of a menu action.
pub(super) fn policy(value: &Value) -> String {
    if value["inspected"] == true {
        return String::new();
    }
    if let Some(reviewed) = value["reviewed"].as_array() {
        let mut output = String::new();
        let mut waiting = 0;
        for row in reviewed {
            match row["choice"].as_str() {
                Some("accept") => output.push_str(&receipt(&row["result"])),
                Some("reject") => {
                    let _ = writeln!(output, "Rejected: {}", label(&row["summary"]));
                }
                _ => waiting += 1,
            }
        }
        if waiting > 0 {
            let _ = writeln!(
                output,
                "{} still waiting; review {} with shield policy review.",
                plural(waiting, "proposal", "proposals"),
                if waiting == 1 { "it" } else { "them" }
            );
        }
        return output;
    }
    if value["restored_project_files"] == true {
        let mut text = "Restored the .shield project files from the active policy.\n".to_owned();
        if value["repair_needed"] == true {
            text.push_str("Some files still conflict with your edits; check shield doctor.\n");
        }
        return text;
    }
    if value["enabled"].is_boolean() && !value["limit"].is_null() {
        return format!(
            "Request history is {}.{}\n",
            if value["enabled"] == true {
                format!("on, keeping up to {} entries", label(&value["limit"]))
            } else {
                "off".into()
            },
            if value["purged"] == true {
                " Existing history was cleared."
            } else {
                ""
            }
        );
    }
    if value.get("coverage").is_some() {
        return super::activity::status(value, false);
    }
    if value["status"] == "pending" && value["proposal_id"].is_string() {
        return format!("Proposal {} stays pending.\n", label(&value["proposal_id"]));
    }
    if value.get("effective").is_none() {
        return receipt(value);
    }
    let mut output = format!(
        "Active policy · revision {}\n",
        short(&value["policy_revision"])
    );
    let packs = value["effective"]["packs"].as_array();
    for pack in packs.into_iter().flatten() {
        let selection = &pack["selection"];
        let _ = writeln!(
            output,
            "  {} {} · {}",
            pack_name(&selection["id"]),
            label(&selection["version"]),
            label(&selection["profile"])
        );
    }
    let rules = value["project_rules"]["rules"]
        .as_array()
        .map_or(0, Vec::len);
    if packs.is_none_or(Vec::is_empty) && rules == 0 {
        let _ = writeln!(
            output,
            "  No packs or project rules. Add a pack with shield pack add."
        );
    } else if rules > 0 {
        let _ = writeln!(
            output,
            "  {}",
            plural(rules, "project rule", "project rules")
        );
    }
    match value["pending_configuration"]
        .as_array()
        .map_or(0, Vec::len)
    {
        0 => {}
        waiting => {
            let _ = writeln!(
                output,
                "{} waiting for review.",
                plural(waiting, "proposal is", "proposals are")
            );
        }
    }
    output
}

/// `shield rule list`: one line per rule in the active policy.
pub(super) fn rules(value: &Value) -> String {
    let rows = value["rules"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        return "No rules in the active policy. Add a pack with shield pack add or a rule with shield rule add.\n"
            .into();
    }
    let width = rows
        .iter()
        .map(|r| label(&r["id"]).len())
        .max()
        .unwrap_or(0);
    let mut output = format!(
        "{} in the active policy\n",
        if value["shared"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
        {
            plural(rows.len(), "rule body", "rule bodies")
        } else {
            plural(rows.len(), "rule", "rules")
        }
    );
    for row in &rows {
        let source = if row["source"] == "custom" {
            "project rule".to_owned()
        } else {
            label(&row["source"]["id"])
        };
        let decision = if row["effective_decision"].is_null() {
            decision(&row["decision"])
        } else {
            label(&row["effective_decision"])
        };
        let _ = writeln!(
            output,
            "  {:<width$}  {decision:<5}  {} · {source}",
            label(&row["id"]),
            rule_conditions(row)
        );
    }
    output
}

/// The receipt of a confirmed policy change.
pub(super) fn receipt(value: &Value) -> String {
    if value["committed"] == false {
        return "Nothing to change; the policy is already up to date.\n".into();
    }
    if value["committed"] != true {
        return super::fallback(value);
    }
    let changes = value["changes"].as_array().cloned().unwrap_or_default();
    let mut output = if changes.is_empty() {
        "Policy saved; no pack or rule changes.\n".to_owned()
    } else if changes.len() > MAX_RECEIPT_CHANGES {
        format!(
            "Policy updated with {} changes. Review them with shield pack list and shield rule list.\n",
            changes.len()
        )
    } else {
        changes.iter().fold(String::new(), |mut output, change| {
            let _ = writeln!(output, "{}", done(change));
            output
        })
    };
    if let Some(error) = value["setup_error"].as_str() {
        let _ = writeln!(output, "Agent prefix rules were not updated: {error}.");
    } else if value["repair_needed"] == true {
        output.push_str(
            "The .shield project files need repair: run shield policy review and choose Repair project files.\n",
        );
    }
    output
}

/// One applied change, in the past tense.
fn done(change: &Value) -> String {
    let id = label(&change["id"]);
    let dependency = if change["dependency"] == true {
        " · dependency"
    } else {
        ""
    };
    let text = match (change["kind"].as_str(), change["change"].as_str()) {
        (Some("pack"), Some("added")) => format!(
            "Added {id} {} · {} profile{dependency}",
            label(&change["version"]),
            label(&change["profile"])
        ),
        (Some("pack"), Some("removed")) => format!("Removed {id} {}", label(&change["version"])),
        (Some("pack"), Some("updated")) => format!(
            "Updated {id} to {} (was {})",
            label(&change["version"]),
            label(&change["from"])
        ),
        (Some("pack"), Some("profile")) => format!(
            "Changed {id} to the {} profile (was {})",
            label(&change["profile"]),
            label(&change["from"])
        ),
        (Some("pack"), Some("mode")) => format!(
            "Changed {id} to {} (was {})",
            label(&change["mode"]),
            label(&change["from"])
        ),
        (Some("pack"), Some("role")) => format!(
            "Changed {id} to a {} selection (was {})",
            label(&change["role"]),
            label(&change["from"])
        ),
        (Some("rule"), Some(kind)) => {
            let verb = match kind {
                "added" => "Added",
                "removed" => "Removed",
                _ => "Edited",
            };
            format!("{verb} rule {}", label(&change["rule"]["id"]))
        }
        (Some("variable"), Some("removed")) => {
            format!("Unset {} {}", label(&change["id"]), label(&change["name"]))
        }
        (Some("variable"), _) => format!(
            "Set {} {} to {}",
            label(&change["id"]),
            label(&change["name"]),
            change["after"]
                .as_str()
                .map_or_else(|| change["after"].to_string(), str::to_owned)
        ),
        (Some("override"), Some("removed")) => format!(
            "Removed the {} override of {}",
            label(&change["id"]),
            label(&change["name"])
        ),
        (Some("override"), _) => format!(
            "Overrode {} {} with {}",
            label(&change["id"]),
            label(&change["name"]),
            change["after"]
                .as_str()
                .map_or_else(|| change["after"].to_string(), str::to_owned)
        ),
        (Some("pack"), Some("disabled")) => format!("Turned off {id} locally"),
        (Some("mcp"), _) => format!("Changed MCP connection {id}"),
        _ => "Changed access paths".into(),
    };
    if change["layer"] == "local" {
        format!("{text} (local only)")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pack(change: &str) -> Value {
        json!({"kind":"pack","change":change,"id":"acme/git","version":"1.1.0","from":"1.0.0","profile":"strict","dependency":false,"digest":"0123456789abcdef".repeat(4)})
    }

    #[test]
    fn change_reviews_list_each_change_and_one_permission_example() {
        let witness = json!({"request":{"executable":"acme","arguments":["write"]},"before":{"decision":"ask"},"after":{"decision":"deny"}});
        let text = review(&json!({"operation":"change policy",
            "changes":[pack("added"),pack("profile"),{"kind":"rule","change":"added","rule":{"id":"status","decision":"allow","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["status"]}}}}],
            "candidate":{"packs":[json!({"id":"acme/git","rules":[{},{}]}).to_string()]},
            "comparison":{"relation":"restrictive","restriction":witness}}))
        .expect("review");
        assert_eq!(
            text,
            "Change policy\n  + acme/git 1.1.0 · strict profile · 2 rules · digest 0123456789ab\n  ~ acme/git profile 1.0.0 → strict\n  + rule status: allow git status\n  Permissions: stricter, for example acme write: ask → deny\n"
        );
    }

    #[test]
    fn local_mcp_values_are_shown_only_in_the_ephemeral_terminal_review() {
        let candidate = |profile: &str| {
            json!({"config":"schema_version: 1\npacks: []\n",
            "local_config":json!({"schema_version":1,"mcp":{"cloud":{"transport":{"type":"stdio","command":"/acme/private/connector","args":["--profile",profile]},"agents":{"claude":"personal_cloud"}}}}).to_string()})
        };
        let before = candidate("acme-personal-before");
        let after = candidate("acme-personal-after");
        let changes =
            crate::review::changes::diff::between(&json!({}), &before, &json!({}), &after);
        let mut view = json!({"operation":"change policy","changes":changes,"candidate":after,"comparison":{"relation":"equivalent","complete":true}});
        crate::review::operator::state::terminal_mcp_details(&mut view["changes"], &before, &after)
            .unwrap();
        let text = review(&view).unwrap();
        for private in [
            "/acme/private/connector",
            "acme-personal-before",
            "acme-personal-after",
        ] {
            assert!(text.contains(private), "{text}");
            assert!(!changes.to_string().contains(private));
        }
        assert!(text.contains("local only"));
        assert_eq!(
            changes,
            json!([{"kind":"mcp","id":"cloud","layer":"local","change":"changed"}])
        );
        let public = review(
            &json!({"operation":"change policy","changes":changes,"candidate":{},"comparison":{}}),
        )
        .unwrap();
        assert!(public.contains("MCP connection cloud changed"), "{public}");
        assert!(
            !receipt(&json!({"committed":true,"changes":view["changes"]}))
                .contains("acme-personal")
        );
    }

    #[test]
    fn change_reviews_name_files_that_lose_formatting() {
        let line = "rules.yml: 1 comment in the changed entries will not be kept.";
        let text = review(&json!({"operation":"change policy",
            "changes":[{"kind":"rule","change":"edited","rule":{"id":"status","decision":"allow","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["status"]}}}}],
            "candidate":{},
            "comparison":{"relation":"equivalent","complete":true,"formatting":[line]}}))
        .expect("review");
        assert!(text.ends_with(&format!("{line}\n")), "{text}");
        let plain = review(
            &json!({"operation":"change policy","changes":[],"candidate":{},
            "comparison":{"relation":"equivalent","complete":true}}),
        )
        .expect("review");
        assert!(!plain.contains("will not be kept"), "{plain}");
    }

    #[test]
    fn proposal_cards_say_what_changes_why_and_what_the_agent_was_doing() {
        let witness = json!({"request":{"executable":"git","arguments":["init"]},"before":{"decision":"ask"},"after":{"decision":"allow"}});
        let change = json!({"operation":"change policy","changes":[pack("added")],
            "candidate":{"packs":[json!({"id":"acme/git","rules":[{"id":"init","decision":"allow","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["init"]}}}]}).to_string()]},
            "comparison":{"relation":"expansive","expansion":witness}});
        let proposal =
            json!({"operation":"pack_add","reasons":{"agent_reason":"Initialize the repository."}});
        let requested = json!({"operation":{"kind":"shell","source":"git init && git add ."},"agent":"codex","recorded_at":"2026-01-02T03:04:05Z"});
        let card = card(&proposal, &change, Some(&requested), false);
        assert_eq!(card.title, "pack add");
        assert!(
            card.summary.starts_with("Install acme/git 1.1.0") && !card.summary.contains("digest"),
            "{}",
            card.summary
        );
        assert_eq!(card.reason, "Initialize the repository.");
        assert_eq!(
            card.resources[0],
            (
                "asked after `git init && git add .`".into(),
                "codex · 2026-01-02 03:04:05".into()
            )
        );
        assert_eq!(
            card.resources.last().unwrap().1,
            "broader, for example git init: ask → allow"
        );
        assert!(
            card.details
                .contains("Rules in acme/git\n  init: allow git init"),
            "{}",
            card.details
        );
        assert!(card.stale.is_none());
        let stale = super::card(&json!({"operation":"rule_add"}), &change, None, true);
        assert_eq!(stale.reason, "The agent gave no reason.");
        assert!(stale.stale.is_some());
    }

    #[test]
    fn broader_permissions_are_never_summarized_away() {
        let witness = json!({"request":{"executable":"acme","arguments":["write"]},"before":{"decision":"deny"},"after":{"decision":"allow"}});
        let text = permissions(&json!({"relation":"unknown","expansion":witness}));
        assert!(text.contains("not fully compared"));
        assert!(text.contains("broader: acme write: deny → allow"));
    }

    #[test]
    fn receipts_list_a_few_changes_or_count_many() {
        let few = receipt(&json!({"committed":true,"changes":[pack("added")]}));
        assert_eq!(few, "Added acme/git 1.1.0 · strict profile\n");
        let many = receipt(
            &json!({"committed":true,"changes":[pack("added"),pack("added"),pack("added"),pack("added")]}),
        );
        assert!(many.starts_with("Policy updated with 4 changes."));
        assert_eq!(
            receipt(&json!({"committed":false,"unchanged":true})),
            "Nothing to change; the policy is already up to date.\n"
        );
    }

    #[test]
    fn rules_show_one_line_each_with_their_source() {
        let text = rules(&json!({"rules":[
            {"id":"status","decision":"allow","source":"custom","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"exact","values":["status"]}}},
            {"id":"mutation","decision":{"loose":"allow","strict":"deny"},"effective_decision":"deny","source":{"id":"acme/test"},"conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"prefix","values":["write"]}}}]}));
        assert_eq!(
            text,
            "2 rules in the active policy\n  status    allow  git status · project rule\n  mutation  deny   acme write … · acme/test\n"
        );
    }
}
