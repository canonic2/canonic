//! Human reviews of project initialization and agent integration plans: what
//! each agent gains or loses and each owned file's status, plus the receipts.
//! Shield only changes its own entries in these files, which the agent lines
//! name, so file contents are not repeated.
use super::label;
use serde_json::Value;
use std::fmt::Write;

/// The review for an initialization or integration operation, if `value` is one.
pub(super) fn review(value: &Value) -> Option<String> {
    let mut output = String::new();
    match value["operation"].as_str()? {
        "initialize project" => {
            let _ = writeln!(
                output,
                "Initialize Shield in {}\n",
                label(&value["project"])
            );
            policy(&mut output, value);
            integrations(&mut output, &value["integration_setup"]);
        }
        "reconcile project" => {
            let _ = writeln!(
                output,
                "Reconcile Shield in {}\nActive policy {} is kept.\n",
                label(&value["project"]),
                super::short(&value["policy_revision"])
            );
            integrations(&mut output, &value["integration_setup"]);
        }
        "set up agent integrations" | "remove Shield integrations" | "keep integrations" => {
            integrations(&mut output, value);
        }
        "change protected file access" => {
            let _ = writeln!(output, "Change protected file access");
            access_lines(&mut output, &value["before"], &value["after"]);
            if value["after"]["agent_settings_change"] == "allow"
                && value["before"]["agent_settings_change"] != "allow"
            {
                let _ = writeln!(
                    output,
                    "  Agents will be able to remove Shield's hooks and turn protection off, or remove the host confirmation that MCP policy changes rely on."
                );
            }
            output.push('\n');
        }
        _ => return None,
    }
    Some(output)
}

/// Each protected file group's access, marking the ones that change.
fn access_lines(output: &mut String, before: &Value, after: &Value) {
    for (key, name) in [
        ("agent_settings_read", "Reading agent settings"),
        ("agent_settings_change", "Changing agent settings"),
        ("shield_config_change", "Changing Shield policy files"),
    ] {
        let now = access_word(&after[key]);
        let was = access_word(&before[key]);
        if now == was {
            let _ = writeln!(output, "  {name:<30}{now}");
        } else {
            let _ = writeln!(output, "  {name:<30}{was} → {now}");
        }
    }
}

fn access_word(value: &Value) -> &'static str {
    match value.as_str() {
        Some("allow") => "allowed",
        Some("ask") => "asks first",
        _ => "blocked",
    }
}

/// The receipt of `shield protection set`.
pub(super) fn protection(value: &Value) -> String {
    let mut output = String::from(if value["changed"] == true {
        "Protected file access updated.\n"
    } else {
        "Protected file access unchanged.\n"
    });
    let access = &value["path_access"];
    access_lines(&mut output, access, access);
    output
}

fn policy(output: &mut String, value: &Value) {
    let _ = writeln!(output, "Policy");
    let config = crate::configuration::layers::merged(&value["policy"]).unwrap_or_default();
    let selections = &config["packs"];
    let packs = value["policy"]["packs"].as_array();
    if packs.is_none_or(Vec::is_empty) {
        let _ = writeln!(output, "  No permission packs");
    }
    for (index, bytes) in packs.into_iter().flatten().enumerate() {
        let Some(text) = bytes.as_str() else { continue };
        let pack = crate::policy::parse::<Value>(text).unwrap_or_default();
        let _ = writeln!(
            output,
            "  {} {} · {} profile · {} · digest {}",
            label(&pack["id"]),
            label(&pack["version"]),
            label(&selections[index]["profile"]),
            super::packs::rule_count(&pack),
            &crate::policy::fingerprint(text.as_bytes())[..12]
        );
        if pack["description"].is_string() {
            let _ = writeln!(output, "    {}", label(&pack["description"]));
        }
    }
    if packs.is_some_and(|packs| !packs.is_empty())
        && let Some(verification) = value["registry_verification"].as_str()
    {
        let _ = writeln!(output, "  Registry: {verification}");
    }
    if !config["access_paths"].is_null() {
        let _ = writeln!(
            output,
            "  Access paths: {}",
            super::policy::access_paths(&config["access_paths"])
        );
    }
    let rules = value["policy"]["rules"]
        .as_str()
        .and_then(|s| crate::policy::parse::<Value>(s).ok())
        .and_then(|document| document["rules"].as_array().cloned())
        .unwrap_or_default();
    for rule in &rules {
        let _ = writeln!(output, "  Rule {}", super::policy::rule(rule));
    }
    output.push('\n');
}

