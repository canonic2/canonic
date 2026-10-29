//! Reader-owned completion correlation and delivery deduplication over one
//! captured journal view. Callbacks store only the reported request ID; a link
//! exists when an earlier decision, still retained when the completion arrived,
//! matches the completion's binding evidence exactly.
use crate::review::model::retained_events;
use serde_json::{Value, json};
use std::collections::HashMap;

/// Provenance fields a completion must share with its decision.
const BINDING: [&str; 5] = ["surface", "agent", "session_id", "tool_use_id", "cwd"];
/// Truncation markers that remove operation or provenance evidence.
const UNBOUND: [&str; 3] = ["complete_input", "detail_fields", "attribution_fields"];

#[derive(Default)]
struct Link {
    request: Option<String>,
    decision: Value,
    duplicate: bool,
    conflict: bool,
}

/// Whether an event still holds the evidence a link compares.
fn bound(event: &Value) -> bool {
    event["truncated"] != true
        || !event["details"]["missing"].as_array().is_some_and(|m| {
            m.iter()
                .any(|f| f.as_str().is_some_and(|f| UNBOUND.contains(&f)))
        })
}

/// A Claude completion can follow a host permission prompt. This links observed
/// execution to the original Ask without inventing an operator approval record.
fn linkable(event: &Value) -> bool {
    event["details"]["host_decision"] == "allow"
        || (event["provenance"]["surface"] == "hook"
            && event["provenance"]["agent"] == "claude-code"
            && event["provenance"]["host_event_name"] == "PreToolUse"
            && event["details"]["host_decision"] == "ask"
            && event["details"]["disposition"] == "approval_required")
}

/// Native callbacks do not carry Shield receipts. Bind their host call identity
/// and exact operation instead; missing identity must never collapse calls.
fn native_binding(event: &Value) -> Option<String> {
    let provenance = &event["provenance"];
    if provenance["surface"] != "hook" || !bound(event) {
        return None;
    }
    for field in ["agent", "session_id", "tool_use_id", "cwd"] {
        if provenance[field].as_str()?.is_empty() {
            return None;
        }
    }
    let operation = &event["details"]["operation"];
    if operation.is_null() {
        return None;
    }
    Some(
        json!([
            provenance["project_id"],
            provenance["project_root"],
            BINDING.map(|field| &provenance[field]),
            operation
        ])
        .to_string(),
    )
}

fn links(events: &[Value], limit: usize) -> Vec<Link> {
    // First-seen ordinal of each decision request, as retention ranks them.
    let mut ordinals: HashMap<&str, usize> = HashMap::new();
    let mut decisions: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut native_calls: HashMap<String, Vec<usize>> = HashMap::new();
    let mut deliveries: HashMap<(&str, String), (usize, &Value)> = HashMap::new();
    let mut links: Vec<Link> = Vec::with_capacity(events.len());
    for (index, event) in events.iter().enumerate() {
        let mut link = Link::default();
        match event["kind"].as_str() {
            Some("decision") => {
                if let Some(id) = event["links"]["request_id"].as_str() {
                    let next = ordinals.len();
                    ordinals.entry(id).or_insert(next);
                    decisions.entry(id).or_default().push(index);
                    if event["provenance"]["host_event_name"] == "PreToolUse"
                        && linkable(event)
                        && let Some(binding) = native_binding(event)
                    {
                        native_calls.entry(binding).or_default().push(index);
                    }
                }
            }
            Some("completion") => {
                // One namespaced delivery is one observation; changed content
                // under it is conflicting evidence, not a later winner.
                if let Some(delivery) = event["details"]["delivery_id"].as_str() {
                    let key = (delivery, event["provenance"].to_string());
                    let digest = &event["details"]["input_digest"];
                    if let Some(&(first, seen)) = deliveries.get(&key) {
                        if seen == digest {
                            link.duplicate = true;
                        } else {
                            link.conflict = true;
                            links[first].conflict = true;
                        }
                        links.push(link);
                        continue;
                    }
                    deliveries.insert(key, (index, digest));
                }
                if let Some((request, decision)) =
                    matching(events, &ordinals, &decisions, &native_calls, event, limit)
                {
                    link.request = Some(request.into());
                    link.decision = decision.clone();
                }
            }
            _ => {}
        }
        links.push(link);
    }
    links
}

