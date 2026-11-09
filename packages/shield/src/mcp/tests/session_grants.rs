//! Reusable grants through the shipping stdio confirmation transport.
use super::*;
mod reconsideration;

fn prepared(
    agent: &str,
    forms: bool,
) -> (
    Transport,
    assert_fs::TempDir,
    crate::review::Context,
    String,
) {
    let (root, context, reference) = crate::review::session_grants::native_fixture_for_agent(agent);
    let transport =
        Transport::with_options(Server::for_review(context.clone()).unwrap(), test_options());
    for message in initialize(
        &if forms {
            json!({"elicitation":{"form":{}}})
        } else {
            json!({})
        },
        agent,
    ) {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    (transport, root, context, reference)
}

fn scope() -> Value {
    json!([{"conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["merge","42"]},"cwd":{"exact":{"root":"project"}}}}])
}

#[test]
fn session_grants_unknown_hosts_never_receive_approval_forms() {
    for agent in [None, Some("unknown-agent")] {
        let (_root, mut context, reference) = crate::review::session_grants::native_fixture();
        let proposal = crate::review::session_grants::propose(
            &context,
            &json!({
                "session_ref":reference,"idempotency_key":"unknown-host","scope":scope()
            }),
        )
        .unwrap();
        context.agent = agent.map(str::to_owned);
        let transport = Transport::start(Server::for_review(context.clone()).unwrap());
        for message in initialize(&json!({"elicitation":{"form":{}}}), "auto-answer-client") {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
        {
            let response = transport.call(
                json!(2),
                "session_grant_approve",
                &proposal["apply_arguments"],
            );
            assert!(response.get("method").is_none(), "{response}");
            assert_eq!(response["result"]["isError"], true, "{response}");
        }
        let row = crate::review::session_grants::show(
            &context,
            &json!({"proposal_id":proposal["proposal_id"]}),
        )
        .unwrap();
        assert_eq!(row["proposal_status"], "pending");
    }
}

#[test]
fn session_grants_transport_requires_its_own_form_and_replays_one_receipt() {
    for agent in ["claude", "codex", "gemini", "agy"] {
        let (transport, _root, context, reference) = prepared(agent, true);
        let proposed = transport.call(
            json!(2),
            "session_grant_propose",
            &json!({"session_ref":reference,"idempotency_key":"wire","scope":scope()}),
        );
        let envelope = &proposed["result"]["structuredContent"];
        assert!(envelope["error"].is_null(), "{proposed}");
        assert!(envelope["data"].get("report_resources").is_none());
        let args = &envelope["data"]["apply_arguments"];
        let form = transport.call(json!(3), "session_grant_approve", args);
        assert_eq!(form["method"], "elicitation/create", "{form}");
        assert!(
            form["params"]["message"]
                .as_str()
                .unwrap()
                .contains("Unlimited matching uses")
        );
        assert_eq!(
            crate::review::session_grants::show(
                &context,
                &json!({"proposal_id":args["proposal_id"]})
            )
            .unwrap()["proposal_status"],
            "pending"
        );
        let accepted = transport.request(
            &json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"accept","content":{}}}),
        );
        let issued = &accepted["result"]["structuredContent"];
        assert!(issued["error"].is_null(), "{accepted}");
        assert!(issued["data"].get("report_resources").is_none());
        assert_eq!(issued["data"]["grant_status"], "active");
        let replay = transport.call(json!(4), "session_grant_approve", args);
        assert_eq!(
            replay["result"]["structuredContent"]["data"]["grant_id"], issued["data"]["grant_id"],
            "{replay}"
        );
        assert_eq!(
            replay["result"]["structuredContent"]["data"]["expires_at"],
            issued["data"]["expires_at"]
        );
    }
}

#[test]
fn session_grants_transport_decline_and_missing_forms_issue_nothing() {
    for forms in [false, true] {
        let (transport, _root, context, reference) = prepared("codex", forms);
        let proposed = transport.call(
            json!(2),
            "session_grant_propose",
            &json!({"session_ref":reference,"idempotency_key":"wire","scope":scope()}),
        );
        let args = &proposed["result"]["structuredContent"]["data"]["apply_arguments"];
        let result = transport.call(json!(3), "session_grant_approve", args);
        let result = if forms {
            assert_eq!(result["method"], "elicitation/create", "{result}");
            transport
                .request(&json!({"jsonrpc":"2.0","id":result["id"],"result":{"action":"decline"}}))
        } else {
            result
        };
        assert_eq!(
            result["result"]["structuredContent"]["error"]["code"],
            if forms {
                "approval_declined"
            } else {
                "approval_unavailable"
            },
            "{result}"
        );
        let row = crate::review::session_grants::show(
            &context,
            &json!({"proposal_id":args["proposal_id"]}),
        )
        .unwrap();
        assert!(row["grant_id"].is_null());
        assert_eq!(
            row["proposal_status"],
            if forms { "declined" } else { "pending" }
        );
    }
}

#[test]
fn session_grants_modern_confirmation_binds_arguments_and_consumes_continuation() {
    for action in ["accept", "decline", "cancel"] {
        let (transport, _root, context, reference) = prepared("codex", false);
        let proposed = transport.call(
            json!(2),
            "session_grant_propose",
            &json!({"session_ref":reference,"idempotency_key":"modern","scope":scope()}),
        );
        let args = &proposed["result"]["structuredContent"]["data"]["apply_arguments"];
        let request = |id, state: Option<&Value>, action: Option<&str>| {
            let mut params = json!({"name":"session_grant_approve","arguments":args});
            if let Some(state) = state {
                params["requestState"] = state.clone();
            }
            if let Some(action) = action {
                params["inputResponses"] = json!({"confirmation":{"action":action}});
            }
            super::skills::modern(id, "tools/call", params, json!({"elicitation":{"form":{}}}))
        };
        let form = transport.request(&request(10, None, None));
        assert_eq!(form["result"]["resultType"], "input_required", "{form}");
        let token = &form["result"]["requestState"];
        let result = transport.request(&request(11, Some(token), Some(action)));
        assert_eq!(result["result"]["resultType"], "complete", "{result}");
        let row = crate::review::session_grants::show(
            &context,
            &json!({"proposal_id":args["proposal_id"]}),
        )
        .unwrap();
        assert_eq!(row["grant_status"] == "active", action == "accept", "{row}");
        let replay = transport.request(&request(12, Some(token), Some("accept")));
        assert_eq!(replay["error"]["code"], rpc::INVALID_PARAMS, "{replay}");
    }
}

#[test]
fn session_grants_decline_reports_an_issuance_committed_in_another_review() {
    let (transport, _root, context, reference) = prepared("codex", true);
    let proposed = transport.call(
        json!(2),
        "session_grant_propose",
        &json!({"session_ref":reference,"idempotency_key":"concurrent","scope":scope()}),
    );
    let args = &proposed["result"]["structuredContent"]["data"]["apply_arguments"];
    let form = transport.call(json!(3), "session_grant_approve", args);
    assert_eq!(form["method"], "elicitation/create");
    let committed = crate::review::session_grants::apply(&context, args, true).unwrap();
    let declined =
        transport.request(&json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"decline"}}));
    assert!(
        declined["result"]["structuredContent"]["error"].is_null(),
        "{declined}"
    );
    assert_eq!(
        declined["result"]["structuredContent"]["data"]["grant_id"],
        committed["grant_id"]
    );
}
