//! Shared human reviews of retained administrative plans.
use super::{label, registries, setup};
use crate::review::{Error, hook_approval::MAX_FORM};
use serde_json::Value;
use std::fmt::Write as _;

pub(crate) const REVIEW_TOO_LARGE: &str = "confirmation_review_too_large: The material changes exceed this host's review limit. Review them in the operator terminal.";
pub(crate) const REVIEW_REQUIRES_DETAILS: &str = "confirmation_review_requires_details: Exact settings or affected commands need inspection. Review them in the operator terminal.";

pub(crate) fn form_refusal(error: &Error) -> Option<(&'static str, &'static str)> {
    match error {
        Error::Invalid(message @ (REVIEW_TOO_LARGE | REVIEW_REQUIRES_DETAILS)) => {
            message.split_once(": ")
        }
        _ => None,
    }
}

pub(crate) struct Review {
    heading: String,
    summary: String,
    requires_details: bool,
}

impl Review {
    pub(crate) fn form(&self) -> Result<String, Error> {
        if self.requires_details {
            return Err(Error::Invalid(REVIEW_REQUIRES_DETAILS));
        }
        let message = format!(
            "{}\n{}\nAccept to apply this captured plan. For exact details, use the operator terminal review.",
            self.heading, self.summary
        );
        if message.len() > MAX_FORM {
            return Err(Error::Invalid(REVIEW_TOO_LARGE));
        }
        Ok(message)
    }

    pub(in crate::review::operator) fn terminal(&self) -> (String, bool) {
        if self.heading.len() + self.summary.len() > MAX_FORM {
            return (
                format!(
                    "{}\nOpen Inspect details to review all material changes.\n",
                    self.heading
                ),
                true,
            );
        }
        (
            format!("{}\n{}", self.heading, self.summary),
            self.requires_details,
        )
    }
}

pub(crate) fn review(value: &Value, project: &str) -> Review {
    let scope = if value["scope"] == "user" {
        "user, across projects"
    } else {
        "project"
    };
    let root = value["project"].as_str().unwrap_or(project);
    let heading = format!(
        "Shield review for {} ({scope} scope)\n",
        label(&Value::String(root.into()))
    );
    let operation = value["operation"].as_str().unwrap_or_default();
    let (summary, requires_details) = match operation {
        "initialize project" | "reconcile project" => initialization(value),
        "set up agent integrations"
        | "remove Shield integrations"
        | "keep integrations"
        | "cancel interrupted agent setup"
        | "review conflicting agent setup" => integration(value),
        "history settings" => (history(value), false),
        "reject policy proposal" => (
            format!(
                "Reject pending proposal {}. The proposal will close without applying.\nReason: {}\n",
                label(&value["proposal"]["proposal_id"]),
                value["reason"]
                    .as_str()
                    .map_or_else(|| "none supplied".into(), |_| label(&value["reason"]))
            ),
            false,
        ),
        "change protected file access" => (protection(value), false),
        _ => setup::review(value)
            .or_else(|| registries::review(value))
            .map_or_else(
                || (format!("Review {}.\n", label(&value["operation"])), true),
                |text| (text, false),
            ),
    };
    Review {
        heading,
        summary,
        requires_details,
    }
}

