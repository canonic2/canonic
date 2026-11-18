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
    let common = "Lists take cursor and limit (default 50, maximum 100; suggestions default 16). Follow next_cursor with unchanged filters until null; invalid_cursor means restart without a cursor. Previews name the tool or section for the rest. Shield manages project permissions. When an Ask or Deny blocks work that will recur, or the user wants something allowed, blocked or always asked about, propose a narrow custom rule as soon as you need it, following skill://shield/custom-rules/SKILL.md: check policy_evaluate and rule_list, then use rule_add or policy_propose; preserve scope and deny boundaries. Nothing changes until the user approves policy_apply. \
suggestion_list returns this session's asked requests with evidence and authoring examples; respect incomplete evidence, pending proposals and deferred reviews. \
For packs, use pack_list and pack_show to inspect installed selections, pack_update to update one reference or all tracking packs when omitted, and pack_remove to prepare removal; omit its reference to let the person select installed packs. pack_configure accepts direct edits, named variables or bare guided settings and overrides. pack_add accepts known typed settings through set. registry_add can collect omitted name/location details through a form; then call it with the returned retry_arguments for a separate trust review. Input submission cannot approve trust or policy. \
Previews and proposals grant no authority; the policy files change only through user-approved policy_apply. A preparation Task grants no authority either: poll tasks/get, use its review_arguments with policy_propose to create a fresh review, then obtain separate human confirmation using unchanged apply_arguments. Repeating identical creation arguments recovers a lost task handle; never restart failed or cancelled work automatically. Read returned report_resources with resources/read when full comparison or retained review evidence is needed; a report does not approve a change. Keep approval and setup in this conversation through Shield tools and host confirmation; sending the user to a terminal is a product bug, not a recovery solution. \
Follow returned request_approve routes for one-time approval, then repeat the approved call once unchanged. For a planned batch of privileged native commands, inspect session_grant_list, propose a bounded exact batch or closed argv family with session_grant_propose, then copy apply_arguments to session_grant_approve for the user's form. Prefer temporary session permission for one task and persistent policy only for future sessions. Inspect existing grants first, never repeat a declined offer automatically, and never bypass refusals. Only the person's explicit request permits session_grant_reconsider followed by fresh human confirmation. ";
    let continuations = "For a large administrative review, review_status: more_pages means nothing has applied: call retry_tool with the exact retry_arguments to present the next human confirmation page. Never skip pages or repeat a declined review. history_list and history_show accept an explicit absolute project for an already enrolled nested worktree inside this connection's project; this is a read-only selector and cannot reach unrelated projects. ";
    let host = adapter.guidance().instructions;
    format!(
        "{common}{continuations}{} {host}Chat replies cannot replace host confirmation.",
        crate::proposal_draft::PACK_SETUP_GUIDANCE
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
    if data["review_status"] == "more_pages" {
        return format!(
            "Accepted {} of {} review pages. Nothing has applied. Call {} again with the exact retry_arguments for the next human confirmation page.",
            data["reviewed_pages"],
            data["total_pages"],
            data["retry_tool"].as_str().unwrap_or(name)
        );
    }
    if data["unchanged"] == true {
        return "Already up to date; no proposal or approval is needed.".into();
    }
    match name {
        "mcp_setup" if data["operator_recovery"].is_object() => data["next_action"].as_str().unwrap_or("Ask the user to run the target-project operator recovery command.").into(),
        "registry_add" if data["status"] == "input_collected" => data["next_action"].as_str().unwrap_or("Registry details collected; separate trust confirmation is required.").into(),
        "session_grant_list"|"session_grant_show"|"session_grant_propose"|"session_grant_reconsider"|"session_grant_approve"|"session_grant_revoke"=>data["next_action"].as_str().unwrap_or("Session grant inspection only; no command was executed.").into(),
        "pack_info" => format!("{} Inspection only; nothing is installed. {}", pack_lead(data), data["setup"]["message"].as_str().unwrap_or_default()).trim_end().to_owned(),
        "policy_evaluate" => {
            let prefix = format!("Policy preview: {}.", data["decision"].as_str().unwrap_or("unknown"));
            if data["decision"] != "allow" && let Some(detail) = crate::agent_message::detail(&data["explanation"]) { format!("{prefix} {detail}") }
            else { format!("{prefix} This result does not authorize execution.") }
        },
        "policy_propose" | "rule_add" | "rule_edit" | "rule_remove" | "pack_configure" | "pack_add" | "pack_update" | "pack_remove" => {
            let guidance = data["next_action"].as_str().unwrap_or(crate::agents::review::resolve(agent).guidance().proposed);
            data["change_summary"].as_str().map_or_else(|| guidance.into(), |change| format!("Proposed: {change} {guidance}"))
        },
        "policy_apply" => data["next_action"].as_str().unwrap_or("Policy change written to the policy files after host confirmation. Retry the original request only if still needed.").into(),
        "request_approve" => match data["grants"].as_array() {
            Some(grants) => format!("The user approved these {} requests once each. Follow each grant's next_action. Checked commands require --approval-request with their request_id; repeat every call unchanged through its original consumer.", grants.len()),
            None => "The user approved this request once. Repeat the identical call now; any change to it needs a new approval.".into(),
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
        "doctor" if data["repair_needed"] == true || data["checks"].as_array().is_some_and(|checks| checks.iter().any(|row| row["blocking"] != false)) => "Shield needs repair. Ask the user to follow the returned repair route before retrying blocked requests.".into(),
        "doctor" if data["checks"].as_array().is_some_and(|checks| !checks.is_empty()) => "Shield remains usable; inspect the nonblocking storage warnings and their paths.".into(),
        _ => "Records returned. Check missing and truncated evidence.".into(),
    }
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
