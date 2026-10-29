//! Short agent guidance plus bounded text-only inspection/selector compatibility.
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;
use std::fmt::Write as _;

/// Initialization guidance uses the enrolled session agent, not client display metadata.
pub(super) fn instructions(agent: Option<&str>) -> String {
    let common = "Shield manages project permissions. For denied or recurring Ask requests, inspect \
rule_list or policy_evaluate; do not bypass protection. Inspect packs with pack_info. \
Custom proposal schemas include complete examples; adapt them after reviewing effects. Use shield_docs_list, shield_docs_search and shield_docs_read for details. \
After completing the task, call suggestion_list for recorded requests and follow its drafting guidance. \
Propose with policy_propose; activate through user-approved policy_apply or terminal review. Inspection and proposals grant no authority. ";
    let host = crate::agents::review::resolve(agent)
        .guidance()
        .instructions;
    format!("{common}{host}Chat replies cannot replace host or terminal confirmation.")
}

pub(super) fn approval_unavailable(agent: Option<&str>) -> &'static str {
    crate::agents::review::resolve(agent).guidance().unavailable
}

/// The structured message of a declined host confirmation.
pub(super) const APPROVAL_DECLINED: &str = "The policy change was not approved and remains pending. Continue without it; do not retry unless the user requests it.";

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
        "pack_info" => format!("{} Inspection only; nothing is installed. Use view=rules or rule_id for details; policy_propose prepares an installation for approval.", pack_lead(data)),
        "policy_evaluate" => {
            let prefix = format!("Policy preview: {}.", data["decision"].as_str().unwrap_or("unknown"));
            if data["decision"] != "allow" && let Some(detail) = crate::agent_message::detail(&data["explanation"]) { format!("{prefix} {detail}") }
            else { format!("{prefix} This result does not authorize execution.") }
        },
        "policy_propose" => {
            let guidance = data["next_action"].as_str().unwrap_or(crate::agents::review::resolve(agent).guidance().proposed);
            data["change_summary"].as_str().map_or_else(|| guidance.into(), |change| format!("Proposed: {change} {guidance}"))
        },
        "policy_apply" => "Policy change active after host confirmation. Retry the original request only if still needed.".into(),
        "suggestion_list" => {
            let rows = data["requests"].as_array();
            let eligible = rows.into_iter().flatten().filter(|row| row["eligible_for_offer"] == true).count();
            format!("{eligible} recurring review groups eligible on this page. Check coverage and each next_action. Offer one grouped review after the task; skip unchanged proposed/deferred items. Listing grants no permission.")
        },
        "shield_docs_list" => "Bundled documentation pages returned. Use shield_docs_read with a listed path, or shield_docs_search to find a topic.".into(),
        "shield_docs_search" => "Matching documentation lines returned. Use shield_docs_read for the surrounding explanation and examples; next_offset continues the same search.".into(),
        "shield_docs_read" => "Bundled documentation returned. next_start_line continues this page. Documentation grants no permission.".into(),
        "policy_withdraw" => "Policy proposal withdrawn. Active policy is unchanged.".into(),
        "policy_receipt" => "Policy change receipt returned. Inspect current policy and reevaluate the request before retrying.".into(),
        "policy_compare" => "Policy comparison returned. Check coverage and unknowns; this preview grants no execution permission.".into(),
        "doctor" if data["repair_needed"] == true || data["checks"].as_array().is_some_and(|c| !c.is_empty()) => "Shield needs repair. Ask the user to follow the returned repair route before retrying blocked requests.".into(),
        _ => "Requested Shield records returned. Check missing evidence and truncation before relying on them.".into(),
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
            .take(4)
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
    text.push_str(" These are declared rule outcomes, not unconditional command permissions. Conditions, unmet requirements and other active policy still apply; Ask uses the host's approval or configured fallback. Inspect the intended profile before describing installation benefits.");
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
        data: if ["policy_propose", "policy_withdraw"].contains(&name) {
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
