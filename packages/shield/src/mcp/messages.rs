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
    let inspection = "Shield manages project permissions. Call setup to initialize or repair the project's integrations, follow the returned action, then check status and doctor. Disabled enforcement and configured hooks do not establish live interception. Inspect decisions with policy_evaluate, rules with rule_list, and installed packs with pack_list or pack_show. If the request is allowed, continue with the normal tool. Use suggestion_list for this session's evidence and existing reviews. \
Use history_list or history_show with an absolute project path for history in an enrolled nested worktree. For more results, follow next_cursor with the same filters. Restart without a cursor on invalid_cursor. ";
    let host = adapter.guidance().instructions;
    if adapter.confirmation() == crate::agents::review::Confirmation::Terminal {
        return format!("{inspection}{host}");
    }
    let approvals = "For one-time approval, follow request_approve and repeat the approved call once unchanged. Ordinary session grants cover eligible Ask decisions only. For a repeated Ask, inspect session_grant_list, propose bounded commands with session_grant_propose, and pass its unchanged apply_arguments to session_grant_approve. Follow denial-specific recovery for Deny. Use session_grant_reconsider only on explicit request and obtain fresh confirmation. \
Prefer temporary session grants for work limited to this task. Propose persistent policy when the permission should carry over to future sessions. Open skill://shield/custom-rules/SKILL.md via MCP resources/read. Check policy_evaluate and rule_list, preserve scope and deny boundaries, and use rule_add or policy_propose. \
Update a named pack with pack_update and its reference. Omit reference only to update all direct packs, including pinned packs. Use pack_info for setup settings before pack_add and pack_configure for installed settings. Use pack_remove with a reference, or let the person choose what to remove. When the user wants a pack, rule or registry change applied, set review: true to request human confirmation after preparation. Follow a returned review_action when the host needs a separate call. Registry input and source trust require separate decisions. \
For a separately prepared proposal, use policy_review with its proposal_id or pass unchanged apply_arguments to policy_apply for native tool approval. A preview, report, prepared Task or chat reply grants no approval. For a preparation Task, poll tasks/get and pass its review_arguments to policy_propose. Recover a lost handle with identical creation arguments. Read report_resources via resources/read for more evidence. When an administrative review requires the operator terminal, ask the person to run the returned operator_action. Stop on a decline. Never bypass a refusal or automatically restart failed or cancelled work. ";
    let form_limit = crate::review::hook_approval::MAX_FORM;
    format!(
        "{inspection}When an edit needs one-time approval, the complete form fits at most {form_limit} UTF-8 bytes, including the patch, paths and review text. Before submitting a large patch that needs approval, split it into smaller complete patches at file or hunk boundaries. {approvals}{host}"
    )
}

pub(super) fn approval_unavailable(agent: Option<&str>) -> &'static str {
    if matches!(agent, Some("gemini" | "agy")) {
        return "This host did not provide Shield's confirmation form. Report the missing in-session capability; do not ask the user to run terminal commands for approval.";
    }
    crate::agents::review::resolve(agent).guidance().unavailable
}

/// What a call stopped at its request time limit means and how to continue.
/// Calls wait in order without a limit, so this covers only a call's own work.
/// `guided` calls collect input through a form; repeating one asks the user
/// again and prepares a new review, so they check for a saved one instead.
pub(super) fn deadline(name: &str, guided: bool, review: bool) -> &'static str {
    match name {
        _ if review && crate::review::mcp::prepares_review(name) => {
            "Shield stopped this call at its time limit. Call policy_show to check the review and its outcome before asking again."
        }
        _ if guided && crate::review::mcp::prepares_review(name) => {
            "Shield stopped preparing this review at its time limit, so nothing was applied, but a review may have been saved. Call policy_show with status pending before calling the tool again, so the user is not asked twice. If it stops again, tell the user Shield timed out on this call and include the doctor tool's result."
        }
        _ if crate::review::mcp::prepares_review(name) => {
            "Shield stopped preparing this review at its time limit, so nothing was applied. Call the tool again with the same arguments; it returns the review if one was saved. If it stops again, tell the user Shield timed out on this call and include the doctor tool's result."
        }
        "policy_apply" | "policy_review" | "policy_withdraw" | "policy_reject" => {
            "Shield stopped this call at its time limit; whether it took effect is unknown. Call policy_show with its proposal_id before calling it again."
        }
        "request_approve" | "request_deny" => {
            "Shield stopped this call at its time limit; whether it took effect is unknown. Call request_show for each request ID it named before calling it again."
        }
        "session_grant_propose"
        | "session_grant_reconsider"
        | "session_grant_approve"
        | "session_grant_revoke" => {
            "Shield stopped this call at its time limit; whether it took effect is unknown. Call session_grant_show with its proposal_id or grant_id, or with session_ref to list them, before calling it again."
        }
        "policy_defer" | "policy_draft_remove" => {
            "Shield stopped this call at its time limit; whether it took effect is unknown. Call policy_draft_list before calling it again."
        }
        "registry_add" if guided && !review => {
            "Shield stopped collecting registry details at its time limit. Nothing was enrolled. If the details are still needed, ask the user before opening the form again."
        }
        "registry_add" => {
            "Shield stopped this call at its time limit. Whether the registry was enrolled is unknown. Call registry_list with the original scope and check the named alias and source before calling it again."
        }
        "registry_default" => {
            "Shield stopped this call at its time limit. Whether the default registry changed is unknown. Call registry_list with the original scope and check the current default before calling it again."
        }
        "registry_remove" => {
            "Shield stopped this call at its time limit. Whether the registry was removed is unknown. Call registry_list with the original scope and check whether the named alias is still registered in that scope before calling it again."
        }
        "review_defer" => {
            "Shield stopped this call at its time limit. Whether the reviews were deferred is unknown. Call suggestion_list with the original session_id and check review_status for the named review_ids before calling it again."
        }
        _ if crate::review::mcp::mutates(name) => {
            "Shield stopped this call at its time limit; whether it took effect is unknown. Call status before calling it again."
        }
        _ => {
            "Shield stopped this read at its time limit; nothing changed. Call it once more. If it stops again, tell the user Shield timed out on this call and include the doctor tool's result."
        }
    }
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
    let primary = primary_summary(name, agent, value);
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
        return match (error["code"].as_str().unwrap_or(""), message) {
            ("invalid_input", Some(message)) if routed => message,
            ("invalid_input", Some(message)) if message != "Request or retained state is invalid." => {
                format!("{message} Correct the arguments and retry.")
            }
            ("invalid_input", _) => "Invalid tool input. Correct the arguments and retry.".into(),
            (_, Some(message)) => message,
            _ => "Shield could not complete this request. Inspect doctor and ask the user to resolve the reported problem before retrying.".into(),
        };
    }
    let data = &value["data"];
    if data["unchanged"] == true {
        return "Already up to date; no proposal or approval is needed.".into();
    }
    match name {
        "setup" => {
            let outcome = match data["setup_status"].as_str() {
                Some("initialized") => "Shield set up for this project.",
                Some("unchanged") => "Shield setup is current.",
                Some("configured") => "Shield integrations configured.",
                Some("resumed") => "Previously approved Shield setup finished.",
                _ => "Shield setup needs attention.",
            };
            format!("{outcome} {}", data["next_action"].as_str().unwrap_or_default()).trim_end().into()
        }
        "mcp_setup" if data["operator_recovery"].is_object() => data["next_action"].as_str().unwrap_or("Ask the user to run the target-project operator recovery command.").into(),
        "registry_add" if data["status"] == "input_collected" => data["next_action"].as_str().unwrap_or("Registry details collected; separate trust confirmation is required.").into(),
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
        "status" | "doctor" => setup_status(data),
        _ => "Records returned. Check missing and truncated evidence.".into(),
    }
}