fn rows(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

fn names(value: &Value) -> String {
    rows(value).map(label).collect::<Vec<_>>().join(", ")
}

fn initialization(value: &Value) -> (String, bool) {
    let mut out = String::from("Initialize Shield with project enforcement enabled.\n");
    for pack in rows(&value["packs"]) {
        let _ = writeln!(
            out,
            "Install {} {} with the {} profile.",
            label(&pack["id"]),
            label(&pack["version"]),
            label(&pack["profile"])
        );
    }
    if value["policy"].is_object() {
        setup::policy(&mut out, value);
    }
    out.push_str("Setup does not verify host loading or interception.\n");
    let (integrations, requires_details) = integration(&value["integration_setup"]);
    out.push_str(&integrations);
    (out, requires_details)
}

fn history(value: &Value) -> String {
    let mode = |key: &str| {
        if value[key]["enabled"] == true {
            "on"
        } else {
            "off"
        }
    };
    let mut text = format!(
        "Request history {} → {}. Retention limit {} → {}.\n",
        mode("before"),
        mode("after"),
        label(&value["before"]["limit"]),
        label(&value["after"]["limit"])
    );
    if value["purge"] == true {
        text.push_str("Clear all retained request history.\n");
    } else if value["after"]["limit"].as_u64() < value["before"]["limit"].as_u64() {
        text.push_str("Older entries above the new limit will be removed.\n");
    }
    text
}

fn protection(value: &Value) -> String {
    let mut out = setup::review(value).unwrap_or_default();
    if value["after"]["agent_settings_read"] == "allow"
        && value["before"]["agent_settings_read"] != "allow"
    {
        out.push_str("Agents can read protected agent settings without asking. These files may contain credentials.\n");
    }
    out
}

fn integration(value: &Value) -> (String, bool) {
    let mut out = String::new();
    let _ = writeln!(out, "{}.", label(&value["operation"]));
    if value["repair"] == true {
        out.push_str(
            "Replace conflicting Shield hooks and its MCP registration for the selected agents.\n",
        );
    }
    for field in [
        &value["effect"],
        &value["maintenance_scope"]["reason"],
        &value["private_definition_access"],
    ] {
        if field.is_string() {
            let _ = writeln!(out, "{}", label(field));
        }
    }
    for agent in rows(&value["agents"]) {
        let id = label(&agent["agent"]);
        let hooks = names(&agent["hooks"]);
        let _ = write!(
            out,
            "{} — {}",
            crate::agents::label(&id),
            label(&agent["change"])
        );
        if !hooks.is_empty() {
            let _ = write!(out, " · hooks {hooks}");
        }
        let removed = names(&agent["removed_hooks"]);
        if !removed.is_empty() {
            let _ = write!(out, " · remove hooks {removed}");
        }
        if agent["mcp_ownership"] == "external" {
            out.push_str(" · uses an externally managed MCP entry");
        }
        out.push('\n');
    }
    if rows(&value["agents"]).any(|agent| agent["change"] == "added") {
        out.push_str("Registered agents gain the adapter's scoped access to skill, temporary, and supported session files.\n");
    }
    if value["binary"].is_string() {
        let _ = writeln!(out, "Shield executable {}.", label(&value["binary"]));
    }
    if value["binary_changed"] == true {
        let _ = writeln!(
            out,
            "Replace the executable pinned at {}. Digest {} → {}.",
            label(&value["previous_binary"]),
            super::short(&value["previous_binary_digest"]),
            super::short(&value["binary_digest"]),
        );
    }
    if value["adopt_existing_entries"] == true {
        out.push_str("Adopt ownership of the existing matching Shield entries.\n");
    }
    if value["host"]["publish"].is_string() {
        let _ = writeln!(
            out,
            "{} publishes the native entries through {}.",
            label(&value["host"]["label"]),
            label(&value["host"]["publish"])
        );
    }
    access(&mut out, value);
    dropped_ownership(&mut out, value);
    for (status, count) in file_counts(value) {
        if count != 0 {
            let _ = writeln!(
                out,
                "{status} {count} configuration {}.",
                if count == 1 { "file" } else { "files" }
            );
        }
    }
    let requires_details = connection_details(&mut out, value);
    if requires_details {
        out.push_str("Inspect the exact settings and owned entries before approving.\n");
    }
    (out, requires_details)
}

fn dropped_ownership(out: &mut String, value: &Value) {
    for (field, text) in [
        ("agents", "native registrations"),
        ("private_launch_agents", "private launcher authority"),
    ] {
        if value["dropped_ownership"][field]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
        {
            let _ = writeln!(
                out,
                "Drop {text} for {}.",
                names(&value["dropped_ownership"][field])
            );
        }
    }
    if value["dropped_ownership"]["host"].is_string() {
        let _ = writeln!(
            out,
            "Drop ownership of the {} definition.",
            label(&value["dropped_ownership"]["host"])
        );
    }
    if value["dropped_ownership"]["legacy_interceptor"] == true {
        out.push_str("Drop ownership of the legacy interceptor.\n");
    }
}

fn connection_details(out: &mut String, value: &Value) -> bool {
    let mut requires_details = value["unclassified_recovery"] == true;
    for connection in rows(&value["connection_changes"]) {
        let action = label(&connection["change"]);
        let _ = writeln!(
            out,
            "{action} MCP connection {} for {}.",
            label(&connection["id"]),
            label(&connection["agent"])
        );
        if action == "Remove" {
            continue;
        }
        for (key, heading) in [("before", "Previous"), ("after", "New")] {
            let side = &connection[key];
            if side.is_null() {
                continue;
            }
            if let Some(description) = public_endpoint(side) {
                let _ = writeln!(out, "  {heading}: {description}");
            } else {
                requires_details = true;
            }
        }
    }
    requires_details
}

fn public_endpoint(connection: &Value) -> Option<String> {
    let endpoint = &connection["settings"]["url"];
    let parsed = url::Url::parse(endpoint.as_str()?).ok()?;
    if connection["source"] != "project"
        || connection["settings"]["type"] != "http"
        || parsed.query().is_some()
    {
        return None;
    }
    Some(format!(
        "{} at {} ({}, {}). Pack associations {}.",
        label(&connection["alias"]),
        label(endpoint),
        if connection["enabled"] == true {
            "enabled"
        } else {
            "disabled"
        },
        label(&connection["projection"]),
        label(&connection["packs"])
    ))
}

fn access(out: &mut String, value: &Value) {
    if value["initial_tool_approvals"] == true {
        let approvals = &value["tool_approvals"];
        let count = rows(&approvals["approved"]).count();
        let ask = names(&approvals["ask"]);
        if count != 0 {
            let _ = writeln!(
                out,
                "{count} Shield tools can start without a host tool prompt. Shield's separate confirmation checks still apply."
            );
        }
        if !ask.is_empty() {
            let _ = writeln!(out, "Keep the host prompt for {ask}.");
        }
    } else {
        for (key, message, many) in [
            (
                "added",
                "Run without the host's tool prompt",
                "Skip the host's prompt for",
            ),
            (
                "removed",
                "Stop skipping the host's tool prompt",
                "Stop skipping the host's prompt for",
            ),
        ] {
            let tools = &value["approval_changes"][key];
            let count = rows(tools).count();
            if count > 3 {
                let _ = writeln!(out, "{many} {count} Shield tools.");
            } else if count != 0 {
                let tools = names(tools);
                let _ = writeln!(out, "{message}: {tools}.");
            }
        }
    }
    for (agent, fallback) in value["ask_fallbacks"].as_object().into_iter().flatten() {
        if fallback["before"] != fallback["after"] && !fallback["after"].is_null() {
            let _ = writeln!(
                out,
                "{} when the host cannot ask: {}.",
                crate::agents::label(agent),
                if fallback["after"] == "allow" {
                    "allow Ask requests"
                } else {
                    "block Ask requests"
                }
            );
        }
    }
    if value["prefix_files_changed"] == true {
        let prefixes = &value["codex_prefixes"];
        out.push_str("Change Shield's Codex command rules. Matching allowed commands can run outside Codex's sandbox.\n");
        out.push_str(&super::policy::codex_prefixes(
            &serde_json::json!({"added":prefixes}),
        ));
    }
}

fn file_counts(value: &Value) -> [(&str, usize); 3] {
    let mut counts = [("Create", 0), ("Update", 0), ("Remove", 0)];
    for file in rows(&value["files"]).filter(|file| file["before"] != file["after"]) {
        let index = if file["before"].is_null() {
            0
        } else if file["after"].is_null() {
            2
        } else {
            1
        };
        counts[index].1 += 1;
    }
    counts
}

/// This output is written only to the controlling terminal on explicit request.
pub(in crate::review::operator) fn exact_details(value: &Value) -> Result<String, Error> {
    let detail = serde_json::to_string_pretty(value)
        .map_err(|_| Error::Invalid("review detail encoding"))?;
    Ok(detail
        .lines()
        .map(|line| label(&Value::String(line.into())))
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn administrative_review_groups_native_files_and_keeps_exact_contents_for_the_operator() {
        let private = "test-private-setting".repeat(3000);
        let value = json!({"operation":"set up agent integrations","project":"/acme",
            "agents":[{"agent":"codex","change":"added","hooks":["PreToolUse"]}],
            "files":[{"path":".codex/config.toml","before":private,"after":"new"}]
        });
        let review = review(&value, "/wrong");
        let form = review.form().unwrap();
        assert!(form.contains("/acme") && form.contains("Update 1 configuration file"));
        assert!(form.contains("PreToolUse") && !form.contains("test-private-setting"));
        assert!(exact_details(&value).unwrap().contains(&private));
    }

    #[test]
    fn administrative_review_refuses_to_shorten_material_changes_into_an_approval() {
        let value = json!({"operation":"set up agent integrations","connection_changes":[{
            "agent":"codex","id":"acme","change":"Add","after":{"env":{"TOKEN":"private-fixture"}}
        }]});
        let review = review(&value, "/acme");
        assert!(matches!(
            review.form(),
            Err(Error::Invalid(REVIEW_REQUIRES_DETAILS))
        ));
        let (summary, details) = review.terminal();
        assert!(details && !summary.contains("private-fixture"));
        assert!(exact_details(&value).unwrap().contains("private-fixture"));

        let large = json!({"operation":"registry versioned trust","root":{"version":2,"root":{"key_ids":["a".repeat(MAX_FORM)]}}});
        assert!(matches!(
            super::review(&large, "/acme").form(),
            Err(Error::Invalid(REVIEW_TOO_LARGE))
        ));
    }
}
