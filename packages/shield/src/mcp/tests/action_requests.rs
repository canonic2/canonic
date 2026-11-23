//! Shipping forms dispatch exact SDK-read and checked-run requests, never tool permission alone.
use super::*;
use crate::review::approvals::tests::Fixture;

fn begin(transport: &Transport, args: &Value, modern: bool) -> Value {
    if modern {
        transport.request(&super::skills::modern(
            10,
            "tools/call",
            json!({"name":"request_approve","arguments":args}),
            json!({"elicitation":{"form":{}}}),
        ))
    } else {
        transport.call(json!(10), "request_approve", args)
    }
}
fn answer(transport: &Transport, args: &Value, form: &Value, action: &str, modern: bool) -> Value {
    if modern {
        transport.request(&super::skills::modern(11, "tools/call",
        json!({"name":"request_approve","arguments":args,"requestState":form["result"]["requestState"],
            "inputResponses":{"confirmation":{"action":action}}}), json!({"elicitation":{"form":{}}})))
    } else {
        transport.request(
            &json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":action,"content":{}}}),
        )
    }
}
fn connect(fixture: &Fixture, forms: bool) -> Transport {
    let transport = Transport::start(Server::for_review(fixture.context.clone()).unwrap());
    let capabilities = if forms {
        json!({"elicitation":{"form":{}}})
    } else {
        json!({})
    };
    for message in initialize(&capabilities, fixture.context.agent.as_deref().unwrap()) {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    transport
}
fn no_grants(f: &Fixture) {
    let state: Value = serde_json::from_slice(
        &std::fs::read(f.root.path().join(".shield/state/state.json")).unwrap(),
    )
    .unwrap();
    assert!(
        !state["authorization_events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| matches!(
                event["kind"].as_str(),
                Some("read_grant_issued" | "run_grant_issued")
            ))
    );
}

#[test]
fn action_request_kinds_shipping_forms_require_acceptance_on_both_protocols() {
    for agent in ["claude", "codex"] {
        for modern in [false, true] {
            for action in ["accept", "decline", "cancel"] {
                let f = crate::review::approvals::tests::fixture(agent);
                let transport = connect(&f, true);
                let args = json!({"request_ids":f.ids});
                let form = begin(&transport, &args, modern);
                assert_eq!(
                    if modern {
                        &form["result"]["resultType"]
                    } else {
                        &form["method"]
                    },
                    if modern {
                        "input_required"
                    } else {
                        "elicitation/create"
                    },
                    "{form}"
                );
                let text = form.to_string();
                assert!(
                    text.contains("SDK text read")
                        && text.contains("checked command")
                        && !text.contains("Acme private text")
                );
                no_grants(&f);
                let inspection = transport.call(json!(12), "request_list", &json!({}));
                assert_eq!(inspection["result"]["isError"], false, "{inspection}");
                let result = answer(&transport, &args, &form, action, modern);
                if action == "accept" {
                    assert_eq!(result["result"]["isError"], false, "{result}");
                    assert_eq!(
                        result["result"]["structuredContent"]["data"]["grants"]
                            .as_array()
                            .unwrap()
                            .len(),
                        2
                    );
                    for id in &f.ids {
                        let shown =
                            transport.call(json!(13), "request_show", &json!({"request_id":id}));
                        assert_eq!(
                            shown["result"]["structuredContent"]["data"]["status"], "approved",
                            "{shown}"
                        );
                    }
                } else {
                    assert_eq!(
                        result["result"]["structuredContent"]["error"]["code"], "approval_declined",
                        "{result}"
                    );
                    no_grants(&f);
                }
            }
        }
    }
}

#[test]
fn action_request_kinds_file_changed_during_form_prevents_the_entire_group() {
    for modern in [false, true] {
        let f = crate::review::approvals::tests::fixture("codex");
        let transport = connect(&f, true);
        let args = json!({"request_ids":f.ids});
        let form = begin(&transport, &args, modern);
        std::fs::write(f.root.path().join("acme.txt"), "changed").unwrap();
        let result = answer(&transport, &args, &form, "accept", modern);
        assert_eq!(result["result"]["isError"], true, "{result}");
        no_grants(&f);
    }
}

#[test]
fn action_request_kinds_missing_forms_and_disconnect_issue_nothing() {
    for forms in [false, true] {
        let f = crate::review::approvals::tests::fixture("codex");
        let transport = connect(&f, forms);
        let args = json!({"request_id":f.ids[0]});
        let form = begin(&transport, &args, false);
        if forms {
            assert_eq!(form["method"], "elicitation/create", "{form}");
            assert!(transport.close().1.is_ok());
        } else {
            assert_eq!(
                form["result"]["structuredContent"]["error"]["code"], "approval_unavailable",
                "{form}"
            );
        }
        no_grants(&f);
    }
}

#[test]
fn action_request_summary_uses_the_grant_route_and_reports_consumed_replays() {
    let f = crate::review::approvals::tests::fixture("codex");
    let transport = connect(&f, true);
    let request_id = &f.ids[1];
    let args = json!({"request_id":request_id});
    let form = begin(&transport, &args, false);
    let approved = answer(&transport, &args, &form, "accept", false);
    let text = approved["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains(&format!("--approval-request {request_id}")),
        "{approved}"
    );

    let mut run = f.context.clone();
    run.surface = "run".into();
    let (result, _) = crate::review::run_approval::gate(
        &run,
        &f.run,
        &f.run_resource,
        Some(request_id),
        || true,
        |_| (),
    )
    .unwrap();
    assert_eq!(result["allowed"], true, "{result}");

    let read_args = json!({"request_id":f.ids[0]});
    let form = begin(&transport, &read_args, false);
    let approved = answer(&transport, &read_args, &form, "accept", false);
    assert_eq!(approved["result"]["isError"], false, "{approved}");
    let read = crate::review::read_approval::gate(
        &f.context,
        "acme-factory",
        &f.read,
        &f.read_digest,
        Some(&f.ids[0]),
        || true,
    )
    .unwrap();
    assert_eq!(read["allowed"], true, "{read}");

    for args in [args, json!({"request_ids":f.ids})] {
        let replay = begin(&transport, &args, false);
        assert_eq!(
            replay["result"]["structuredContent"]["data"]["allowed"], false,
            "{replay}"
        );
        let text = replay["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("grant") && text.contains("no permission"),
            "{replay}"
        );
        assert!(
            !text.contains("approved") && !text.contains("retry"),
            "{replay}"
        );
    }
}
