//! Policy application through host confirmation, end to end over stdio: no
//! answer other than the host's acceptance activates a proposal.
use super::*;

const FORM: &str = r#"{"elicitation":{"form":{}}}"#;

fn capabilities(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn rule(id: &str, executable: &str) -> Value {
    json!({"change":{"kind":"rule_add","rule":{"id":id,"decision":"allow","conditions":{"domain":"argv","executable":executable,"arguments":{"kind":"any"}}}}})
}

/// A live Claude Code connection with one pending proposal.
fn prepared(agent: Option<&str>, capabilities: &Value) -> (Transport, Project, Value) {
    let (server, project) = fixture(agent);
    let transport = Transport::start(server);
    for message in initialize(capabilities, "claude-code") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    let args = propose(&transport, json!(2), &rule("acme-read", "acme"));
    (transport, project, args)
}

fn propose(transport: &Transport, id: Value, arguments: &Value) -> Value {
    let proposal = transport.call(id, "policy_propose", arguments);
    assert_eq!(proposal["result"]["isError"], false, "{proposal}");
    proposal["result"]["structuredContent"]["data"]["apply_arguments"].clone()
}

/// Commits `args` as a confirmed apply outside the connection.
fn commit_elsewhere(project: &Project, args: &Value) -> Value {
    let applied = crate::review::mcp::call_with_confirmation(
        &project.review,
        &crate::review::mcp::Connection::new("acme-other".into()),
        "policy_apply",
        args,
        &|| Ok(()),
        true,
    );
    assert!(applied["error"].is_null(), "{applied}");
    applied
}

fn answer(form: &Value, result: &Value) -> Value {
    json!({"jsonrpc":"2.0","id":form["id"],"result":result})
}

fn error_code(response: &Value) -> &Value {
    &response["result"]["structuredContent"]["error"]["code"]
}

fn modern_apply(
    id: i64,
    args: &Value,
    state: Option<&Value>,
    action: Option<&str>,
    form: bool,
) -> Value {
    let mut params = json!({"name":"policy_apply","arguments":args});
    if let Some(state) = state {
        params["requestState"] = state.clone();
    }
    if let Some(action) = action {
        params["inputResponses"] = json!({"confirmation":{"action":action}});
    }
    super::skills::modern(
        id,
        "tools/call",
        params,
        if form {
            json!({"elicitation":{"form":{}}})
        } else {
            json!({})
        },
    )
}

#[test]
fn modern_confirmation_accept_decline_cancel_and_receipt_replay() {
    for action in ["accept", "decline", "cancel"] {
        let (transport, project, args) = prepared(Some("claude"), &capabilities(FORM));
        let before = project.revision();
        let interim = transport.request(&modern_apply(10, &args, None, None, true));
        assert_eq!(
            interim["result"]["resultType"], "input_required",
            "{interim}"
        );
        assert!(interim["result"].get("ttlMs").is_none());
        assert_eq!(project.revision(), before);
        // Cancelling a completed interim RPC ID cannot revoke a separate retry.
        transport.send(
            &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10}}),
        );
        let state = &interim["result"]["requestState"];
        let result = transport.request(&modern_apply(11, &args, Some(state), Some(action), true));
        assert_eq!(result["result"]["resultType"], "complete", "{result}");
        assert_eq!(project.revision() != before, action == "accept");
        if action == "accept" {
            assert_eq!(result["result"]["isError"], false, "{result}");
            let receipt = transport.request(&modern_apply(12, &args, None, None, true));
            assert_eq!(receipt["result"]["isError"], false, "{receipt}");
            // The retained receipt is compact; commit also returns change detail.
            for field in [
                "activation_id",
                "candidate_digest",
                "policy_revision",
                "previous_activation_id",
                "committed",
            ] {
                assert_eq!(
                    receipt["result"]["structuredContent"]["data"]["receipt"][field],
                    result["result"]["structuredContent"]["data"]["receipt"][field]
                );
            }
        } else {
            assert_eq!(error_code(&result), "approval_declined");
        }
        let replay = transport.request(&modern_apply(13, &args, Some(state), Some("accept"), true));
        assert_eq!(replay["error"]["code"], rpc::INVALID_PARAMS, "{replay}");
    }
}

