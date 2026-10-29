//! Review completeness, recurrence, projection and non-authorizing suppression.
use super::*;

fn record(context: &Context, id: Option<&str>, escalated: bool, partial: bool) {
    let mut hook = context.clone();
    hook.surface = "hook".into();
    hook.host_event_name = Some("PreToolUse".into());
    hook.session_id = Some("acme-session".into());
    hook.tool_use_id = id.map(str::to_owned);
    let source = if partial {
        "node --test --watch"
    } else {
        "curl http://localhost:3000/health"
    };
    let guidance = if partial {
        json!([{"message":"Tests execute project code.","rule_ref":{"scope":"pack","rule_id":"node-test"}}])
    } else {
        json!([{"source":"decisive","message":"Local fetch asks.","request":{"executable":"curl","arguments":["http://localhost:3000/health"]}}])
    };
    review::observations::decision(&hook, json!({"decision":"ask",
        "operation":{"kind":"shell","source":source},
        "original_input":{"sandbox_permissions":if escalated {"require_escalated"}else{"use_default"}},
        "evaluation":{"coverage":if partial {"partial"}else{"single-literal-invocation"},"explanation":{"guidance":guidance}}})).unwrap();
}

/// Codex's escalation shape: hook input carries only the command, and the
/// escalated attempt is followed by a `PermissionRequest` without a tool ID.
fn record_codex(context: &Context, event: &str, id: Option<&str>) {
    let mut hook = context.clone();
    hook.surface = "hook".into();
    hook.agent = Some("codex".into());
    hook.host_event_name = Some(event.into());
    hook.session_id = Some("acme-session".into());
    hook.tool_use_id = id.map(str::to_owned);
    review::observations::decision(&hook, json!({"decision":"ask",
        "operation":{"kind":"shell","source":"curl http://localhost:3000/health"},
        "original_input":{"command":"curl http://localhost:3000/health"},
        "evaluation":{"coverage":"single-literal-invocation","explanation":{"guidance":[{"source":"decisive","message":"Local fetch asks.","request":{"executable":"curl","arguments":["http://localhost:3000/health"]}}]}}})).unwrap();
}

fn listed(context: &Context, connection: &Connection) -> Value {
    invoke(
        context,
        connection,
        "suggestion_list",
        json!({"session_id":"acme-session"}),
    )
}

#[test]
fn suggestions_partial_watcher_is_visible_without_an_invented_rule() {
    let (_root, context, connection) = fixture();
    record(&context, Some("watch"), false, true);
    let result = listed(&context, &connection);
    assert!(result["error"].is_null(), "{result}");
    let row = &result["data"]["requests"][0];
    assert_eq!(row["command"], "node --test --watch");
    assert_eq!(row["review_status"], "unsupported");
    assert!(row.get("request").is_none());
    assert!(row.get("suggested_change").is_none());
    assert_eq!(row["next_action"]["tool"], "history_list");
    assert_eq!(row["eligible_for_offer"], false);
}

#[test]
fn suggestions_recurrence_excludes_duplicates_escalation_and_missing_identity() {
    let (_root, context, connection) = fixture();
    record(&context, Some("first"), false, false);
    record(&context, Some("first"), false, false);
    record(&context, Some("retry"), true, false);
    let first = listed(&context, &connection);
    assert_eq!(first["data"]["requests"][0]["intentional_invocations"], 1);
    assert_eq!(first["data"]["requests"][0]["eligible_for_offer"], false);
    record(&context, Some("second"), false, false);
    let second = listed(&context, &connection);
    assert_eq!(second["data"]["requests"][0]["intentional_invocations"], 2);
    assert_eq!(second["data"]["requests"][0]["eligible_for_offer"], true);
    record(&context, None, false, false);
    assert_eq!(
        listed(&context, &connection)["data"]["requests"][0]["eligible_for_offer"],
        false
    );
}

