//! Reader-owned completion correlation and delivery deduplication over one
//! captured journal view. Callbacks store only the reported request ID; a link
//! exists when an earlier decision, still retained when the completion arrived,
//! matches the completion's binding evidence exactly.
use crate::review::model::retained_events;
use serde_json::{Value, json};
use std::{borrow::Cow, collections::HashMap};

/// Provenance fields a completion must share with its decision. The working
/// folder is recorded relative to the project, so a moved project keeps links.
const BINDING: [&str; 6] = [
    "project_id",
    "surface",
    "agent",
    "session_id",
    "tool_use_id",
    "cwd",
];
/// Truncation markers that remove operation or provenance evidence.
const UNBOUND: [&str; 3] = ["complete_input", "detail_fields", "attribution_fields"];

#[derive(Default)]
struct Link {
    request: Option<String>,
    decision: Option<usize>,
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
            && event["provenance"]["agent"] == "claude"
            && event["provenance"]["host_event_name"] == "PreToolUse"
            && event["details"]["host_decision"] == "ask"
            && event["details"]["disposition"] == "approval_required")
}

fn native_before(event: &Value) -> bool {
    let provenance = &event["provenance"];
    let Some(name) = provenance["host_event_name"].as_str() else {
        return false;
    };
    name == "PreToolUse"
        || provenance["agent"]
            .as_str()
            .and_then(crate::agents::find)
            .is_some_and(|adapter| adapter.event(name) == Some(crate::agents::Event::Before))
}

/// Native callbacks do not carry Shield receipts. Bind their host call identity
/// and exact operation instead; missing identity must never collapse calls.
fn native_binding(event: &Value) -> Option<String> {
    let provenance = &event["provenance"];
    if provenance["surface"] != "hook" || !bound(event) {
        return None;
    }
    for field in BINDING {
        if provenance[field].as_str()?.is_empty() {
            return None;
        }
    }
    let operation = operation_identity(event)?;
    Some(json!([BINDING.map(|field| &provenance[field]), operation]).to_string())
}

fn operation_identity(event: &Value) -> Option<Cow<'_, Value>> {
    let details = &event["details"];
    let operation = &details["operation"];
    if operation.is_null() {
        return None;
    }
    if operation["kind"] != "mcp" {
        return Some(Cow::Borrowed(operation));
    }
    if let Some(digest) = details.get("mcp_operation_digest") {
        digest
            .as_str()
            .filter(|digest| canonic_registry_kit::digest::is_sha256_hex(digest))?;
        return Some(Cow::Borrowed(digest));
    }
    // Older records can bind only when they still hold the argument object.
    // In particular, all masked SQL arguments have the same string value.
    if !operation["arguments"].is_object() {
        return None;
    }
    crate::json::fingerprint(operation)
        .ok()
        .map(|digest| Cow::Owned(json!(digest)))
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
                    if native_before(event)
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
                    link.decision = Some(decision);
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
) -> Option<(&'a str, usize)> {
    let provenance = &completion["provenance"];
    if provenance["project_id"].as_str().is_none_or(str::is_empty) {
        return None;
    }
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
    let operation = operation_identity(completion)?;
    let index = decisions.get(request)?.iter().copied().find(|&index| {
        let decision = &events[index];
        bound(decision)
            && linkable(decision)
            && BINDING
                .iter()
                .all(|key| decision["provenance"][*key] == provenance[*key])
            && operation_identity(decision).as_ref() == Some(&operation)
    })?;
    Some((events[index]["links"]["request_id"].as_str()?, index))
}

fn set(event: &mut Value, object: &str, field: &str, value: Value) {
    if let Some(fields) = event.get_mut(object).and_then(Value::as_object_mut) {
        fields.insert(field.into(), value);
    }
}

fn project(event: &mut Value, link: &Link, decision: Option<&Value>) {
    if event["kind"] == "completion" {
        // Stored links are never trusted; only the reader derives them.
        set(event, "links", "request_id", json!(link.request));
        let decision_id = decision.map_or(Value::Null, |decision| decision["event_id"].clone());
        set(event, "links", "decision_event_id", decision_id);
        if event["details"]["operation"]["kind"] == "mcp"
            && event["details"]["operation"]["arguments"] == "[REDACTED]"
            && event["details"]["mcp_operation_digest"].is_string()
            && let Some(decision) = decision
        {
            let operation = decision["details"]["operation"].clone();
            set(event, "details", "operation", operation);
        }
        if link.conflict {
            set(event, "details", "delivery_conflict", json!(true));
        }
    }
    if let Some(details) = event.get_mut("details").and_then(Value::as_object_mut) {
        details.remove("mcp_operation_digest");
        if details
            .get("operation")
            .is_some_and(|operation| operation["kind"] == "mcp")
        {
            details.remove("input_digest");
        }
    }
}

/// Bound the serialized uncorrelated array, including delimiters. Linked
/// command evidence never competes for this separate presentation allowance.
fn uncorrelated_mask(events: &[Value], links: &[Link]) -> Vec<bool> {
    let mut keep = vec![true; events.len()];
    let mut count = 0;
    let mut bytes = 2usize;
    for (index, (event, link)) in events.iter().zip(links).enumerate().rev() {
        if event["kind"] != "completion" || link.request.is_some() || link.duplicate {
            continue;
        }
        let mut projected = event.clone();
        project(&mut projected, link, None);
        let size = crate::json::encoded_len(&projected).unwrap_or(usize::MAX);
        let next = bytes
            .saturating_add(size)
            .saturating_add(usize::from(count > 0));
        if count >= 128 || next > 1024 * 1024 {
            keep[index] = false;
        } else {
            count += 1;
            bytes = next;
        }
    }
    keep
}