#[test]
fn modern_confirmation_rejects_forgery_changed_arguments_and_lost_capability() {
    for variant in ["forged", "arguments", "capability", "missing-response"] {
        let (transport, project, args) = prepared(Some("claude"), &capabilities(FORM));
        let before = project.revision();
        let interim = transport.request(&modern_apply(10, &args, None, None, true));
        let mut state = interim["result"]["requestState"].clone();
        let mut retry_args = args.clone();
        if variant == "forged" {
            state = json!("forged");
        }
        if variant == "arguments" {
            retry_args["candidate_digest"] = json!("different");
        }
        let response = transport.request(&modern_apply(
            11,
            &retry_args,
            Some(&state),
            (variant != "missing-response").then_some("accept"),
            variant != "capability",
        ));
        assert!(response["error"].is_object(), "{variant}: {response}");
        assert_eq!(project.revision(), before);
    }
}

#[test]
fn modern_confirmation_rechecks_policy_and_connection_ownership() {
    let (transport, project, args) = prepared(Some("claude"), &capabilities(FORM));
    let interim = transport.request(&modern_apply(10, &args, None, None, true));
    let state = &interim["result"]["requestState"];
    let (other_server, other_project) = fixture(Some("claude"));
    let other = Transport::start(other_server);
    let before_other = other_project.revision();
    let foreign = other.request(&modern_apply(11, &args, Some(state), Some("accept"), true));
    assert_eq!(foreign["error"]["code"], rpc::INVALID_PARAMS);
    assert_eq!(other_project.revision(), before_other);
    let other_args = propose(&transport, json!(12), &rule("acme-other", "other"));
    commit_elsewhere(&project, &other_args);
    let current = project.revision();
    let expired_response =
        transport.request(&modern_apply(13, &args, Some(state), Some("accept"), true));
    assert_eq!(
        error_code(&expired_response),
        "stale_revision",
        "{expired_response}"
    );
    assert_eq!(project.revision(), current);
}

#[test]
fn modern_requests_preserve_registered_adapter_and_per_request_capabilities() {
    for agent in [Some("claude"), Some("codex"), None] {
        // Legacy form support must not bleed into the following modern request.
        let (transport, project, args) = prepared(agent, &capabilities(FORM));
        let before = project.revision();
        let mut request = modern_apply(10, &args, None, None, false);
        request["params"]["_meta"]["io.modelcontextprotocol/clientInfo"] =
            json!({"name":"codex","version":"1"});
        let result = transport.request(&request);
        assert_eq!(result["result"]["resultType"], "complete", "{result}");
        match agent {
            Some("codex") => assert_eq!(result["result"]["isError"], false, "{result}"),
            Some("claude") => assert_eq!(error_code(&result), "approval_unavailable"),
            _ => assert_eq!(error_code(&result), "operator_review_required"),
        }
        assert_eq!(project.revision() != before, agent == Some("codex"));
    }
}