#[test]
fn suggestions_count_a_codex_escalation_retry_once_without_its_sandbox_field() {
    let (_root, context, connection) = fixture();
    record_codex(&context, "PreToolUse", Some("exec-sandboxed"));
    record_codex(&context, "PreToolUse", Some("exec-escalated"));
    record_codex(&context, "PermissionRequest", None);
    let retried = listed(&context, &connection);
    assert_eq!(retried["data"]["requests"][0]["intentional_invocations"], 1);
    assert_eq!(retried["data"]["requests"][0]["eligible_for_offer"], false);
    // A later, separate request is a second deliberate invocation.
    record_codex(&context, "PreToolUse", Some("exec-later"));
    let recurring = listed(&context, &connection);
    assert_eq!(
        recurring["data"]["requests"][0]["intentional_invocations"],
        2
    );
    assert_eq!(recurring["data"]["requests"][0]["eligible_for_offer"], true);
}

#[test]
fn suggestions_count_each_request_escalated_on_its_first_attempt() {
    let (_root, context, connection) = fixture();
    // Network access needs escalation, so every deliberate request is escalated.
    record_codex(&context, "PreToolUse", Some("exec-first"));
    record_codex(&context, "PermissionRequest", None);
    let first = listed(&context, &connection);
    assert_eq!(first["data"]["requests"][0]["intentional_invocations"], 1);
    assert_eq!(first["data"]["requests"][0]["eligible_for_offer"], false);
    record_codex(&context, "PreToolUse", Some("exec-second"));
    record_codex(&context, "PermissionRequest", None);
    let second = listed(&context, &connection);
    assert_eq!(second["data"]["requests"][0]["intentional_invocations"], 2);
    assert_eq!(second["data"]["requests"][0]["eligible_for_offer"], true);
}

#[test]
fn suggestions_deferral_survives_reconnect_and_cannot_change_policy() {
    let (root, context, connection) = fixture();
    record(&context, Some("first"), false, false);
    record(&context, Some("second"), false, false);
    let before = Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap()
        .policy_revision;
    let result = listed(&context, &connection);
    let id = result["data"]["requests"][0]["review_id"].clone();
    let deferred = invoke(
        &context,
        &connection,
        "review_defer",
        json!({"session_id":"acme-session","review_ids":[id]}),
    );
    assert!(deferred["error"].is_null(), "{deferred}");
    let reconnect = Connection::new("reconnected".into());
    let result = listed(&context, &reconnect);
    assert_eq!(result["data"]["requests"][0]["review_status"], "deferred");
    assert_eq!(result["data"]["requests"][0]["eligible_for_offer"], false);
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .policy_revision,
        before
    );
    let foreign = invoke(
        &context,
        &connection,
        "review_defer",
        json!({"session_id":"other","review_ids":[id]}),
    );
    assert_eq!(foreign["error"]["code"], "invalid_input");
}

#[test]
fn suggestions_structured_projection_contains_action_examples_and_pending_recovery() {
    let (_root, context, connection) = fixture();
    record(&context, Some("first"), false, false);
    record(&context, Some("second"), false, false);
    let result = listed(&context, &connection);
    let data = &result["data"];
    assert!(data["coverage"]["available"].as_bool().unwrap());
    assert!(data["authoring_examples"][0]["change"]["rule"].is_object());
    assert!(data["approval_guidance"].is_string());
    let row = &data["requests"][0];
    let mut input = row["next_action"]["proposal_context"].clone();
    input["change"] = row["suggested_change"].clone();
    let proposed = invoke(&context, &connection, "policy_propose", input);
    assert!(proposed["error"].is_null(), "{proposed}");
    let next = listed(&context, &connection);
    assert_eq!(next["data"]["requests"][0]["review_status"], "proposed");
    assert_eq!(next["data"]["requests"][0]["eligible_for_offer"], false);
    assert_eq!(
        next["data"]["requests"][0]["next_action"]["arguments"]["proposal_id"],
        proposed["data"]["proposal_id"]
    );
}

