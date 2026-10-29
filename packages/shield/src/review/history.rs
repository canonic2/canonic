//! Request history joins observations with action authorization evidence.
use super::{Context, Error};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct Query<'a> {
    pub entry: Option<&'a str>,
    pub denied: bool,
    pub approved: bool,
    pub agent: Option<&'a str>,
}

pub(super) fn inspect(context: &Context, input: &Query<'_>) -> Result<Value, Error> {
    let state = crate::review::storage::Store::open(std::path::Path::new(&context.project_root))?
        .read_history()?;
    state.check(context)?;
    let entries = request_entries(&state.events)?;
    let mut rows: Vec<_> = entries
        .into_values()
        .filter(|row| {
            (!input.denied || row["denied"] == true)
                && (!input.approved || row["approved"] == true)
                && input.agent.is_none_or(|a| agent_matches(row, a))
        })
        .collect();
    rows.sort_by(|a, b| {
        b["requested_at"]
            .as_str()
            .cmp(&a["requested_at"].as_str())
            .then_with(|| b["_order"].as_u64().cmp(&a["_order"].as_u64()))
    });
    rows.truncate(state.history_limit);
    for row in &mut rows {
        if let Some(fields) = row.as_object_mut() {
            fields.remove("_order");
        }
        bound_entry(row);
    }
    if let Some(entry) = input.entry {
        return rows
            .into_iter()
            .find(|r| r["entry_id"] == entry)
            .ok_or(Error::Missing);
    }
    let truncated = rows.iter().any(|r| r["truncated"] == true);
    let mut missing = Vec::new();
    if truncated {
        missing.push("some_command_evidence");
    }
    if !state.history_available {
        missing.push("optional_history");
    }
    if state.history_evicted {
        missing.push("evicted_history");
    }
    Ok(
        json!({"policy_revision":state.policy_revision,"enabled":state.history_enabled,"limit":state.history_limit,"available":state.history_available,"missing":missing,"truncated":truncated,"entries":rows}),
    )
}

/// Match an agent filter given as a CLI id or a recorded history name.
pub(super) fn agent_matches(row: &Value, filter: &str) -> bool {
    row["provenance"]["agent"]
        .as_str()
        .is_some_and(|recorded| crate::agents::same_agent(filter, recorded))
}

/// Group only by retained request IDs; command text and event order cannot
/// disambiguate host calls that lack an explicit mapping.
fn request_entries(events: &[Value]) -> Result<BTreeMap<String, Value>, Error> {
    let mut entries: BTreeMap<String, Value> = BTreeMap::new();
    for (order, event) in events.iter().enumerate() {
        let kind = event["kind"].as_str().unwrap_or_default();
        if !(kind == "decision"
            || kind == "completion"
            || kind.starts_with("read_grant_")
            || kind.starts_with("run_grant_"))
        {
            continue;
        }
        let Some(id) = event["links"]["request_id"].as_str() else {
            continue;
        };
        let entry=entries.entry(id.into()).or_insert_with(||json!({"entry_id":id,"requested_at":event["recorded_at"],"provenance":event["provenance"],"observations":[],"approved":false,"denied":false,"outcome":"unobserved","_order":order}));
        if kind.ends_with("_issued") {
            entry["approved"] = json!(true);
        }
        // Revoking a pending request rejects it; revoking an issued grant
        // keeps the entry approved.
        if kind.ends_with("_revoked") && event["links"]["approval_id"].is_null() {
            entry["denied"] = json!(true);
        }
        if kind == "decision" && entry.get("command").is_none() {
            retain_decision(entry, &event["details"]);
        }
        if kind == "decision" {
            let details = &event["details"];
            if details["evaluation"]["decision"] == "deny"
                || details["evaluation"]["evaluation"]["decision"] == "deny"
            {
                entry["denied"] = json!(true);
            }
        }
        if kind == "completion" {
            entry["outcome"] = event["details"].clone();
            if event["details"]["delivery_conflict"] == true {
                entry["delivery_conflict"] = json!(true);
            }
        }
        entry["observations"]
            .as_array_mut()
            .ok_or(Error::Invalid("history observations"))?
            .push(event.clone());
    }
    Ok(entries)
}