#[test]
fn host_answers_other_than_acceptance_leave_the_proposal_pending() {
    for result in [
        json!({"action":"accept","content":{}}),
        json!({"action":"accept"}),
        json!({"action":"decline"}),
        json!({"action":"cancel"}),
        json!({"content":{}}),
        json!({"action":true}),
    ] {
        let (transport, project, args) = prepared(Some("claude"), &capabilities(FORM));
        let before = project.revision();
        let form = transport.call(json!(3), "policy_apply", &args);
        assert_eq!(form["method"], "elicitation/create");
        assert_eq!(form["params"]["requestedSchema"]["properties"], json!({}));
        let message = form["params"]["message"].as_str().unwrap();
        assert!(message.contains("Rule ID: acme-read"), "{message}");
        assert!(message.contains("allow for acme"), "{message}");
        assert!(!message.contains(args["candidate_digest"].as_str().unwrap()));
        assert_eq!(project.revision(), before);
        // An unrelated response cannot answer the outstanding confirmation.
        transport.send(&json!({"jsonrpc":"2.0","id":"unrelated","result":{"action":"accept"}}));
        let response = transport.request(&answer(&form, &result));
        assert_eq!(response["id"], 3, "{response}");
        let accepted = result["action"] == "accept";
        assert_eq!(response["result"]["isError"], !accepted, "{response}");
        assert_eq!(project.revision() != before, accepted);
        if accepted {
            assert_eq!(
                response["result"]["structuredContent"]["data"]["operator"]["mechanism"],
                "host-elicitation"
            );
            // A retry reads its receipt without another form or commit.
            let retry = transport.call(json!(4), "policy_apply", &args);
            assert_eq!(retry["id"], 4, "{retry}");
            assert_eq!(retry["result"]["isError"], false);
        } else if result["action"] == "decline" {
            assert_eq!(error_code(&response), "approval_declined");
            let error = &response["result"]["structuredContent"]["error"];
            assert!(
                error["message"]
                    .as_str()
                    .unwrap()
                    .contains("do not retry unless the user requests it"),
                "{response}"
            );
        }
        drop(transport);
    }
}

#[test]
fn hosts_without_form_confirmation_cannot_activate_a_proposal() {
    for capabilities in [json!({}), json!({"elicitation":{"url":{}}})] {
        let (transport, project, args) = prepared(Some("claude"), &capabilities);
        let before = project.revision();
        let response = transport.call(json!(3), "policy_apply", &args);
        assert_eq!(response["id"], 3, "{response}");
        assert_eq!(error_code(&response), "approval_unavailable");
        assert!(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("enable MCP form elicitation")
        );
        assert_eq!(project.revision(), before);
    }
}

#[test]
fn unsupported_agents_are_sent_to_terminal_review() {
    let (transport, project, args) = prepared(Some("acme-agent"), &capabilities(FORM));
    let before = project.revision();
    let response = transport.call(json!(3), "policy_apply", &args);
    let error = &response["result"]["structuredContent"]["error"];
    assert_eq!(error["code"], "operator_review_required");
    assert_eq!(error["operator_route"], "shield policy review");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("shield policy review"));
    assert!(!message.contains("policy_apply"));
    assert!(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("shield policy review")
    );
    assert_eq!(project.revision(), before);
}

#[test]
fn generic_adapter_is_used_for_missing_and_unrecognized_hosts_even_with_form_support() {
    // A client's self-reported name never selects a review adapter.
    for (agent, client) in [
        (None, "acme-client"),
        (None, "codex"),
        (None, "claude-code"),
        (Some("acme-agent"), "acme-client"),
        (Some("Codex"), "codex"),
        (Some("claude-desktop"), "claude-code"),
    ] {
        let (server, project) = fixture(agent);
        let transport = Transport::start(server);
        for message in initialize(&capabilities(FORM), client) {
            transport.send(&message);
        }
        let initialized = transport.receive();
        assert!(
            initialized["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("in their terminal")
        );
        let proposal = transport.call(json!(2), "policy_propose", &rule("acme-generic", "acme"));
        let data = &proposal["result"]["structuredContent"]["data"];
        assert!(
            data["next_action"]
                .as_str()
                .unwrap()
                .contains("shield policy review in their terminal"),
            "{proposal}"
        );
        assert_eq!(
            data["provenance"]["agent"],
            agent.unwrap_or(client),
            "{proposal}"
        );
        let args = data["apply_arguments"].clone();
        let before = project.revision();
        let response = transport.call(json!(3), "policy_apply", &args);
        assert_eq!(response["id"], 3, "{response}");
        assert_ne!(response["method"], "elicitation/create", "{response}");
        assert_eq!(error_code(&response), "operator_review_required");
        assert_eq!(
            response["result"]["structuredContent"]["error"]["operator_route"],
            "shield policy review"
        );
        assert_eq!(project.revision(), before);
    }
}

#[test]
fn codex_applies_after_its_own_tool_permission_without_a_form() {
    let (transport, project, args) = prepared(Some("codex"), &capabilities(FORM));
    let before = project.revision();
    assert!(args["change"].as_str().unwrap().contains("allow for acme"));
    assert!(args.get("confirmation").is_none());
    let response = transport.call(json!(3), "policy_apply", &args);
    assert_eq!(response["id"], 3, "{response}");
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["data"]["operator"]["mechanism"],
        "host-tool-permission"
    );
    assert_ne!(project.revision(), before);
}

