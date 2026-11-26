//! Short agent guidance plus bounded text-only inspection/selector compatibility.
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;
use std::fmt::Write as _;

/// Initialization guidance uses the enrolled session agent, not client display metadata.
pub(super) fn instructions(agent: Option<&str>) -> String {
    instructions_with(crate::agents::review::resolve(agent))
}

pub(super) fn instructions_for(context: &crate::review::Context) -> String {
    if context.confirmed_mcp_forms && context.surface == "mcp" {
        instructions_with(crate::agents::review::for_context(context))
    } else {
        instructions(context.agent.as_deref())
    }
}

fn instructions_with(adapter: &dyn crate::agents::review::ReviewAdapter) -> String {
    format!(
        "Shield manages project permissions. Use setup to connect agents, status for a summary, and doctor for repair. For a block or Ask, follow the returned action, such as request_approve for one call, session_grant_show for session grants, or suggestion_list for recurring requests. Only the supported approval flow grants permission; previews and chat cannot approve. Do not bypass refusals or retry a decline. {}",
        adapter.guidance().instructions
    )
}

pub(super) fn approval_unavailable(agent: Option<&str>) -> &'static str {
    if matches!(agent, Some("gemini" | "agy")) {
        return "This host did not provide Shield's confirmation form. Report the missing in-session capability; do not ask the user to run terminal commands for approval.";
    }
    crate::agents::review::resolve(agent).guidance().unavailable
}

/// The structured message of a declined host confirmation.
pub(super) const APPROVAL_DECLINED: &str =
    "The change was not approved. Continue without it; do not retry unless the user requests it.";

/// The user declined an in-session approval.
pub(super) const REQUEST_DECLINED: &str = "The user declined this request. Do not retry it; continue without it or ask the user what they want instead.";

/// The host could not show Shield's form; the remedy stays in the session.
pub(super) fn request_unavailable(agent: Option<&str>) -> &'static str {
    match agent {
        Some("gemini" | "agy") => {
            "This host did not provide a confirmation form. Report the missing in-session capability; do not ask the user to run terminal commands for approval."
        }
        Some("codex") => {
            "Shield could not ask the user: this session's approval settings reject confirmation forms. Ask the user to choose an approval policy that allows prompts with /permissions, then call request_approve again with the same arguments. Do not change approval settings yourself."
        }
        _ => {
            "Shield could not ask the user: this session cannot show confirmation forms. Ask the user to allow Shield's confirmation prompts, then call request_approve again with the same arguments."
        }
    }
}

/// Text block 0. An error's structured message carries its recovery, so the
/// summary repeats it rather than keeping a second copy of the guidance.
pub(super) fn summary(name: &str, agent: Option<&str>, value: &Value) -> String {
    let mut primary = primary_summary(name, agent, value);
    if value["truncated"] == true
        || value["missing"]
            .as_array()
            .is_some_and(|fields| !fields.is_empty())
    {
        primary.push_str(" Some details are omitted. Check the missing and truncated fields.");
    }
    let notice = &value["data"]["pack_update_notice"];
    if value["error"].is_null()
        && let Some(message) = notice["message"].as_str()
    {
        format!(
            "{primary} {message} {}",
            notice["instruction"].as_str().unwrap_or_default()
        )
    } else {
        primary
    }
}