/// Events as readers show them: completions carry reader-derived links, exact
/// duplicate deliveries are dropped and conflicting deliveries are marked.
pub(super) fn view(mut events: Vec<Value>, limit: usize) -> Vec<Value> {
    let links = links(&events, limit);
    let keep = uncorrelated_mask(&events, &links);
    for (index, (link, keep)) in links.iter().zip(&keep).enumerate() {
        if *keep && !link.duplicate {
            let (earlier, current) = events.split_at_mut(index);
            project(
                &mut current[0],
                link,
                link.decision.map(|index| &earlier[index]),
            );
        }
    }
    events
        .into_iter()
        .zip(links)
        .zip(keep)
        .filter(|((_, link), keep)| *keep && !link.duplicate)
        .map(|((event, _), _)| event)
        .collect()
}

/// Raw records a retention rewrite keeps. Retention follows the reader's links,
/// while stored completions keep only their reported request IDs.
pub(super) fn retain_raw(events: Vec<Value>, limit: usize) -> Vec<Value> {
    let keep = {
        let links = links(&events, limit);
        let uncorrelated = uncorrelated_mask(&events, &links);
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
            .zip(uncorrelated)
            .map(|((kept, link), uncorrelated)| kept && !link.duplicate && uncorrelated)
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
            "provenance":{"project_id":"acme-project",
                "surface":"hook", "agent":"codex", "session_id":"acme-session",
                "tool_use_id":"acme-call", "cwd":".", "host_event_name":"PreToolUse"},
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
    fn reported_completions_require_same_checkout_provenance() {
        for surface in ["hook", "sdk"] {
            let mut decided = decision("original");
            let mut completed = completion();
            decided["provenance"]["surface"] = json!(surface);
            completed["provenance"]["surface"] = json!(surface);
            completed["details"]["reported_request_id"] = json!("original");
            assert_eq!(
                view(vec![decided.clone(), completed.clone()], 5000)[1]["links"]["request_id"],
                "original"
            );
            for field in ["project_id"] {
                for replacement in [Value::Null, json!(""), json!("another-checkout")] {
                    let mut foreign = completed.clone();
                    foreign["provenance"][field] = replacement.clone();
                    let result = view(vec![decided.clone(), foreign.clone()], 5000);
                    assert!(
                        result[1]["links"]["request_id"].is_null(),
                        "{surface}: {field}={replacement}"
                    );
                    assert!(result[1]["links"]["decision_event_id"].is_null());
                    assert_eq!(result[1]["details"]["reported_request_id"], "original");
                    if replacement.is_null() || replacement == "" {
                        let mut missing = decided.clone();
                        missing["provenance"][field] = replacement;
                        assert!(
                            view(vec![missing, foreign], 5000)[1]["links"]["request_id"].is_null()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn uncorrelated_view_and_rewrite_bound_bytes_without_pruning_linked_evidence() {
        let mut events = vec![decision("retained"), completion()];
        for index in 0..128 {
            let mut event = completion();
            event["event_id"] = json!(format!("unlinked-{index}"));
            event["details"]["reported_request_id"] = json!("missing");
            event["details"]["padding"] = json!("x".repeat(8192));
            events.push(event);
        }
        let result = view(events.clone(), 5000);
        assert_eq!(result[1]["links"]["request_id"], "retained");
        let unlinked: Vec<_> = result
            .iter()
            .filter(|event| event["kind"] == "completion" && event["links"]["request_id"].is_null())
            .cloned()
            .collect();
        assert!(unlinked.len() < 128);
        assert!(crate::json::encoded_len(&json!(unlinked)).unwrap() <= 1024 * 1024);
        assert_eq!(result.last().unwrap()["event_id"], "unlinked-127");
        assert!(!result.iter().any(|event| event["event_id"] == "unlinked-0"));
        let retained = retain_raw(events, 5000);
        assert_eq!(view(retained.clone(), 5000), result);
        assert!(retained[1]["links"].as_object().unwrap().is_empty());
    }

    #[test]
    fn only_claude_native_prompt_decisions_accept_ask_completions() {
        for request in [Value::Null, json!("original")] {
            for variant in ["prompt", "blocked", "codex", "sdk", "denied"] {
                let mut decided = decision("original");
                let mut completed = completion();
                decided["provenance"]["agent"] = json!("claude");
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
    fn legacy_mcp_completion_requires_retained_arguments_or_a_valid_digest() {
        for reported in [Value::Null, json!("original")] {
            for (tool, arguments, digest, linked) in [
                ("execute_sql", json!("[REDACTED]"), None, false),
                (
                    "execute_sql",
                    json!("[REDACTED]"),
                    Some("bad-digest"),
                    false,
                ),
                ("inspect", json!({"query":"acme"}), None, true),
            ] {
                let operation =
                    json!({"kind":"mcp","alias":"acme","tool":tool,"arguments":arguments});
                let mut decided = decision("original");
                let mut completed = completion();
                completed["details"]["reported_request_id"] = reported.clone();
                for event in [&mut decided, &mut completed] {
                    event["details"]["operation"] = operation.clone();
                    if let Some(digest) = digest {
                        event["details"]["mcp_operation_digest"] = json!(digest);
                    }
                }
                let result = view(vec![decided, completed], 5000);
                assert_eq!(
                    result[1]["links"]["decision_event_id"] == "event-original",
                    linked
                );
            }
        }
    }

    #[test]
    fn native_completion_requires_complete_matching_identity_and_operation() {
        for field in [
            "project_id",
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