/// Preserve the first decision separately from later retries and completions.
fn retain_decision(entry: &mut Value, details: &Value) {
    entry["command"] = details
        .get("command")
        .filter(|v| !v.is_null())
        .or_else(|| details.get("operation"))
        .cloned()
        .unwrap_or(Value::Null);
    for field in [
        "request",
        "evaluation",
        "disposition",
        "code",
        "ask_fallback",
        "integration",
    ] {
        entry[field] = details[field].clone();
    }
}

fn bound_entry(row: &mut Value) {
    let observations = row["observations"].as_array().cloned().unwrap_or_default();
    let missing: std::collections::BTreeSet<_> = observations
        .iter()
        .flat_map(|e| e["details"]["missing"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    row["truncated"] =
        json!(!missing.is_empty() || observations.iter().any(|e| e["truncated"] == true));
    row["missing"] = json!(missing);
    if row.to_string().len() <= 8192 {
        return;
    }
    row["observations"] = json!(
        observations
            .first()
            .into_iter()
            .chain(observations.last().filter(|_| observations.len() > 1))
            .collect::<Vec<_>>()
    );
    mark_missing(row, "intermediate_observations");
    if row.to_string().len() > 8192 {
        row["observations"] = json!([]);
        mark_missing(row, "observation_details");
    }
    if row.to_string().len() > 8192 {
        row["evaluation"] = json!({"decision":row["evaluation"]["decision"],"revision":row["evaluation"]["revision"]});
        mark_missing(row, "full_evaluation");
    }
    if row.to_string().len() > 8192 {
        row["request"] = Value::Null;
        row["command"] = Value::Null;
        mark_missing(row, "complete_input");
    }
    if row.to_string().len() > 8192 {
        if row["outcome"].is_object() {
            row["outcome"] = json!({"status":row["outcome"]["status"],"exit_code":row["outcome"]["exit_code"],"evidence":row["outcome"]["evidence"]});
        }
        row["provenance"] = Value::Null;
        mark_missing(row, "outcome_and_attribution_details");
    }
}
fn mark_missing(row: &mut Value, field: &str) {
    row["truncated"] = json!(true);
    if let Some(missing) = row["missing"].as_array_mut() {
        missing.push(json!(field));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(id: &str, host: &str, tool: &Value, outcome: &str) -> Value {
        json!({"kind":"decision","links":{"request_id":id},
            "provenance":{"surface":"hook","agent":"codex","cwd":"/acme",
                "session_id":"acme-session","host_event_name":host,"tool_use_id":tool},
            "details":{"operation":{"kind":"shell","source":"npm run dev"},
                "evaluation":{"decision":outcome}}})
    }

    #[test]
    fn concurrent_identical_commands_keep_permission_denials_separate() {
        let events = [
            decision("first", "PreToolUse", &json!("first-call"), "ask"),
            decision("second", "PreToolUse", &json!("second-call"), "ask"),
            decision("permission", "PermissionRequest", &Value::Null, "deny"),
        ];
        let entries = request_entries(&events).unwrap();
        assert_eq!(entries.len(), 3);
        for (index, id) in ["first", "second"].into_iter().enumerate() {
            assert_eq!(entries[id]["denied"], false);
            assert_eq!(entries[id]["observations"], json!([events[index]]));
        }
        assert_eq!(entries["permission"]["denied"], true);
        assert_eq!(entries["permission"]["observations"], json!([events[2]]));
    }

    #[test]
    fn permission_requests_keep_distinct_cwd_and_agent_identities() {
        for (field, other) in [("cwd", "/acme/other"), ("agent", "claude-code")] {
            let pre = decision("pre", "PreToolUse", &json!("acme-call"), "ask");
            let mut permission = decision("permission", "PermissionRequest", &Value::Null, "deny");
            permission["provenance"][field] = json!(other);
            let entries = request_entries(&[pre.clone(), permission.clone()]).unwrap();
            assert_eq!(entries.len(), 2, "{field}");
            assert_eq!(entries["pre"]["denied"], false, "{field}");
            assert_eq!(entries["pre"]["provenance"], pre["provenance"]);
            assert_eq!(entries["permission"]["denied"], true, "{field}");
            assert_eq!(
                entries["permission"]["provenance"],
                permission["provenance"]
            );
        }
    }

    #[test]
    fn agent_filter_accepts_the_cli_id_or_the_recorded_history_name() {
        let row = |agent: &str| json!({"provenance":{"agent":agent}});
        for filter in ["claude", "claude-code"] {
            assert!(agent_matches(&row("claude-code"), filter), "{filter}");
            assert!(!agent_matches(&row("codex"), filter), "{filter}");
        }
        assert!(agent_matches(&row("codex"), "codex"));
        assert!(agent_matches(&row("acme-agent"), "acme-agent"));
        assert!(!agent_matches(&row("acme-agent"), "claude"));
        assert!(!agent_matches(&json!({"provenance":null}), "claude"));
    }

    #[test]
    fn sequential_identical_commands_without_a_mapping_remain_separate() {
        let entries = request_entries(&[
            decision("pre", "PreToolUse", &json!("acme-call"), "ask"),
            decision("permission", "PermissionRequest", &Value::Null, "deny"),
        ])
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries["pre"]["denied"], false);
        assert_eq!(entries["permission"]["denied"], true);
    }

    #[test]
    fn retained_request_links_join_decisions_grants_and_completions() {
        let pre = decision("request", "PreToolUse", &json!("acme-call"), "ask");
        let permission = decision("request", "PermissionRequest", &Value::Null, "allow");
        let grant = json!({"kind":"run_grant_issued","links":{"request_id":"request"}});
        let completion = json!({"kind":"completion","links":{"request_id":"request"},
            "details":{"status":"completed","exit_code":0}});
        let entries =
            request_entries(&[pre.clone(), permission, grant, completion.clone()]).unwrap();
        assert_eq!(entries.len(), 1);
        let entry = &entries["request"];
        assert_eq!(entry["observations"].as_array().unwrap().len(), 4);
        assert_eq!(entry["evaluation"], pre["details"]["evaluation"]);
        assert_eq!(entry["approved"], true);
        assert_eq!(entry["denied"], false);
        assert_eq!(entry["outcome"], completion["details"]);
    }

    #[test]
    fn revoking_a_grant_stays_approved_while_rejecting_a_pending_request_denies() {
        for owner in ["read", "run"] {
            let ask = |id| decision(id, "PreToolUse", &json!(id), "ask");
            let event = |kind: &str, id: &str, approval: Value| {
                json!({"kind":format!("{owner}_grant_{kind}"),
                    "links":{"request_id":id,"approval_id":approval}})
            };
            let entries = request_entries(&[
                ask("granted"),
                event("issued", "granted", json!("approval-acme")),
                event("revoked", "granted", json!("approval-acme")),
                ask("rejected"),
                event("revoked", "rejected", Value::Null),
            ])
            .unwrap();
            assert_eq!(entries["granted"]["approved"], true, "{owner}");
            assert_eq!(entries["granted"]["denied"], false, "{owner}");
            assert_eq!(entries["rejected"]["approved"], false, "{owner}");
            assert_eq!(entries["rejected"]["denied"], true, "{owner}");
        }
    }

    #[test]
    fn bounded_entry_preserves_the_original_decision_and_latest_observed_outcome() {
        let mut row = json!({"entry_id":"acme-request","evaluation":{"decision":"ask","revision":"acme","matches":"x".repeat(9000)},"request":{"original_input":"x".repeat(9000)},"command":"acme read","outcome":{"status":"failed","exit_code":7,"evidence":"observed","command":"x".repeat(9000)},"provenance":{"agent":"acme"},"observations":[{"details":"x".repeat(9000)},{"details":"x".repeat(9000)}]});
        bound_entry(&mut row);
        assert!(row.to_string().len() <= 8192);
        assert_eq!(row["evaluation"]["decision"], "ask");
        assert_eq!(row["outcome"]["status"], "failed");
        assert_eq!(row["outcome"]["exit_code"], 7);
        assert_eq!(row["truncated"], true);
        assert!(
            row["missing"]
                .as_array()
                .unwrap()
                .contains(&json!("complete_input"))
        );
    }
}