fn primary_summary(name: &str, agent: Option<&str>, value: &Value) -> String {
    let error = &value["error"];
    if !error.is_null() {
        return error_summary(error);
    }
    let data = &value["data"];
    if data["unchanged"] == true && name != "setup" {
        return "Already up to date; no proposal or approval is needed.".into();
    }
    match name {
        "setup" => {
            if data["setup_error"].is_string() {
                return with_guidance("Shield setup needs attention.", data);
            }
            if data["next"]["server"] == "hub" {
                return with_guidance("Hub publication pending.", data);
            }
            let outcome = match data["setup_status"].as_str() {
                Some("initialized") => "Shield set up for this project.",
                Some("unchanged") => "Shield setup is current.",
                Some("configured") => "Shield integrations configured.",
                Some("resumed") => "Previously approved Shield setup finished.",
                _ => "Shield setup needs attention.",
            };
            with_guidance(outcome, data)
        }
        "enable" | "disable" => {
            let outcome = match (name, data["changed"] == false) {
                ("enable", true) => "Shield is already enabled for this project.",
                ("enable", false) => "Shield enabled for this project.",
                (_, true) => "Shield is already paused for this project.",
                (_, false) => "Shield paused for this project.",
            };
            with_guidance(outcome, data)
        }
        "mcp_setup" if data["operator_recovery"].is_object() => data["next_action"].as_str().unwrap_or("Ask the user to run the target-project operator recovery command.").into(),
        "mcp_setup" if data["setup_error"].is_string() => with_guidance("Shield integration setup needs attention.", data),
        "mcp_setup" => with_guidance("Shield integration settings reviewed.", data),
        "mcp_setup_preview" => "Shield integration preview ready.".into(),
        "registry_add" if data["status"] == "input_collected" => data["next_action"].as_str().unwrap_or("Registry details collected; separate trust confirmation is required.").into(),
        "registry_add" | "registry_remove" | "registry_default" | "registry_trust" => {
            let alias = data["alias"].as_str().unwrap_or("selected registry");
            let scope = data["scope"].as_str().unwrap_or("project");
            match name {
                "registry_add" => format!("Registry {alias} registered at {scope} scope."),
                "registry_remove" => format!("Registry {alias} removed from {scope} scope."),
                "registry_default" => format!("Registry {alias} is now the {scope} default."),
                _ => format!("Trust for registry {alias} updated at {scope} scope."),
            }
        }
        "pack_restore" => pack_restore_summary(data),
        "protection_set" => "Shield protected-file settings saved.".into(),
        "history_configure" if data["purged"] == true => "Shield history cleared and settings saved.".into(),
        "history_configure" => "Shield history settings saved.".into(),
        "policy_defer" => "Policy decision saved for later.".into(),
        "policy_draft_remove" => "Deferred policy decision removed.".into(),
        "policy_reject" => "Policy proposal rejected.".into(),
        "session_grant_list"|"session_grant_show"|"session_grant_propose"|"session_grant_reconsider"|"session_grant_approve"|"session_grant_revoke"=>data["next_action"].as_str().unwrap_or("Session grant inspection only; no command was executed.").into(),
        "pack_info" => format!("{} Inspection only; nothing is installed. {}", pack_lead(data), data["setup"]["message"].as_str().unwrap_or_default()).trim_end().to_owned(),
        "policy_evaluate" => {
            let prefix = format!("Policy preview: {}.", data["decision"].as_str().unwrap_or("unknown"));
            if let Some(next) = data["next_action"].as_str() { format!("{prefix} {next}") }
            else if data["decision"] != "allow" && let Some(detail) = crate::agent_message::detail(&data["explanation"]) { format!("{prefix} {detail}") }
            else { format!("{prefix} This result does not authorize execution.") }
        },
        "policy_propose" | "rule_add" | "rule_edit" | "rule_remove" | "pack_configure" | "pack_add" | "pack_update" | "pack_remove" => {
            let guidance = data["next_action"].as_str().unwrap_or(crate::agents::review::resolve(agent).guidance().proposed);
            data["change_summary"].as_str().map_or_else(|| guidance.into(), |change| format!("Proposed: {change} {guidance}"))
        },
        "policy_apply" => data["next_action"].as_str().unwrap_or("Policy change written to the policy files after host confirmation. Retry the original request only if still needed.").into(),
        "request_approve" if data["terminal"] == true || data["status"] == "terminal" => {
            data["next_action"].as_str().unwrap_or("These requests are closed. Their recorded outcomes grant no permission.").into()
        },
        "request_approve" => match data["grants"].as_array() {
            Some(grants) => format!("The user approved these {} requests once each. {}", grants.len(), data["next_action"].as_str().unwrap_or("Follow each grant's next_action.")),
            None => format!("The user approved this request once. {}", data["next_action"].as_str().unwrap_or("Repeat the request once through its original consumer with the same inputs.")),
        },
        "request_deny" => "The user denied this request or revoked its unused grant. No action was authorized.".into(),
        "suggestion_list" => {
            let rows = data["requests"].as_array();
            let eligible = rows.into_iter().flatten().filter(|row| row["eligible_for_offer"] == true).count();
            format!("{eligible} eligible review groups. Follow coverage and next_action; group reviews after the task and skip unchanged proposed/deferred items.")
        },
        "docs_list" => "Bundled guides listed.".into(),
        "docs_search" => "Matching guide lines returned; next_cursor continues the search.".into(),
        "docs_read" => "Guide text returned; next_cursor continues this page.".into(),
        "policy_withdraw" => "Policy proposal withdrawn. The policy files are unchanged.".into(),
        "policy_compare" => "Policy comparison returned. Check coverage and unknowns; this preview grants no execution permission.".into(),
        "status" | "doctor" => diagnostic_summary(data),
        _ => data["next_action"].as_str().filter(|text| !text.is_empty())
            .map_or_else(|| format!("Shield {} complete.", name.replace('_', " ")), str::to_owned),
    }
}

