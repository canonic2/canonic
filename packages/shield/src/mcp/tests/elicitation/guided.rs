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
fn guided_registry_details_default_to_project_scope_then_require_trust_confirmation() {
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
        assert_eq!(schema["required"], json!(["location"]));
        assert!(schema["properties"]["scope"].is_null());
        let collected = submit(
            &transport,
            "registry_add",
            &args,
            &form,
            "accept",
            json!({"location":"registry"}),
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
                json!({"location":"registry"}),
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
        json!({"location":"not-read"}),
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

fn confirm(
    transport: &Transport,
    name: &str,
    args: &Value,
    form: &Value,
    action: &str,
    modern: bool,
) -> Value {
    if modern {
        transport.request(&request(
            24,
            name,
            args,
            Some(form["result"]["requestState"].clone()),
            Some(json!({"confirmation":{"action":action}})),
            true,
        ))
    } else {
        transport.request(&answer(form, action, json!({})))
    }
}

#[test]
fn guided_review_rule_confirms_once_and_rejects_changed_arguments_or_replay() {
    for modern in [false, true] {
        for action in ["accept", "decline", "cancel"] {
            let (transport, project) = connected(VERSION, true);
            let rule = crate::mcp::tests::confirmation::rule("acme-guided-rule", "acme");
            let args = json!({"rule":rule["change"]["rule"],"review":true});
            let before = project.revision();
            let form = begin(&transport, "rule_add", &args, modern);
            assert_eq!(project.revision(), before);
            let reply = confirm(&transport, "rule_add", &args, &form, action, modern);
            if action == "accept" {
                assert_eq!(
                    data(&reply)["data"]["operator"]["mechanism"],
                    "host-elicitation",
                    "{reply}"
                );
                assert_ne!(project.revision(), before);
            } else {
                assert!(data(&reply)["error"].is_object(), "{reply}");
                assert_eq!(project.revision(), before);
            }
            if modern {
                assert_eq!(
                    confirm(&transport, "rule_add", &args, &form, "accept", true)["error"]["code"],
                    rpc::INVALID_PARAMS
                );
            }
        }
    }
    let (transport, project) = connected(VERSION, true);
    let rule = crate::mcp::tests::confirmation::rule("acme-guided-rule", "acme");
    let args = json!({"rule":rule["change"]["rule"],"review":true});
    let before = project.revision();
    let form = begin(&transport, "rule_add", &args, true);
    let mut changed = args.clone();
    changed["layer"] = json!("local");
    assert_eq!(
        confirm(&transport, "rule_add", &changed, &form, "accept", true)["error"]["code"],
        rpc::INVALID_PARAMS
    );
    assert_eq!(project.revision(), before);
}

#[test]
fn guided_review_requires_a_form_even_on_native_tool_approval_hosts() {
    for forms in [false, true] {
        let (server, project) = fixture(Some("codex"));
        let transport = Transport::start(server);
        for message in initialize(
            &if forms {
                json!({"elicitation":{"form":{}}})
            } else {
                json!({})
            },
            "codex",
        ) {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
        let rule = crate::mcp::tests::confirmation::rule("acme-guided-native", "acme");
        let args = json!({"rule":rule["change"]["rule"],"review":true});
        let before = project.revision();
        let form = begin(&transport, "rule_add", &args, false);
        assert_eq!(project.revision(), before);
        if forms {
            assert_eq!(form["method"], "elicitation/create", "{form}");
            let reply = confirm(&transport, "rule_add", &args, &form, "accept", false);
            assert_eq!(
                data(&reply)["data"]["operator"]["mechanism"],
                "host-elicitation",
                "{reply}"
            );
            assert_ne!(project.revision(), before);
        } else {
            assert_eq!(
                data(&form)["error"]["code"],
                "approval_unavailable",
                "{form}"
            );
            assert_eq!(project.revision(), before);
            let proposals = transport.call(json!(26), "policy_show", &json!({"status":"pending"}));
            assert_eq!(data(&proposals)["data"]["proposals"], json!([]));
        }
    }
}

#[test]
fn guided_review_registry_carries_details_into_separate_trust_without_relay() {
    for modern in [false, true] {
        let (transport, project) = connected(VERSION, true);
        let registry = project.root.path().join("registry");
        std::fs::create_dir(&registry).unwrap();
        std::fs::write(
            registry.join("catalog.json"),
            r#"{"schema_version":1,"packs":[]}"#,
        )
        .unwrap();
        let args = json!({"name":"acme","review":true});
        let input = begin(&transport, "registry_add", &args, modern);
        let next = submit(
            &transport,
            "registry_add",
            &args,
            &input,
            "accept",
            json!({"location":"registry"}),
            modern,
        );
        let (request, trust) = if modern {
            (args, next)
        } else {
            let collected = &data(&next)["data"];
            assert!(collected["retry_arguments"].is_null(), "{next}");
            let reference = collected["review_action"]["arguments"].clone();
            let trust = begin(&transport, "registry_add", &reference, false);
            (reference, trust)
        };
        assert!(trust.to_string().contains("Register acme"), "{trust}");
        assert!(data(&transport.call(json!(25), "registry_list", &json!({})))["data"]["sources"]["acme"].is_null());
        let accepted = confirm(
            &transport,
            "registry_add",
            &request,
            &trust,
            "accept",
            modern,
        );
        assert_eq!(data(&accepted)["data"]["committed"], true, "{accepted}");
        if !modern {
            assert!(data(&begin(&transport, "registry_add", &request, false))["error"].is_object());
        }
    }
}

#[test]
fn legacy_registry_details_reclaim_discarded_and_stale_references() {
    let (transport, project) = connected(VERSION, true);
    let args = json!({"name":"acme","review":true});
    let collect = || {
        let form = begin(&transport, "registry_add", &args, false);
        assert_eq!(form["method"], "elicitation/create", "{form}");
        let reply = submit(
            &transport,
            "registry_add",
            &args,
            &form,
            "accept",
            json!({"location":"not-read"}),
            false,
        );
        let reference = data(&reply)["data"]["review_action"]["arguments"].clone();
        assert!(reference["review_ref"].is_string(), "{reply}");
        reference
    };
    // A denied form releases capacity without retaining its input.
    let declined = begin(&transport, "registry_add", &args, false);
    assert_eq!(
        data(&submit(
            &transport,
            "registry_add",
            &args,
            &declined,
            "decline",
            json!({}),
            false,
        ))["error"]["code"],
        "input_declined"
    );
    let references: Vec<_> = (0..8).map(|_| collect()).collect();
    let blocked = begin(&transport, "registry_add", &args, false);
    assert_eq!(data(&blocked)["error"]["code"], "overloaded", "{blocked}");

    let mut discard = references[0].clone();
    discard["discard"] = json!(true);
    discard["name"] = json!("not-permitted");
    assert!(data(&begin(&transport, "registry_add", &discard, false))["error"].is_object());
    discard.as_object_mut().unwrap().remove("name");
    let discarded = begin(&transport, "registry_add", &discard, false);
    assert_eq!(
        data(&discarded)["data"]["status"],
        "discarded",
        "{discarded}"
    );
    assert_eq!(data(&discarded)["data"]["committed"], false);
    assert_eq!(
        data(&begin(&transport, "registry_add", &references[0], false))["error"]["code"],
        "not_found"
    );
    collect();

    let previous = project.revision();
    let rule = crate::mcp::tests::confirmation::rule("acme-registry-stale", "acme");
    let update = json!({"rule":rule["change"]["rule"],"review":true});
    let form = begin(&transport, "rule_add", &update, false);
    let accepted = confirm(&transport, "rule_add", &update, &form, "accept", false);
    assert!(data(&accepted)["error"].is_null(), "{accepted}");
    assert_ne!(project.revision(), previous);
    collect();
    assert_eq!(
        data(&begin(&transport, "registry_add", &references[1], false))["error"]["code"],
        "not_found"
    );
    let registries = transport.call(json!(25), "registry_list", &json!({}));
    assert!(data(&registries)["data"]["sources"]["acme"].is_null());
}

#[test]
fn guided_review_pack_removal_uses_retained_candidate_on_both_transports() {
    for modern in [false, true] {
        let (transport, project) = prepared("claude", &json!({"elicitation":{"form":{}}}));
        let args = json!({"review":true});
        let before = project.revision();
        let input = begin(&transport, "pack_remove", &args, modern);
        let selectors =
            definition(&input, modern)["properties"]["references"]["items"]["enum"].clone();
        let formed = submit(
            &transport,
            "pack_remove",
            &args,
            &input,
            "accept",
            json!({"references":selectors}),
            modern,
        );
        let (name, request, form) = if modern {
            ("pack_remove", args, formed)
        } else {
            let result = data(&formed);
            assert!(result["data"]["apply_arguments"].is_null());
            let request = result["data"]["review_action"]["arguments"].clone();
            let foreign =
                Transport::start(Server::for_review(project.review.as_ref().clone()).unwrap());
            for message in initialize(&json!({"elicitation":{"form":{}}}), "claude") {
                foreign.send(&message);
            }
            assert!(foreign.receive()["result"].is_object());
            assert!(data(&begin(&foreign, "policy_review", &request, false))["error"].is_object());
            let form = begin(&transport, "policy_review", &request, false);
            ("policy_review", request, form)
        };
        assert_eq!(project.revision(), before);
        let accepted = confirm(&transport, name, &request, &form, "accept", modern);
        assert_eq!(data(&accepted)["data"]["status"], "applied", "{accepted}");
        assert_ne!(project.revision(), before);
        if !modern {
            let repeated = begin(&transport, "policy_review", &request, false);
            assert_eq!(data(&repeated)["data"]["status"], "applied", "{repeated}");
        }
    }
}

#[test]
fn guided_review_refuses_when_policy_changes_after_the_confirmation_is_shown() {
    let (transport, project) = connected(VERSION, true);
    let rule = crate::mcp::tests::confirmation::rule("acme-guided-stale", "acme");
    let args = json!({"rule":rule["change"]["rule"],"review":true});
    let form = begin(&transport, "rule_add", &args, true);
    assert_eq!(form["result"]["resultType"], "input_required");
    let peer = Transport::start(Server::for_review(project.review.as_ref().clone()).unwrap());
    for message in initialize(&json!({"elicitation":{"form":{}}}), "claude") {
        peer.send(&message);
    }
    assert!(peer.receive()["result"].is_object());
    let other = crate::mcp::tests::confirmation::rule("acme-guided-peer", "acme");
    let other = json!({"rule":other["change"]["rule"],"review":true});
    let peer_form = begin(&peer, "rule_add", &other, true);
    let accepted = confirm(&peer, "rule_add", &other, &peer_form, "accept", true);
    assert_eq!(data(&accepted)["data"]["status"], "applied", "{accepted}");
    let revision = project.revision();
    let stale = confirm(&transport, "rule_add", &args, &form, "accept", true);
    assert!(data(&stale)["error"].is_object(), "{stale}");
    assert_eq!(project.revision(), revision);
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
