//! Broader input collection preserves exact review and approval boundaries.
use super::*;

fn begin(transport: &Transport, name: &str, arguments: &Value, modern: bool) -> Value {
    if modern {
        transport.request(&request(20, name, arguments, None, None, true))
    } else {
        transport.call(json!(20), name, arguments)
    }
}
fn request(
    id: i64,
    name: &str,
    args: &Value,
    state: Option<Value>,
    answers: Option<Value>,
    forms: bool,
) -> Value {
    let mut params = json!({"name":name,"arguments":args});
    if let Some(state) = state {
        params["requestState"] = state;
    }
    if let Some(answers) = answers {
        params["inputResponses"] = answers;
    }
    crate::mcp::tests::skills::modern(
        id,
        "tools/call",
        params,
        if forms {
            json!({"elicitation":{"form":{}}})
        } else {
            json!({})
        },
    )
}
fn submit(
    transport: &Transport,
    name: &str,
    args: &Value,
    form: &Value,
    action: &str,
    content: Value,
    modern: bool,
) -> Value {
    if modern {
        transport.request(&request(
            21,
            name,
            args,
            Some(form["result"]["requestState"].clone()),
            Some(json!({"settings":{"action":action,"content":content}})),
            true,
        ))
    } else {
        transport.request(&answer(form, action, content))
    }
}
fn definition(form: &Value, modern: bool) -> &Value {
    if modern {
        &form["result"]["inputRequests"]["settings"]["params"]["requestedSchema"]
    } else {
        &form["params"]["requestedSchema"]
    }
}
fn connected(version: &str, forms: bool) -> (Transport, Project) {
    let (server, project) = fixture(Some("claude"));
    let transport = Transport::start(server);
    let mut messages = initialize(
        &if forms {
            json!({"elicitation":{"form":{}}})
        } else {
            json!({})
        },
        "claude",
    );
    messages[0]["params"]["protocolVersion"] = json!(version);
    for message in messages {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    (transport, project)
}

#[test]
fn guided_input_cross_tool_retries_revoke_state_before_discovery_or_preparation() {
    let (transport, project) = connected(VERSION, true);
    let args = json!({});
    let form = begin(&transport, "registry_add", &args, true);
    let state = form["result"]["requestState"].clone();
    let wrong = transport.request(&request(22,"policy_propose",&crate::mcp::tests::confirmation::rule("acme-forged","acme"),Some(state),Some(json!({"settings":{"action":"accept","content":{"name":"acme","location":"not-read","scope":"project"}}})),true));
    assert_eq!(wrong["error"]["code"], rpc::INVALID_PARAMS, "{wrong}");
    let replay = submit(
        &transport,
        "registry_add",
        &args,
        &form,
        "accept",
        json!({"name":"acme","location":"not-read","scope":"project"}),
        true,
    );
    assert_eq!(replay["error"]["code"], rpc::INVALID_PARAMS);
    assert_eq!(proposals(&project), 0);
}

#[test]
fn guided_registry_cancel_and_disconnect_ignore_late_answers() {
    for scenario in ["cancel", "disconnect"] {
        let (server, project) = fixture(Some("claude"));
        let transport = Transport::with_options(server, test_options());
        for message in initialize(&json!({"elicitation":{"form":{}}}), "claude") {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
        let before = project.revision();
        let form = transport.call(json!(20), "registry_add", &json!({}));
        if scenario == "disconnect" {
            transport.close().1.unwrap();
        } else {
            transport.send(&json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":20}}));
            assert_eq!(transport.receive()["method"], "notifications/cancelled");
            transport.send(&answer(
                &form,
                "accept",
                json!({"name":"acme","location":"not-read","scope":"project"}),
            ));
            assert_eq!(transport.call(json!(22), "status", &json!({}))["id"], 22);
        }
        assert_eq!(project.revision(), before);
        assert_eq!(proposals(&project), 0);
    }
}

#[test]
fn guided_removal_invalid_selections_and_stale_base_create_no_removal_proposal() {
    let (transport, project) = prepared("claude", &json!({"elicitation":{"form":{}}}));
    let count = proposals(&project);
    for selection in [json!([]), json!(["not-installed"])] {
        let form = begin(&transport, "pack_remove", &json!({}), false);
        let result = submit(
            &transport,
            "pack_remove",
            &json!({}),
            &form,
            "accept",
            json!({"references":selection}),
            false,
        );
        assert_eq!(data(&result)["error"]["code"], "invalid_input");
        assert_eq!(proposals(&project), count);
    }
    let form = begin(&transport, "pack_remove", &json!({}), true);
    let selectors = definition(&form, true)["properties"]["references"]["items"]["enum"].clone();
    let change = transport.call(
        json!(25),
        "policy_propose",
        &crate::mcp::tests::confirmation::rule("acme-other", "acme"),
    );
    crate::mcp::tests::confirmation::commit_elsewhere(
        &project,
        &data(&change)["data"]["apply_arguments"],
    );
    let revision = project.revision();
    let stale = submit(
        &transport,
        "pack_remove",
        &json!({}),
        &form,
        "accept",
        json!({"references":selectors}),
        true,
    );
    assert_eq!(data(&stale)["error"]["code"], "stale_revision", "{stale}");
    assert_eq!(data(&stale)["error"]["operator_route"], "pack_remove");
    assert_eq!(project.revision(), revision);
    assert_eq!(proposals(&project), count + 1);
}

#[test]
fn guided_registry_details_require_explicit_scope_then_separate_trust_confirmation() {
    for (version, modern) in [("2025-06-18", false), (VERSION, false), (VERSION, true)] {
        let (transport, project) = connected(version, true);
        let root = project.root.path().join("registry");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("catalog.json"),
            r#"{"schema_version":1,"packs":[]}"#,
        )
        .unwrap();
        let before = project.revision();
        let args = json!({"name":"acme"});
        let form = begin(&transport, "registry_add", &args, modern);
        let schema = definition(&form, modern);
        assert!(schema["properties"]["name"].is_null(), "{form}");
        assert_eq!(schema["required"], json!(["location", "scope"]));
        assert!(schema["properties"]["scope"].get("default").is_none());
        let collected = submit(
            &transport,
            "registry_add",
            &args,
            &form,
            "accept",
            json!({"location":"registry","scope":"project"}),
            modern,
        );
        let retry = &data(&collected)["data"]["retry_arguments"];
        assert_eq!(
            retry,
            &json!({"name":"acme","location":"registry","scope":"project"}),
            "{collected}"
        );
        assert_eq!(data(&collected)["data"]["committed"], false);
        assert!(data(&transport.call(json!(22), "registry_list", &json!({})))["data"]["sources"]["acme"].is_null());
        let trust = begin(&transport, "registry_add", retry, modern);
        let accepted = if modern {
            let mut call = request(
                23,
                "registry_add",
                retry,
                Some(trust["result"]["requestState"].clone()),
                None,
                true,
            );
            call["params"]["inputResponses"] = json!({"confirmation":{"action":"accept"}});
            transport.request(&call)
        } else {
            transport.request(&answer(&trust, "accept", json!({})))
        };
        assert!(data(&accepted)["error"].is_null(), "{accepted}");
        assert_eq!(data(&accepted)["data"]["committed"], true);
        assert_eq!(project.revision(), before);
        if modern {
            let replay = submit(
                &transport,
                "registry_add",
                &args,
                &form,
                "accept",
                json!({"location":"registry","scope":"project"}),
                true,
            );
            assert_eq!(replay["error"]["code"], rpc::INVALID_PARAMS);
        }
    }
}

#[test]
fn guided_registry_invalid_partial_capability_and_changed_argument_retries_never_enroll() {
    let (transport, project) = connected(VERSION, true);
    let before = project.revision();
    let args = json!({});
    let form = begin(&transport, "registry_add", &args, true);
    let state = form["result"]["requestState"].clone();
    let partial = transport.request(&request(
        22,
        "registry_add",
        &args,
        Some(state.clone()),
        Some(json!({})),
        true,
    ));
    assert_eq!(partial["result"]["requestState"], state);
    let incomplete = submit(
        &transport,
        "registry_add",
        &args,
        &form,
        "accept",
        json!({"name":"acme","location":"not-read"}),
        true,
    );
    assert_eq!(incomplete["error"]["code"], rpc::INVALID_PARAMS);
    let lost_capability = transport.request(&request(23,"registry_add", &args, Some(state.clone()), Some(json!({"settings":{"action":"accept","content":{"name":"acme","location":"not-read","scope":"project"}}})), false));
    assert_eq!(lost_capability["error"]["code"], -32021);
    let changed = submit(
        &transport,
        "registry_add",
        &json!({"scope":"user"}),
        &form,
        "accept",
        json!({"name":"acme","location":"not-read","scope":"project"}),
        true,
    );
    assert_eq!(changed["error"]["code"], rpc::INVALID_PARAMS);
    let replay = submit(
        &transport,
        "registry_add",
        &args,
        &form,
        "accept",
        json!({"name":"acme","location":"not-read","scope":"project"}),
        true,
    );
    assert_eq!(replay["error"]["code"], rpc::INVALID_PARAMS);
    assert_eq!(project.revision(), before);
    assert_eq!(proposals(&project), 0);
}

#[test]
fn guided_registry_decline_dismiss_invalid_and_missing_forms_preserve_state() {
    for (version, modern) in [("2025-06-18", false), (VERSION, false), (VERSION, true)] {
        for action in ["decline", "cancel", "invalid"] {
            let (transport, project) = connected(version, true);
            let before = project.revision();
            let args = json!({"name":"acme","scope":"project"});
            let form = begin(&transport, "registry_add", &args, modern);
            let result = submit(
                &transport,
                "registry_add",
                &args,
                &form,
                if action == "invalid" {
                    "accept"
                } else {
                    action
                },
                json!({"location":7}),
                modern,
            );
            if action == "invalid" && modern {
                assert_eq!(result["error"]["code"], rpc::INVALID_PARAMS);
            } else {
                assert_eq!(
                    data(&result)["error"]["code"],
                    if action == "decline" {
                        "input_declined"
                    } else if action == "cancel" {
                        "input_cancelled"
                    } else {
                        "invalid_input"
                    },
                    "{result}"
                );
            }
            assert_eq!(project.revision(), before);
            assert_eq!(proposals(&project), 0);
        }
    }
    let (transport, project) = connected(VERSION, false);
    let result = begin(&transport, "registry_add", &json!({}), false);
    assert_eq!(data(&result)["error"]["code"], "input_unavailable");
    assert_eq!(proposals(&project), 0);
}

#[test]
fn guided_removal_keeps_exact_selection_inactive_until_separate_approval() {
    for modern in [false, true] {
        let (transport, project) = prepared("claude", &json!({"elicitation":{"form":{}}}));
        let before = project.revision();
        let count = proposals(&project);
        let args = json!({});
        let form = begin(&transport, "pack_remove", &args, modern);
        let selectors =
            definition(&form, modern)["properties"]["references"]["items"]["enum"].clone();
        assert_eq!(selectors.as_array().unwrap().len(), 1);
        let proposal = submit(
            &transport,
            "pack_remove",
            &args,
            &form,
            "accept",
            json!({"references":selectors}),
            modern,
        );
        assert!(data(&proposal)["error"].is_null(), "{proposal}");
        assert_eq!(project.revision(), before);
        assert_eq!(proposals(&project), count + 1);
        let approval = transport.call(
            json!(23),
            "policy_apply",
            &data(&proposal)["data"]["apply_arguments"],
        );
        assert_eq!(approval["method"], "elicitation/create");
        assert_eq!(project.revision(), before);
        let result = transport.request(&answer(&approval, "accept", json!({})));
        assert!(data(&result)["error"].is_null(), "{result}");
        assert_ne!(project.revision(), before);
        assert_eq!(
            data(&transport.call(json!(24), "pack_list", &json!({})))["data"]["packs"],
            json!([])
        );
    }
}

#[test]
fn guided_removal_earliest_revision_never_narrows_multiple_selection() {
    let (transport, project) = prepared_with_options(
        "claude",
        &json!({"elicitation":{"form":{}}}),
        test_options(),
        "2025-06-18",
    );
    let count = proposals(&project);
    let result = transport.call(json!(20), "pack_remove", &json!({}));
    assert_eq!(data(&result)["error"]["code"], "input_unavailable");
    assert_eq!(proposals(&project), count);
    let direct = transport.call(
        json!(21),
        "pack_remove",
        &json!({"reference":"acme/settings"}),
    );
    assert!(
        data(&direct)["data"]["apply_arguments"].is_object(),
        "{direct}"
    );
}