fn matching<'a>(
    events: &'a [Value],
    ordinals: &HashMap<&str, usize>,
    decisions: &HashMap<&str, Vec<usize>>,
    native_calls: &HashMap<String, Vec<usize>>,
    completion: &Value,
    limit: usize,
) -> Option<(&'a str, &'a Value)> {
    let provenance = &completion["provenance"];
    let request = if let Some(request) = completion["details"]["reported_request_id"].as_str() {
        request
    } else {
        let binding = native_binding(completion)?;
        let mut candidates = native_calls.get(&binding)?.iter().filter_map(|index| {
            let decision = &events[*index];
            let request = decision["links"]["request_id"].as_str()?;
            (ordinals.len() - ordinals.get(request)? <= limit).then_some(request)
        });
        let request = candidates.next()?;
        // Reused host IDs with distinct retained requests are ambiguous.
        if candidates.any(|other| other != request) {
            return None;
        }
        request
    };
    if (provenance["surface"] == "hook" && provenance["tool_use_id"].is_null())
        || !bound(completion)
    {
        return None;
    }
    // A completion arriving after its command left retention stays unlinked.
    if ordinals.len() - ordinals.get(request)? > limit {
        return None;
    }
    let decision = decisions
        .get(request)?
        .iter()
        .map(|i| &events[*i])
        .find(|d| {
            bound(d)
                && linkable(d)
                && d["details"]["operation"] == completion["details"]["operation"]
                && BINDING
                    .iter()
                    .all(|key| d["provenance"][*key] == provenance[*key])
        })?;
    Some((
        decision["links"]["request_id"].as_str()?,
        &decision["event_id"],
    ))
}

fn set(event: &mut Value, object: &str, field: &str, value: Value) {
    if let Some(fields) = event.get_mut(object).and_then(Value::as_object_mut) {
        fields.insert(field.into(), value);
    }
}

/// Events as readers show them: completions carry reader-derived links, exact
/// duplicate deliveries are dropped and conflicting deliveries are marked.
pub(super) fn view(events: Vec<Value>, limit: usize) -> Vec<Value> {
    let links = links(&events, limit);
    events
        .into_iter()
        .zip(links)
        .filter(|(_, link)| !link.duplicate)
        .map(|(mut event, link)| {
            if event["kind"] == "completion" {
                // Stored links are never trusted; only the reader derives them.
                set(&mut event, "links", "request_id", json!(link.request));
                set(&mut event, "links", "decision_event_id", link.decision);
                if link.conflict {
                    set(&mut event, "details", "delivery_conflict", json!(true));
                }
            }
            event
        })
        .collect()
}

