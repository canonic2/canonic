//! Short agent guidance plus bounded text-only inspection/selector compatibility.
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;
use std::fmt::Write as _;

/// Initialization guidance uses the enrolled session agent, not client display metadata.
pub(super) fn instructions(agent: Option<&str>) -> String {
    let common = "Shield manages project permissions. For requested reviews, start with suggestion_list; inspect relevant history_list, rule_list or policy_evaluate evidence. \
Respect incomplete evidence, pending proposals and deferred reviews. For authorized custom rules, use policy_propose and its schema examples; preserve scope and deny boundaries. Read bundled docs only when rule syntax needs clarification. \
Previews and proposals grant no authority; activation requires user-approved policy_apply or the returned terminal route. \
Follow returned request_approve routes for one-time approval, then repeat the approved call once unchanged. Do not bypass refusals or interrupt unrelated work with reviews. ";
    let host = crate::agents::review::resolve(agent)
        .guidance()
        .instructions;
    format!(
        "{common}{} {host}Chat replies cannot replace host or terminal confirmation.",
        crate::proposal_draft::PACK_SETUP_GUIDANCE
    )
}

pub(super) fn approval_unavailable(agent: Option<&str>) -> &'static str {
    crate::agents::review::resolve(agent).guidance().unavailable
}

/// The structured message of a declined host confirmation.
pub(super) const APPROVAL_DECLINED: &str = "The policy change was not approved and remains pending. Continue without it; do not retry unless the user requests it.";

/// The user declined an in-session approval.
pub(super) const REQUEST_DECLINED: &str = "The user declined this request. Do not retry it; continue without it or ask the user what they want instead.";

/// The host could not show Shield's form; the remedy stays in the session.
pub(super) fn request_unavailable(agent: Option<&str>) -> &'static str {
    match agent {
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
    match name {
        "pack_info" => format!("{} Inspection only; nothing is installed. {}", pack_lead(data), data["setup"]["message"].as_str().unwrap_or_default()).trim_end().to_owned(),
        "policy_evaluate" => {
            let prefix = format!("Policy preview: {}.", data["decision"].as_str().unwrap_or("unknown"));
            if data["decision"] != "allow" && let Some(detail) = crate::agent_message::detail(&data["explanation"]) { format!("{prefix} {detail}") }
            else { format!("{prefix} This result does not authorize execution.") }
        },
        "policy_propose" | "pack_configure" | "pack_add" => {
            let guidance = data["next_action"].as_str().unwrap_or(crate::agents::review::resolve(agent).guidance().proposed);
            data["change_summary"].as_str().map_or_else(|| guidance.into(), |change| format!("Proposed: {change} {guidance}"))
        },
        "policy_apply" => "Policy change active after host confirmation. Retry the original request only if still needed.".into(),
        "request_approve" => match data["grants"].as_array() {
            Some(grants) => format!("The user approved these {} requests once each. Repeat each identical call now, in any order; any change to one needs a new approval.", grants.len()),
            None => "The user approved this request once. Repeat the identical call now; any change to it needs a new approval.".into(),
        },
        "suggestion_list" => {
            let rows = data["requests"].as_array();
            let eligible = rows.into_iter().flatten().filter(|row| row["eligible_for_offer"] == true).count();
            format!("{eligible} eligible review groups. Follow coverage and next_action; group reviews after the task and skip unchanged proposed/deferred items.")
        },
        "shield_docs_list" => "Bundled guides listed. Read a path with shield_docs_read or find a topic with shield_docs_search.".into(),
        "shield_docs_search" => "Matching guide lines returned. Use shield_docs_read for context; next_offset continues the search.".into(),
        "shield_docs_read" => "Guide text returned; next_start_line continues this page.".into(),
        "policy_withdraw" => "Policy proposal withdrawn. Active policy is unchanged.".into(),
        "policy_receipt" => "Policy change receipt returned. Inspect current policy and reevaluate the request before retrying.".into(),
        "policy_compare" => "Policy comparison returned. Check coverage and unknowns; this preview grants no execution permission.".into(),
        "doctor" if data["repair_needed"] == true || data["checks"].as_array().is_some_and(|c| !c.is_empty()) => "Shield needs repair. Ask the user to follow the returned repair route before retrying blocked requests.".into(),
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
            "pack_configure",
            "pack_add",
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
        assert!(proposed.starts_with("Proposed: Add the runtime rule pack. Policy proposal saved"));
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
