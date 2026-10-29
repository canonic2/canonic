//! Verifies complete tool evidence, selector text, limits and interruption.

use super::*;
use canonic_mcp::rpc;
use std::cell::Cell;

fn result(summary: &str, text: &Value, evidence: &Value, failed: bool) -> Value {
    json!({"content":[{"type":"text","text":summary},{"type":"text","text":text.to_string()}],
        "structuredContent":evidence,"isError":failed})
}

fn value(result: &ToolResult) -> Value {
    serde_json::to_value(result).unwrap()
}

fn assert_codex_terminal_fallback(summary: &str) {
    assert!(
        summary.contains("If the approval policy is never"),
        "{summary}"
    );
    assert!(summary.contains("leave the proposal pending"), "{summary}");
    assert!(
        summary.contains("ask the user to activate it with shield policy review in their terminal"),
        "{summary}"
    );
    assert!(
        summary.contains("never bypass that restriction"),
        "{summary}"
    );
}

#[test]
fn host_approval_guidance_matches_agent_without_changing_error_evidence() {
    for (agent, expected, absent) in [
        (
            Some("codex"),
            "Codex tool approval is unavailable",
            "enable MCP form elicitation",
        ),
        (
            Some("claude-code"),
            "enable MCP form elicitation",
            "Codex tool approval is unavailable",
        ),
        (
            None,
            "Host confirmation unavailable",
            "enable MCP form elicitation",
        ),
    ] {
        let evidence = json!({"error":{"code":"approval_unavailable",
            "message":messages::approval_unavailable(agent)}, "data":null});
        let reply = value(&tool_result(
            "policy_apply",
            agent,
            evidence.clone(),
            true,
            &|| Ok(()),
        ));
        let summary = reply["content"][0]["text"].as_str().unwrap();
        assert!(summary.contains(expected), "{summary}");
        assert!(!summary.contains(absent), "{summary}");
        assert!(summary.contains("shield policy review in their terminal"));
        assert_eq!(reply["structuredContent"], evidence);
        assert_eq!(reply["isError"], true);
    }
    // An error without a message still names a recovery.
    let bare = messages::summary(
        "policy_apply",
        Some("codex"),
        &json!({"error":{"code":"acme_unknown"}}),
    );
    assert!(bare.contains("doctor"), "{bare}");
    let proposed = messages::summary(
        "policy_propose",
        Some("codex"),
        &json!({"data":{"status":"pending"}}),
    );
    assert!(
        proposed.contains("Policy proposal saved; inactive"),
        "{proposed}"
    );
    assert!(
        proposed.contains("Copy the returned apply_arguments object unchanged into policy_apply for Codex approval"),
        "{proposed}"
    );
    assert!(
        proposed.contains("Wait before the next proposal"),
        "{proposed}"
    );
    assert_codex_terminal_fallback(&proposed);
}

#[test]
fn results_preserve_complete_evidence_and_escaped_text_for_success_and_error() {
    let matches: Vec<_> = (0..128)
        .map(|id| json!({"rule_id":format!("acme/{id}"),"reason":"Acme \"quoted\"\n東京\\path"}))
        .collect();
    for failed in [false, true] {
        let error = if failed {
            json!({"code":"invalid_input","message":"Acme \"input\"\n"})
        } else {
            Value::Null
        };
        let evidence = json!({"schema_version":1,"project_id":"acme","policy_revision":"revision",
            "data":{"decision":"ask","matches":matches},"error":error,
            "missing":["acme/missing"],"truncated":false,"redacted":true});
        let text = json!({"data":evidence["data"],"error":error,
            "missing":["acme/missing"],"truncated":false});
        // The reason leads the summary, without control characters.
        let summary = if failed {
            "Acme \"input\". Correct the arguments and retry."
        } else {
            "Policy preview: ask. This result does not authorize execution."
        };
        let expected = result(summary, &text, &evidence, failed);
        let reply = tool_result("policy_evaluate", None, evidence, failed, &|| Ok(()));
        assert_eq!(value(&reply), expected);
    }
}

#[test]
fn proposal_text_preserves_present_null_selectors_and_full_structured_evidence() {
    for (name, summary) in [
        (
            "policy_propose",
            "Policy proposal saved; inactive. Ask the user to review and activate it with shield policy review in their terminal.",
        ),
        (
            "policy_withdraw",
            "Policy proposal withdrawn. Active policy is unchanged.",
        ),
    ] {
        let evidence = json!({"data":{"proposal_id":"acme-proposal","status":"pending",
            "policy_revision":null,"before_candidate_id":"acme-before",
            "complete_evidence":{"rules":["acme/full"]}},"error":null,
            "missing":[],"truncated":false});
        let text = json!({"data":{"proposal_id":"acme-proposal","status":"pending",
            "policy_revision":null,"before_candidate_id":"acme-before"},
            "error":null,"missing":[],"truncated":false});
        let expected = result(summary, &text, &evidence, false);
        let reply = tool_result(name, None, evidence, false, &|| Ok(()));
        assert_eq!(value(&reply), expected);
    }
}

#[test]
fn oversized_evidence_cannot_be_encoded_within_the_output_limit() {
    let encodes = |evidence: Value| {
        let reply = tool_result("status", None, evidence, false, &|| Ok(()));
        rpc::encode_success(&json!(1), &reply, MAX_OUTPUT, &|| Ok(())).is_ok()
    };
    assert!(encodes(json!({"data":{"text":"x".repeat(1024)}})));
    assert!(!encodes(json!({"secret":"x".repeat(MAX_OUTPUT)})));
    // The evidence fits alone but not beside its text copy.
    assert!(!encodes(
        json!({"data":{"text":"x".repeat(MAX_OUTPUT / 2 + 1)}})
    ));
}

#[test]
fn a_failed_checkpoint_stops_encoding_and_reports_cancellation() {
    let evidence = json!({"data":(0..128).map(|n| format!("Acme {n}")).collect::<Vec<_>>()});
    let calls = Cell::new(0);
    let checkpoint = || {
        calls.set(calls.get() + 1);
        if calls.get() < 10 {
            Ok(())
        } else {
            Err(Interrupted)
        }
    };
    let reply = value(&tool_result("status", None, evidence, false, &checkpoint));
    assert_eq!(calls.get(), 10);
    assert_eq!(reply["isError"], true);
    assert!(reply.get("structuredContent").is_none());
    assert!(
        reply["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("The request was cancelled.")
    );
}