/// The receipt of `shield init`.
pub(super) fn initialized(value: &Value) -> String {
    let packs = value["installed_packs"]["packs"]
        .as_array()
        .map_or(0, Vec::len);
    let mut output = format!(
        "Shield is set up in {} with {}.\n",
        label(&value["project_root"]),
        super::plural(packs, "pack", "packs")
    );
    output.push_str(&connected(value));
    output
}

/// The receipt of `shield mcp setup`.
pub(super) fn configured(value: &Value) -> String {
    connected(value)
}

/// Which agents are connected, and what the operator still has to do.
fn connected(value: &Value) -> String {
    let coverage = &value["coverage"];
    let agents: Vec<_> = coverage["installed_integrations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| crate::agents::label(row["agent"].as_str().unwrap_or("an agent")))
        .collect();
    let mut output = if agents.is_empty() {
        "No agent is connected. Connect one with shield mcp setup.\n".to_owned()
    } else {
        format!(
            "Connected {}. Restart it, trust this project and approve Shield's hooks and MCP server.\n",
            agents.join(" and ")
        )
    };
    if value["repair_needed"] == true || value.get("setup_error").is_some() {
        output.push_str("Something still needs repair: run shield doctor.\n");
    }
    output
}

fn integrations(output: &mut String, value: &Value) {
    let _ = writeln!(output, "Agent integrations");
    match value["operation"].as_str() {
        Some("keep integrations") | None => {
            let _ = writeln!(
                output,
                "  None selected. Set them up later with shield mcp setup.\n"
            );
            return;
        }
        Some("remove Shield integrations") => {
            let _ = writeln!(output, "  Remove Shield's hooks and MCP servers.");
        }
        _ => {}
    }
    for agent in value["agents"].as_array().into_iter().flatten() {
        let hooks: Vec<_> = agent["hooks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(label)
            .collect();
        let _ = write!(
            output,
            "  {:<12}{} · hooks {} · MCP server {}",
            crate::agents::label(agent["agent"].as_str().unwrap_or("Agent")),
            label(&agent["change"]),
            hooks.join(", "),
            label(&agent["mcp_server"])
        );
        if let Some(setting) = agent["agent"]
            .as_str()
            .and_then(|id| value["ask_fallbacks"].get(id))
        {
            output.push_str(&fallback(setting));
        }
        output.push('\n');
    }
    tool_permissions(output, value);
    output.push_str(&super::policy::codex_prefixes(
        &serde_json::json!({"added":value["codex_prefixes"]}),
    ));
    for connection in value["connections"].as_array().into_iter().flatten() {
        let _ = writeln!(
            output,
            "  MCP {} → {} / {} · {} · {}\n    Settings: {}\n    Pack associations: {}",
            label(&connection["id"]),
            label(&connection["agent"]),
            label(&connection["alias"]),
            label(&connection["source"]),
            label(&connection["projection"]),
            label(&connection["settings"]),
            label(&connection["packs"])
        );
    }
    let _ = writeln!(output, "\n  Files");
    let width = value["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["path"].as_str())
        .map(str::len)
        .max()
        .unwrap_or(0);
    for file in value["files"].as_array().into_iter().flatten() {
        file_change(output, file, width);
    }
    if value["adopt_existing_entries"] == true {
        let _ = writeln!(
            output,
            "  Shield entries already in these files are adopted as they are."
        );
    }
    let _ = writeln!(output, "  Other settings in these files are preserved.\n");
    if value["operation"] == "set up agent integrations" {
        let _ = writeln!(
            output,
            "After confirming, restart each agent, trust this project, review its hooks and approve the MCP server.\n"
        );
    }
}

/// Scope the selected Shield tool approvals independently of third-party servers.
fn tool_permissions(output: &mut String, value: &Value) {
    if !value["binary"].is_null() && value["operation"] == "set up agent integrations" {
        let _ = writeln!(
            output,
            "  Hooks and MCP run {} for this project.",
            label(&value["binary"])
        );
        let names = |key: &str| -> Vec<&str> {
            value["tool_approvals"][key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect()
        };
        let (approved, ask) = (names("approved"), names("ask"));
        let _ = writeln!(
            output,
            "  {} of {} Shield MCP tools run without asking; ask each time: {}.",
            approved.len(),
            approved.len() + ask.len(),
            if ask.is_empty() {
                "none".to_owned()
            } else {
                ask.join(", ")
            }
        );
        let _ = writeln!(
            output,
            "  Policy activation still requires your confirmation."
        );
        let _ = writeln!(
            output,
            "  Agents can read and write system temporary folders. Session files use each agent's supported mappings."
        );
    }
}

fn fallback(value: &Value) -> String {
    let describe = |setting: &Value| match setting.as_str() {
        Some("allow") => "Ask fallback allows requests",
        _ => "Ask requests are blocked",
    };
    let after = &value["after"];
    let mut text = format!(" · {}", describe(after));
    if !value["before"].is_null() && value["before"] != *after {
        let _ = write!(
            text,
            " (was: {})",
            describe(&value["before"]).to_lowercase()
        );
    }
    text
}

fn file_change(output: &mut String, file: &Value, width: usize) {
    let (before, after) = (file["before"].as_str(), file["after"].as_str());
    let status = match (before, after) {
        (None, None) => return,
        (Some(b), Some(a)) if b == a => "unchanged",
        (None, Some(_)) => "created",
        (Some(_), None) => "removed",
        (Some(_), Some(_)) => "updated",
    };
    let path = file["path"].as_str().unwrap_or_default();
    let holds = crate::agents::ADAPTERS
        .iter()
        .find_map(|a| {
            let spec = a.spec();
            if path == spec.hooks_path {
                Some(format!("{} hooks", spec.label))
            } else if spec.mcp_path.unwrap_or(".mcp.json") == path {
                Some(format!("{} MCP server", spec.label))
            } else if spec.prefix_rules_path == Some(path) {
                Some(format!("{} prefix rules", spec.label))
            } else {
                None
            }
        })
        .unwrap_or_default();
    let _ = writeln!(
        output,
        "    {:<width$}  {status:<9}  {holds}",
        label(&file["path"])
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integration_reviews_summarize_agents_and_list_each_file_once() {
        let plan = json!({"operation":"set up agent integrations","binary":"/usr/local/bin/shield",
        "agents":[{"agent":"codex","change":"added","hooks":["PostToolUse","PreToolUse"],"mcp_server":"shield"}],
        "ask_fallbacks":{"codex":{"before":"block","after":"allow"}},
        "adopt_existing_entries":false,
        "files":[
            {"path":".mcp.json","before":"{}\n","after":"{}\n"},
            {"path":".codex/config.toml","before":"model = \"acme\"\n","after":"model = \"acme\"\n\n[mcp_servers.shield]\ncommand = \"shield\"\n"},
            {"path":".codex/hooks.json","before":null,"after":"{\n  \"hooks\": {}\n}\n"},
        ]});
        let text = review(&plan).expect("integration review");
        assert!(text.contains("Codex       added · hooks PostToolUse, PreToolUse · MCP server shield · Ask fallback allows requests (was: ask requests are blocked)"), "{text}");
        assert!(
            text.contains(
                "    .mcp.json           unchanged  Claude Code MCP server\n    .codex/config.toml  updated    Codex MCP server\n    .codex/hooks.json   created    Codex hooks\n  Other settings"
            ),
            "{text}"
        );
        // File contents are never repeated; the agent line names what changes.
        assert!(
            !text.contains("model = ") && !text.contains("mcp_servers"),
            "{text}"
        );
        assert!(text.contains("restart each agent"), "{text}");
    }

    #[test]
    fn skipped_integrations_point_to_later_setup() {
        let text = review(&json!({"operation":"keep integrations"})).expect("review");
        assert!(text.contains("None selected. Set them up later with shield mcp setup."));
    }
}