fn error_summary(error: &Value) -> String {
    if error["code"] == "cache_unavailable"
        && let Some(route) = error["operator_route"].as_str()
    {
        return format!(
            "Compiled policy unavailable. Ask the user to run `{route}`; retry after it succeeds."
        );
    }
    let message = error["message"]
        .as_str()
        .map(|message| {
            let message: String = message.chars().filter(|c| !c.is_control()).collect();
            format!("{}.", message.trim_end_matches('.'))
        })
        .filter(|message| message.len() > 1);
    // A routed refusal's message already names the user's step.
    let routed = error["operator_route"].is_string();
    match (error["code"].as_str().unwrap_or(""), message) {
        ("invalid_input", Some(message)) if routed => message,
        ("invalid_input", Some(message)) if message != "Request or retained state is invalid." => {
            format!("{message} Correct the arguments and retry.")
        }
        ("invalid_input", _) => "Invalid tool input. Correct the arguments and retry.".into(),
        (_, Some(message)) => message,
        _ => "Shield could not complete this request. Inspect doctor and ask the user to resolve the reported problem before retrying.".into(),
    }
}

fn pack_restore_summary(data: &Value) -> String {
    let count = |key| data[key].as_array().map_or(0, Vec::len);
    let restored = count("fetched");
    let failed = count("fetch_failures");
    let outcome = if restored == 0 && failed == 0 && data["policy_files"]["error"].is_null() {
        "Locked pack artifacts are already available.".to_owned()
    } else if failed > 0 {
        format!("Restored {restored} locked packs. {failed} could not be restored.")
    } else {
        format!("Restored {restored} locked packs.")
    };
    with_guidance(&outcome, data)
}

fn with_guidance(outcome: &str, data: &Value) -> String {
    data["next_action"]
        .as_str()
        .filter(|text| !text.is_empty())
        .map_or_else(|| outcome.to_owned(), |text| format!("{outcome} {text}"))
}

fn diagnostic_summary(data: &Value) -> String {
    let mut message = with_guidance(
        data["summary"]
            .as_str()
            .unwrap_or("Shield diagnostics inspected."),
        data,
    );
    if data["enforcement_evidence"]["saved_policy"] == "enabled"
        && data["enforcement_evidence"]["published_hooks"] == "configured"
    {
        message.push_str(" Host loading and interception are unverified.");
    }
    if let Some(checks) = data["checks"].as_array() {
        let blocking = checks
            .iter()
            .filter(|check| check["blocking"] != false)
            .count();
        let warnings = checks.len() - blocking;
        for (count, one, many) in [
            (blocking, "check needs attention", "checks need attention"),
            (warnings, "warning", "warnings"),
        ] {
            match count {
                0 => {}
                1 => {
                    let _ = write!(message, " 1 {one}.");
                }
                _ => {
                    let _ = write!(message, " {count} {many}.");
                }
            }
        }
    }
    message
}

/// What a pack inspection found, first, since hosts may preview only a few lines.
fn pack_lead(data: &Value) -> String {
    let release = &data["selected_release"];
    let pack = format!(
        "{} {}",
        release["id"].as_str().unwrap_or("Pack"),
        release["version"].as_str().unwrap_or_default()
    );
    let pack = pack.trim_end();
    let count = |value: &Value| value.as_u64().unwrap_or_default();
    if let Some(rule) = data["rule"].as_object() {
        let id = rule.get("id").and_then(Value::as_str).unwrap_or("rule");
        return match rule.get("reason").and_then(Value::as_str) {
            Some(reason) => format!("{pack} rule {id}: {reason}"),
            None => format!("{pack} rule {id}."),
        };
    }
    match data["view"].as_str() {
        Some("summary") => {
            let outcomes = &data["profiles"]["recommended"]["rule_outcomes"];
            format!(
                "{pack}: {} rules; the recommended profile declares {} Allow, {} Ask and {} Deny outcomes.{}",
                count(&data["rule_count"]),
                count(&outcomes["allow"]),
                count(&outcomes["ask"]),
                count(&outcomes["deny"]),
                pack_examples(&data["profiles"]["recommended"]["examples"])
            )
        }
        Some("settings") => format!(
            "{pack} settings: {} variables, {} preconditions.",
            data["variables"]
                .as_object()
                .map_or(0, serde_json::Map::len),
            data["preconditions"].as_array().map_or(0, Vec::len)
        ),
        _ => format!(
            "{pack}: {} rules in this page{}.",
            data["rules"].as_array().map_or(0, Vec::len),
            if data["next_cursor"].is_string() {
                "; more with next_cursor"
            } else {
                ""
            }
        ),
    }
}