/// Raw records a retention rewrite keeps. Retention follows the reader's links,
/// while stored completions keep only their reported request IDs.
pub(super) fn retain_raw(events: Vec<Value>, limit: usize) -> Vec<Value> {
    let keep = {
        let links = links(&events, limit);
        let fields: Vec<_> = events
            .iter()
            .zip(&links)
            .map(|(event, link)| {
                let kind = event["kind"].as_str().unwrap_or_default();
                let request = if kind == "completion" {
                    link.request.as_deref()
                } else {
                    event["links"]["request_id"].as_str()
                };
                (kind, request)
            })
            .collect();
        retained_events(&fields, limit)
            .into_iter()
            .zip(&links)
            .map(|(kept, link)| kept && !link.duplicate)
            .collect::<Vec<_>>()
    };
    events
        .into_iter()
        .zip(keep)
        .filter_map(|(event, kept)| kept.then_some(event))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(request: &str) -> Value {
        json!({"kind":"decision", "event_id":format!("event-{request}"),
            "links":{"request_id":request},
            "provenance":{"project_id":"acme-project", "project_root":"/acme",
                "surface":"hook", "agent":"codex", "session_id":"acme-session",
                "tool_use_id":"acme-call", "cwd":"/acme", "host_event_name":"PreToolUse"},
            "details":{"host_decision":"allow", "operation":{"kind":"shell", "source":"git status"}}})
    }

    fn completion() -> Value {
        let mut event = decision("unused");
        event["kind"] = json!("completion");
        event["links"] = json!({});
        event["provenance"]["host_event_name"] = json!("PostToolUse");
        event["details"] = json!({"reported_request_id":null,
            "operation":event["details"]["operation"], "status":"unknown"});
        event
    }

    #[test]
    fn only_claude_native_prompt_decisions_accept_ask_completions() {
        for request in [Value::Null, json!("original")] {
            for variant in ["prompt", "blocked", "codex", "sdk", "denied"] {
                let mut decided = decision("original");
                let mut completed = completion();
                decided["provenance"]["agent"] = json!("claude-code");
                decided["details"]["host_decision"] = json!("ask");
                decided["details"]["disposition"] = json!("approval_required");
                match variant {
                    "blocked" => decided["details"]["disposition"] = json!("blocked"),
                    "codex" => decided["provenance"]["agent"] = json!("codex"),
                    "sdk" => decided["provenance"]["host_event_name"] = json!("Before"),
                    "denied" => decided["details"]["host_decision"] = json!("deny"),
                    _ => {}
                }
                completed["provenance"] = decided["provenance"].clone();
                completed["provenance"]["host_event_name"] = json!("PostToolUse");
                completed["details"]["reported_request_id"] = request.clone();
                let result = view(vec![decided, completed], 5000);
                assert_eq!(
                    result[1]["links"]["request_id"] == "original",
                    variant == "prompt",
                    "{variant}: {request}"
                );
            }
        }
    }

    #[test]
    fn native_completion_without_receipt_links_only_to_its_exact_host_call() {
        let decided = decision("original");
        let mut permission = decided.clone();
        permission["links"]["request_id"] = json!("permission");
        permission["provenance"]["host_event_name"] = json!("PermissionRequest");
        permission["provenance"]["tool_use_id"] = Value::Null;
        let completed = completion();
        let events = vec![decided, permission, completed];
        let result = view(events.clone(), 5000);
        assert_eq!(result[2]["links"]["request_id"], "original");
        assert_eq!(result[2]["links"]["decision_event_id"], "event-original");
        assert_eq!(result[2]["details"]["status"], "unknown");
        assert!(result[2]["details"]["reported_request_id"].is_null());
        // Retention rewrites preserve raw records, not synthesized receipts.
        assert_eq!(retain_raw(events.clone(), 5000), events);
    }

    #[test]
    fn native_completion_requires_complete_matching_identity_and_operation() {
        for field in [
            "project_id",
            "project_root",
            "surface",
            "agent",
            "session_id",
            "tool_use_id",
            "cwd",
        ] {
            for replacement in [Value::Null, json!("other")] {
                let mut completed = completion();
                completed["provenance"][field] = replacement;
                let result = view(vec![decision("original"), completed], 5000);
                assert!(result[1]["links"]["request_id"].is_null(), "{field}");
            }
        }
        for field in ["session_id", "tool_use_id"] {
            let mut decided = decision("original");
            let mut completed = completion();
            decided["provenance"][field] = Value::Null;
            completed["provenance"][field] = Value::Null;
            assert!(view(vec![decided, completed], 5000)[1]["links"]["request_id"].is_null());
        }
        let mut completed = completion();
        completed["details"]["operation"]["source"] = json!("git diff");
        assert!(
            view(vec![decision("original"), completed], 5000)[1]["links"]["request_id"].is_null()
        );
    }

    #[test]
    fn native_completion_rejects_ambiguous_denied_late_and_truncated_decisions() {
        let result = view(
            vec![decision("first"), decision("second"), completion()],
            5000,
        );
        assert!(result[2]["links"]["request_id"].is_null());
        for change in ["deny", "truncated", "late", "permission"] {
            let mut decided = decision("original");
            match change {
                "deny" => decided["details"]["host_decision"] = json!("deny"),
                "truncated" => {
                    decided["truncated"] = json!(true);
                    decided["details"]["missing"] = json!(["complete_input"]);
                }
                "permission" => {
                    decided["provenance"]["host_event_name"] = json!("PermissionRequest");
                }
                _ => {}
            }
            let mut other = decision("other");
            other["provenance"]["tool_use_id"] = json!("other-call");
            let limit = if change == "late" { 1 } else { 5000 };
            let result = view(vec![decided, other, completion()], limit);
            assert!(result[2]["links"]["request_id"].is_null(), "{change}");
        }
        assert!(
            view(vec![completion(), decision("later")], 5000)[0]["links"]["request_id"].is_null()
        );
    }

    #[test]
    fn explicit_receipt_never_falls_back_to_a_different_native_request() {
        let mut completed = completion();
        completed["details"]["reported_request_id"] = json!("missing");
        assert!(
            view(vec![decision("original"), completed], 5000)[1]["links"]["request_id"].is_null()
        );
    }
}