/// Hosts often show only the first text block. Installed hooks and a usable
/// MCP connection must never be presented as proof of active enforcement.
fn setup_status(data: &Value) -> String {
    let mut message = if data["enabled"] == false {
        "Shield enforcement is disabled. Call enable to review resuming it; protected-file and operator guards still apply.".to_owned()
    } else if data["policy_files"]["error"].is_string() {
        "Shield cannot load its policy files. Inspect policy_files.error and the doctor checks."
            .to_owned()
    } else {
        match data["enforcement"].as_str() {
            Some("installed") => "Shield enforcement is enabled and native hooks are configured.".into(),
            Some("defined-for-hub") => "Shield is defined for Hub but native hooks are not published. Call Hub's setup tool to review publication.".into(),
            Some("defined-for-box") => "Shield is defined for Box but native hooks are not published. Use Box's config_sync to review publication.".into(),
            Some("not-installed") => "Shield has no native hooks configured. Call mcp_setup to review connecting agents.".into(),
            Some("degraded") => "Shield's integration configuration needs attention.".into(),
            _ => "Shield configuration inspected.".into(),
        }
    };
    if data["repair_needed"] == true
        || data["checks"]
            .as_array()
            .is_some_and(|checks| checks.iter().any(|row| row["blocking"] != false))
    {
        message.push_str(" Inspect coverage diagnostics and checks for the exact affected settings. Review Shield-owned changes with mcp_setup; use Hub's setup for Hub-published hooks. Third-party server settings belong to their publisher.");
    } else if data["checks"]
        .as_array()
        .is_some_and(|checks| !checks.is_empty())
    {
        message.push_str(" Inspect the nonblocking warnings.");
    }
    message.push_str(
        " Current host loading and live policy interception are unverified by this report.",
    );
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
    Selectors(&'a Value),
}

impl Serialize for Projection<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Complete(data) => data.serialize(serializer),
            Self::Selectors(data) => {
                let mut map = serializer.serialize_map(None)?;
                // Preserve the canonical key order of the compatibility text.
                for key in [
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
                ] {
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
            Projection::Selectors(&value["data"])
        } else {
            Projection::Complete(&value["data"])
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
    fn setup_reports_distinguish_disabled_unpublished_and_configured_hooks() {
        for tool in ["status", "doctor"] {
            for (data, expected) in [
                (
                    json!({"enabled":false,"enforcement":"installed"}),
                    "Shield enforcement is disabled.",
                ),
                (
                    json!({"enabled":true,"enforcement":"defined-for-hub"}),
                    "Shield is defined for Hub but native hooks are not published.",
                ),
                (
                    json!({"enabled":true,"enforcement":"not-installed"}),
                    "Shield has no native hooks configured.",
                ),
                (
                    json!({"enabled":true,"enforcement":"installed"}),
                    "Shield enforcement is enabled and native hooks are configured.",
                ),
                (
                    json!({"enabled":true,"enforcement":"installed","policy_files":{"error":"invalid policy"}}),
                    "Shield cannot load its policy files.",
                ),
            ] {
                let message = summary(tool, Some("codex"), &json!({"data":data}));
                assert!(message.starts_with(expected), "{message}");
                assert!(
                    message.contains("live policy interception are unverified"),
                    "{message}"
                );
            }
            let message = summary(
                tool,
                None,
                &json!({"data":{"enabled":true,"enforcement":"degraded","repair_needed":true}}),
            );
            assert!(message.contains("mcp_setup"), "{message}");
            assert!(message.contains("Hub's setup"), "{message}");
            assert!(!message.contains("Ask the user to follow"), "{message}");
        }
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