fn pack_examples(examples: &Value) -> String {
    let mut text = String::new();
    for (key, label) in [("allow", "Allow"), ("ask", "Ask"), ("deny", "Deny")] {
        let descriptions: Vec<_> = examples[key]
            .as_array()
            .into_iter()
            .flatten()
            .take(2)
            .filter_map(|row| {
                let description: String = row["description"]
                    .as_str()?
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(160)
                    .collect();
                let qualifier = match (
                    row["conditional"] == true,
                    row["on_unmet_decision"].as_str(),
                ) {
                    (true, Some("ask")) => " (conditional; unmet requirements: Ask)",
                    (true, Some("deny")) => " (conditional; unmet requirements: Deny)",
                    (true, _) => " (conditional)",
                    _ if row["fallback"] == true => " (fallback rule)",
                    _ => "",
                };
                Some(format!("{description}{qualifier}"))
            })
            .collect();
        if !descriptions.is_empty() {
            let _ = write!(text, " {label} examples: {}.", descriptions.join("; "));
        }
    }
    text.push_str(" Declared outcomes are not unconditional: conditions and other policy apply. Ask follows host approval or configured fallback. Inspect the intended profile.");
    text
}

#[derive(Serialize)]
pub(super) struct TextData<'a> {
    data: Projection<'a>,
    error: &'a Value,
    missing: &'a Value,
    truncated: &'a Value,
}

enum Projection<'a> {
    Complete(&'a Value),
    Selectors(&'a Value, &'static [&'static str]),
}

impl Serialize for Projection<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Complete(data) => data.serialize(serializer),
            Self::Selectors(data, keys) => {
                let mut map = serializer.serialize_map(None)?;
                for &key in *keys {
                    if let Some(field) = data.get(key) {
                        map.serialize_entry(key, field)?;
                    }
                }
                map.end()
            }
        }
    }
}