fn pending_hook_requests(project: &Project, action: String, count: usize) -> Vec<String> {
    use crate::review::hook_approval::{Identity, Key, Outcome, Summary, refused};
    let root = std::path::Path::new(&project.review.project_root);
    let active = crate::review::runtime::status(&project.review).unwrap()["active"].clone();
    let identity = Identity {
        activation_id: active["activation_id"].as_str().unwrap(),
        policy_revision: active["policy_revision"].as_str().unwrap(),
        candidate_digest: active["candidate_digest"].as_str().unwrap(),
    };
    let summary = Summary {
        action,
        reason: "the active policy denies it".into(),
        overrides_deny: true,
        deny_rules: vec!["acme-branch-delete".into()],
        settings_change: false,
    };
    (0..count)
        .map(|n| {
            let key = Key {
                agent: "codex",
                session: "acme-session",
                cwd: "/workspace/acme",
                tool: "Bash",
                input_digest: crate::policy::fingerprint(
                    format!("{}-{n}", summary.action).as_bytes(),
                ),
            };
            match refused(root, &key, &identity, &summary).unwrap() {
                Outcome::Pending(id, _) => id,
                Outcome::Allowed => panic!("expected a pending request"),
            }
        })
        .collect()
}

#[test]
fn request_approve_asks_once_for_a_group_and_the_schema_refuses_both_forms() {
    let (server, project) = fixture(Some("codex"));
    let ids = pending_hook_requests(
        &project,
        "Run this command:\ngit branch -D acme/task".into(),
        2,
    );
    let transport = Transport::start(server);
    for message in initialize(&capabilities(FORM), "codex") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());

    // The published schema refuses both argument forms before dispatch.
    let both = transport.call(
        json!(2),
        "request_approve",
        &json!({"request_id":ids[0],"request_ids":ids}),
    );
    assert_eq!(error_code(&both), "invalid_input", "{both}");
    let text = both["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("Tool arguments do not match the input schema: "),
        "{text}"
    );

    let form = transport.call(json!(3), "request_approve", &json!({"request_ids":ids}));
    assert_eq!(form["method"], "elicitation/create", "{form}");
    let message = form["params"]["message"].as_str().unwrap();
    assert!(
        message.contains("Accepting creates 2 separate one-time permissions. 2 of them override rules that deny them."),
        "{message}"
    );
    let response = transport.request(&answer(&form, &json!({"action":"accept"})));
    assert_eq!(response["id"], 3, "{response}");
    assert_eq!(response["result"]["isError"], false, "{response}");
    let grants = &response["result"]["structuredContent"]["data"]["grants"];
    assert_eq!(grants.as_array().map(Vec::len), Some(2), "{response}");
    let summary = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        summary.starts_with("The user approved these 2 requests once each."),
        "{summary}"
    );
}