#[test]
fn suggestions_disabled_history_reports_unavailable_evidence() {
    let (root, context, connection) = fixture();
    Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.history_enabled = false;
            Ok(json!({}))
        })
        .unwrap();
    let result = listed(&context, &connection);
    assert_eq!(result["data"]["coverage"]["available"], false);
    assert_eq!(result["data"]["coverage"]["complete"], false);
}

#[test]
fn suggestions_wire_projection_keeps_examples_actions_and_read_only_listing() {
    for agent in ["codex", "claude-code"] {
        let (_root, mut context, _connection) = fixture();
        context.agent = Some(agent.into());
        record(&context, Some("first"), false, false);
        record(&context, Some("second"), false, false);
        let transport =
            crate::mcp::tests::Transport::start(crate::mcp::Server::for_review(context).unwrap());
        let [initialize, ready] = crate::mcp::tests::initialize(&json!({}), agent);
        assert!(transport.request(&initialize)["error"].is_null());
        transport.send(&ready);
        let catalog =
            transport.request(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
        let tools = catalog["result"]["tools"].as_array().unwrap();
        let suggestion = tools
            .iter()
            .find(|tool| tool["name"] == "suggestion_list")
            .unwrap();
        assert_eq!(suggestion["annotations"]["readOnlyHint"], true);
        let deferred = tools
            .iter()
            .find(|tool| tool["name"] == "review_defer")
            .unwrap();
        assert_eq!(deferred["annotations"]["readOnlyHint"], false);
        let response = transport.call(
            json!(3),
            "suggestion_list",
            &json!({"session_id":"acme-session"}),
        );
        assert!(response["error"].is_null(), "{response}");
        let structured = &response["result"]["structuredContent"]["data"];
        let text: Value =
            serde_json::from_str(response["result"]["content"][1]["text"].as_str().unwrap())
                .unwrap();
        for key in [
            "coverage",
            "requests",
            "authoring_examples",
            "approval_guidance",
        ] {
            assert_eq!(text["data"][key], structured[key], "{key}");
        }
        assert_eq!(structured["requests"][0]["eligible_for_offer"], true);
        assert!(
            structured["requests"][0]["next_action"]["proposal_context"]["review_ids"].is_array()
        );
        transport.close().1.unwrap();
    }
}

#[test]
fn a_full_deferral_store_refuses_with_capacity_guidance_instead_of_reconnecting() {
    let (root, context, connection) = fixture();
    record(&context, Some("first"), false, false);
    let id = listed(&context, &connection)["data"]["requests"][0]["review_id"].clone();
    Store::open(root.path())
        .unwrap()
        .update_checkpoint(&context, &|| Ok(()), |state| {
            state.dismissed_reviews = (0..1024).map(|index| format!("{index:064x}")).collect();
            Ok(json!({}))
        })
        .unwrap();
    let refused = invoke(
        &context,
        &connection,
        "review_defer",
        json!({"session_id":"acme-session","review_ids":[id]}),
    );
    let error = &refused["error"];
    assert_eq!(error["code"], "capacity_reached", "{refused}");
    assert!(error["operator_route"].is_null(), "{refused}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("do not retry review_defer"), "{message}");
    assert!(!message.contains("reconnect"), "{message}");
}

#[test]
fn suggestions_history_clear_preserves_explicit_deferral() {
    let (root, context, connection) = fixture();
    record(&context, Some("first"), false, false);
    record(&context, Some("second"), false, false);
    let first = listed(&context, &connection);
    let id = first["data"]["requests"][0]["review_id"].clone();
    let deferred = invoke(
        &context,
        &connection,
        "review_defer",
        json!({"session_id":"acme-session","review_ids":[id]}),
    );
    assert!(deferred["error"].is_null());
    crate::review::history_settings(&context, None, None, true, &|| Ok(())).unwrap();
    record(&context, Some("third"), false, false);
    record(&context, Some("fourth"), false, false);
    let next = listed(&context, &connection);
    assert_eq!(next["data"]["requests"][0]["review_id"], id);
    assert_eq!(next["data"]["requests"][0]["review_status"], "deferred");
    assert_eq!(
        Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .dismissed_reviews
            .len(),
        1
    );
}