pub(super) fn text_data<'a>(name: &str, value: &'a Value) -> TextData<'a> {
    let data = &value["data"];
    TextData {
        data: if [
            "policy_propose",
            "rule_add",
            "rule_edit",
            "rule_remove",
            "pack_configure",
            "pack_add",
            "pack_update",
            "pack_remove",
            "policy_withdraw",
        ]
        .contains(&name)
        {
            Projection::Selectors(
                data,
                &[
                    "after_candidate_id",
                    "apply_arguments",
                    "before_candidate_id",
                    "policy_revision",
                    "proposal_id",
                    "status",
                    "unchanged",
                    "committed",
                    "report_resources",
                    "preparation_task_id",
                    "review_arguments",
                    "review_action",
                    "next_action",
                ],
            )
        } else if name == "status" && data["summary"].is_string() {
            Projection::Selectors(
                data,
                &[
                    "project_root",
                    "configuration",
                    "policy_files",
                    "enforcement_evidence",
                    "policy_candidate_id",
                    "pending_approvals",
                    "pending_configuration_total",
                    "next",
                    "next_action",
                ],
            )
        } else {
            Projection::Complete(data)
        },
        error: &value["error"],
        missing: &value["missing"],
        truncated: &value["truncated"],
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn candidate_artifact_failure_preserves_recovery_without_argument_retry() {
        let message = "Shield could not build a consistent pack candidate. Continue without this proposal and report the failure";
        let text = super::summary(
            "policy_propose",
            None,
            &serde_json::json!({"error":{"code":"candidate_error","message":message,"operator_route":null}}),
        );
        assert_eq!(text, format!("{message}."));
        assert!(!text.contains("Correct the arguments"));
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn administrative_results_lead_with_the_outcome_and_its_action() {
        let enabled = json!({"data":{"changed":true,"enabled":true},"error":null});
        assert_eq!(
            summary("enable", None, &enabled),
            "Shield enabled for this project."
        );
        let hub = json!({"data":{"setup_status":"configured","next":{"server":"hub"},
            "next_action":"Run Hub setup for this project."},"error":null});
        assert_eq!(
            summary("setup", None, &hub),
            "Hub publication pending. Run Hub setup for this project."
        );
    }

    #[test]
    fn pack_summary_explains_allowances_and_retained_approval_boundaries() {
        let data = json!({"view":"summary","selected_release":{"id":"acme/runtime","version":"1.0.0"},
            "profiles":{"recommended":{"rule_outcomes":{"allow":1,"ask":2,"deny":1},"examples":{
                "allow":[{"description":"Inspect installed dependencies.","conditional":true,"on_unmet_decision":"deny"}],
                "ask":[{"description":"Adding dependencies can run their code."},{"description":"Tests execute project code."}],
                "deny":[{"description":"Configuration writes require operator control."}]}}}});
        let text = summary("pack_info", None, &json!({"data":data}));
        for expected in [
            "Allow examples: Inspect installed dependencies",
            "Ask examples: Adding dependencies",
            "Tests execute project code",
            "Deny examples: Configuration writes",
            "not unconditional",
            "configured fallback",
            "intended profile",
            "conditional; unmet requirements: Deny",
        ] {
            assert!(text.contains(expected), "{text}");
        }
    }

    #[test]
    fn result_summaries_lead_with_what_was_found_or_proposed() {
        let release = json!({"id":"acme/runtime","version":"1.0.0"});
        for (data, lead) in [
            (
                json!({"view":"summary","selected_release":release,"rule_count":3,
                    "profiles":{"recommended":{"rule_outcomes":{"allow":1,"ask":2,"deny":0}}}}),
                "acme/runtime 1.0.0: 3 rules; the recommended profile declares 1 Allow, 2 Ask and 0 Deny outcomes.",
            ),
            (
                json!({"selected_release":release,"rules":[{}],"next_cursor":"acme-cursor"}),
                "acme/runtime 1.0.0: 1 rules in this page; more with next_cursor.",
            ),
            (
                json!({"selected_release":release,"rule":{"id":"install","reason":"Install dependencies."}}),
                "acme/runtime 1.0.0 rule install: Install dependencies.",
            ),
        ] {
            let text = summary("pack_info", None, &json!({"data":data,"error":null}));
            assert!(text.starts_with(lead), "{text}");
            assert!(text.contains("nothing is installed"));
            assert!(!text.contains("pack_add collects"));
            assert!(!text.contains("policy_propose"));
        }
        let proposed = summary(
            "policy_propose",
            Some("codex"),
            &json!({"data":{"change_summary":"Add the runtime rule pack."},"error":null}),
        );
        assert!(
            proposed.starts_with("Proposed: Add the runtime rule pack. Inactive change prepared")
        );
    }

    #[test]
    fn dependency_refusals_name_the_dependents_instead_of_doctor_guidance() {
        let error = json!({"code":"dependency_required","operator_route":null,
            "pack":"acme-registry/acme/git","dependents":["acme-registry/acme/workflow"],
            "message":"Pack acme-registry/acme/git is required by acme-registry/acme/workflow. Keep it, or first remove the packs that depend on it; inspect them with pack_list"});
        let text = summary("policy_propose", None, &json!({"data":null,"error":error}));
        assert!(text.contains("acme-registry/acme/workflow"), "{text}");
        assert!(!text.contains("doctor"), "{text}");
    }

    #[test]
    fn routed_invalid_input_keeps_the_user_step_without_argument_correction() {
        let error = json!({"code":"invalid_input","operator_route":"shield registry add",
            "message":"A dependency needs registry acme-foreign, which this project has not added. Ask the user to add it with shield registry add, then retry"});
        let text = summary("policy_propose", None, &json!({"data":null,"error":error}));
        assert!(text.contains("shield registry add"), "{text}");
        assert!(!text.contains("Correct the arguments"), "{text}");
    }

    #[test]
    fn capacity_refusals_keep_their_counts_and_review_route_in_text() {
        let error = json!({"code":"capacity_reached","operator_route":"shield request list",
            "message":"Capacity reached for run requests: 120 pending, 128 retained, limit 128; ask the user to review them with shield request list (or deny with shield request deny), then retry"});
        let text = summary("status", None, &json!({"data":null,"error":error}));
        assert!(
            text.contains("120 pending") && text.contains("shield request list"),
            "{text}"
        );
    }
}