#[test]
fn request_approve_examples_and_size_recovery_require_confirmation() {
    let (server, project) = fixture(Some("codex"));
    let ids = pending_hook_requests(
        &project,
        format!("Run this command:\necho {}", "é".repeat(3_500)),
        8,
    );
    let transport = Transport::start(server);
    for message in initialize(&capabilities(FORM), "codex") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    let catalog =
        transport.request(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
    let tool = catalog["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "request_approve")
        .unwrap();
    let examples = &tool["inputSchema"]["examples"];
    let mut group = examples[1].clone();
    group["request_ids"] = json!(ids);
    // Eight is schema-valid, but the complete form must also fit. A refusal
    // returns usable arguments through the real transport without opening a form.
    let oversized = transport.call(json!(3), "request_approve", &group);
    assert_eq!(error_code(&oversized), "invalid_input", "{oversized}");
    let error = &oversized["result"]["structuredContent"]["error"];
    assert_eq!(error["retry_arguments"], json!({"request_ids":[ids[0]]}));
    assert_eq!(error["remaining_request_ids"], json!(ids[1..]));
    let text = oversized["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains(&ids[0]), "{text}");
    assert!(text.contains("Nothing was approved"), "{text}");
    let retry = transport.call(json!(4), "request_approve", &error["retry_arguments"]);
    assert_eq!(retry["method"], "elicitation/create", "{retry}");
    assert!(retry["params"]["message"].as_str().unwrap().len() <= 12_000);
    let declined = transport.request(&answer(&retry, &json!({"action":"decline"})));
    assert_eq!(error_code(&declined), "approval_declined", "{declined}");
    // The advertised single-request example uses the same confirmation boundary.
    let mut single = examples[0].clone();
    single["request_id"] = json!(ids[1]);
    let form = transport.call(json!(5), "request_approve", &single);
    assert_eq!(form["method"], "elicitation/create", "{form}");
    let declined = transport.request(&answer(&form, &json!({"action":"decline"})));
    assert_eq!(error_code(&declined), "approval_declined", "{declined}");
}

#[test]
fn inspection_runs_while_a_form_is_open_and_a_second_form_is_unavailable() {
    let (transport, project, args) = prepared(Some("claude"), &capabilities(FORM));
    let before = project.revision();
    let form = transport.call(json!(3), "policy_apply", &args);
    assert_eq!(form["method"], "elicitation/create");
    for (id, name) in [(4, "status"), (5, "history_list"), (6, "rule_list")] {
        let response = transport.call(json!(id), name, &json!({}));
        assert_eq!(response["id"], id, "{response}");
        assert_eq!(response["result"]["isError"], false, "{response}");
    }
    let second = transport.call(json!(7), "policy_apply", &args);
    assert_eq!(second["id"], 7, "{second}");
    assert_eq!(error_code(&second), "approval_unavailable");
    assert_eq!(project.revision(), before);
    let applied = transport.request(&answer(&form, &json!({"action":"accept"})));
    assert_eq!(applied["id"], 3, "{applied}");
    assert_eq!(applied["result"]["isError"], false, "{applied}");
    assert_ne!(project.revision(), before);
}

#[test]
fn a_declined_form_reports_a_receipt_committed_meanwhile() {
    let (transport, project, args) = prepared(Some("claude"), &capabilities(FORM));
    let form = transport.call(json!(3), "policy_apply", &args);
    assert_eq!(form["method"], "elicitation/create");
    let applied = commit_elsewhere(&project, &args);
    let response = transport.request(&answer(&form, &json!({"action":"decline"})));
    assert_eq!(response["result"]["isError"], false, "{response}");
    let data = &response["result"]["structuredContent"]["data"];
    assert_eq!(data["status"], "applied");
    for field in [
        "activation_id",
        "candidate_digest",
        "policy_revision",
        "committed",
    ] {
        assert_eq!(data["receipt"][field], applied["data"]["receipt"][field]);
    }
    let mut changed = args;
    changed["candidate_digest"] = json!("acme-invalid-digest");
    let invalid = transport.call(json!(4), "policy_apply", &changed);
    assert_eq!(error_code(&invalid), "invalid_input");
}

#[test]
fn a_policy_change_while_the_form_is_open_makes_acceptance_stale() {
    let (transport, project, args) = prepared(Some("claude"), &capabilities(FORM));
    let form = transport.call(json!(3), "policy_apply", &args);
    assert_eq!(form["method"], "elicitation/create");
    // No writer or connection lock is held while the user decides.
    let other = propose(&transport, json!(4), &rule("acme-other", "other"));
    commit_elsewhere(&project, &other);
    let current = project.revision();
    let response = transport.request(&answer(&form, &json!({"action":"accept"})));
    assert_eq!(error_code(&response), "stale_revision", "{response}");
    assert_eq!(project.revision(), current);
}

#[test]
fn cancellation_or_disconnection_withdraws_the_form_without_committing() {
    for disconnect in [false, true] {
        let (transport, project, args) = prepared(Some("claude"), &capabilities(FORM));
        let before = project.revision();
        let form = transport.call(json!(3), "policy_apply", &args);
        assert_eq!(form["method"], "elicitation/create");
        let outputs = if disconnect {
            let (outputs, result) = transport.close();
            assert!(result.is_ok());
            outputs
        } else {
            transport.send(&json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}));
            let withdrawn = transport.receive();
            // A late acceptance of the withdrawn form is ignored.
            transport.send(&answer(&form, &json!({"action":"accept"})));
            let status = transport.call(json!(4), "status", &json!({}));
            assert_eq!(status["id"], 4, "{status}");
            vec![withdrawn]
        };
        assert_eq!(outputs[0]["method"], "notifications/cancelled");
        assert_eq!(outputs[0]["params"]["requestId"], form["id"]);
        if disconnect {
            assert_eq!(reply(&outputs, &json!(3))["error"]["code"], -32800);
        }
        assert_eq!(project.revision(), before);
    }
}

#[test]
fn confirmation_describes_retained_web_scope_and_removed_requirements() {
    let (transport, project, _) = prepared(Some("claude"), &capabilities(FORM));
    let message = |args: &Value| {
        crate::review::mcp::apply_confirmation(&project.review, args)
            .unwrap()
            .unwrap()
    };
    let narrow = json!({"id":"fetch", "decision":"allow", "conditions":{"domain":"web", "action":"fetch",
        "url":{"schemes":["https"],"hosts":["docs.example.com","api.example.com"]}},
        "requirements":{"url":{"schemes":["https"],"hosts":["docs.example.com"],"path":{"prefix":"/guide"}}},
        "on_unmet":{"decision":"deny"}});
    let mut broad = narrow.clone();
    broad.as_object_mut().unwrap().remove("requirements");
    broad.as_object_mut().unwrap().remove("on_unmet");
    let narrow_args = propose(
        &transport,
        json!(20),
        &json!({"change":{"kind":"rule_add","rule":narrow}}),
    );
    let broad_args = propose(
        &transport,
        json!(21),
        &json!({"change":{"kind":"rule_add","rule":broad}}),
    );
    let (narrow_message, broad_message) = (message(&narrow_args), message(&broad_args));
    assert_ne!(narrow_message, broad_message);
    assert!(narrow_message.contains("URL schemes: https; hosts: docs.example.com"));
    assert!(narrow_message.contains("URL path: prefix: /guide"));
    assert!(narrow_message.contains("If requirements fail or cannot be verified: deny"));
    assert!(
        broad_message
            .contains("hosts: docs.example.com, api.example.com; ports: any; URL path: any")
    );
    assert!(broad_message.contains("Requirements: none beyond the scope above"));
    assert!(!broad_message.contains("URL path: prefix: /guide"));
    commit_elsewhere(&project, &narrow_args);
    let edited = message(&propose(
        &transport,
        json!(22),
        &json!({"change":{"kind":"rule_edit","rule_id":"fetch","rule":broad}}),
    ));
    let (before, after) = edited.split_once("After: ").unwrap();
    assert!(before.contains("Before: allow for web fetch requests"));
    assert!(before.contains("hosts: docs.example.com"));
    assert!(after.contains("hosts: docs.example.com, api.example.com; ports: any; URL path: any"));
    assert!(after.contains("Requirements: none beyond the scope above"));
    assert!(!after.contains("URL path: prefix: /guide"));
    for message in [&narrow_message, &broad_message, &edited] {
        assert!(!message.contains("candidate_digest"));
        assert!(!message.contains("Requirements also apply"));
        assert!(!message.contains('{'));
    }
}
