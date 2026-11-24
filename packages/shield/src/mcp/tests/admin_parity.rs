//! Overlapping forms retain distinct immutable administrative plans.
use super::*;
#[cfg(unix)]
mod history_controls;

#[test]
fn administrative_review_large_setup_uses_one_form_and_preserves_native_settings() {
    for modern in [false, true] {
        let (server, project) = fixture(Some("codex"));
        let path = project.root.path().join(".codex/config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let settings = format!("# {}\n", "unrelated-native-setting".repeat(3000));
        std::fs::write(&path, &settings).unwrap();
        let transport = connect(server);
        let args = json!({"agents":["codex"]});
        let form = if modern {
            transport.request(&super::skills::modern(
                2,
                "tools/call",
                json!({"name":"mcp_setup","arguments":args}),
                json!({"elicitation":{"form":{}}}),
            ))
        } else {
            transport.call(json!(2), "mcp_setup", &args)
        };
        let message = if modern {
            &form["result"]["inputRequests"]["confirmation"]["params"]["message"]
        } else {
            &form["params"]["message"]
        };
        assert!(
            message.as_str().is_some_and(|text| text.contains("Codex")
                && text.len() < 12_000
                && !text.contains("unrelated-native-setting")),
            "{form}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), settings);
        let result = if modern {
            transport.request(&super::skills::modern(3, "tools/call", json!({"name":"mcp_setup","arguments":args,"requestState":form["result"]["requestState"],"inputResponses":{"confirmation":{"action":"accept"}}}), json!({"elicitation":{"form":{}}})))
        } else {
            transport.request(
                &json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"accept","content":{}}}),
            )
        };
        assert_eq!(result["result"]["isError"], false, "{result}");
        assert!(
            std::fs::read_to_string(path)
                .unwrap()
                .contains(settings.trim_end())
        );
        assert!(!result.to_string().contains("unrelated-native-setting"));
    }
}

#[test]
fn shield_mode_transport_forms_require_acceptance_to_disable_and_reenable() {
    for modern in [false, true] {
        let (server, _project) = fixture(Some("codex"));
        let transport = connect(server);
        let mut enabled = true;
        for (name, target) in [("disable", false), ("enable", true)] {
            for action in ["decline", "accept"] {
                let form = if modern {
                    transport.request(&super::skills::modern(
                        10,
                        "tools/call",
                        json!({"name":name,"arguments":{}}),
                        json!({"elicitation":{"form":{}}}),
                    ))
                } else {
                    transport.call(json!(10), name, &json!({}))
                };
                let result = if modern {
                    assert_eq!(form["result"]["resultType"], "input_required", "{form}");
                    transport.request(&super::skills::modern(11, "tools/call",
                        json!({"name":name,"arguments":{},"requestState":form["result"]["requestState"],
                            "inputResponses":{"confirmation":{"action":action}}}),
                        json!({"elicitation":{"form":{}}})))
                } else {
                    assert_eq!(form["method"], "elicitation/create", "{form}");
                    transport.request(&json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":action,"content":{}}}))
                };
                let data = &result["result"]["structuredContent"];
                if action == "accept" {
                    assert!(data["error"].is_null(), "{result}");
                    enabled = target;
                    assert_eq!(data["data"]["enabled"], enabled);
                } else {
                    assert_eq!(data["error"]["code"], "approval_declined", "{result}");
                }
                let status = transport.call(json!(12), "status", &json!({}));
                assert_eq!(
                    status["result"]["structuredContent"]["data"]["enabled"], enabled,
                    "{status}"
                );
            }
        }
    }
}

#[test]
fn admin_parity_unavailable_and_cancelled_forms_preserve_settings_and_tool_recovery() {
    for modern in [false, true] {
        for action in ["unavailable", "cancel"] {
            let (server, _project) = fixture(Some("codex"));
            let transport = Transport::start(server);
            let capabilities = if action == "unavailable" {
                json!({})
            } else {
                json!({"elicitation":{"form":{}}})
            };
            for message in initialize(&capabilities, "codex") {
                transport.send(&message);
            }
            assert!(transport.receive()["result"].is_object());
            let before = settings(&transport, 2);
            let args = json!({"enabled":false});
            let result = if action == "cancel" {
                let form = begin(&transport, 3, &args, modern);
                answer(&transport, 4, &args, &form, action, modern)
            } else if modern {
                transport.request(&super::skills::modern(
                    3,
                    "tools/call",
                    json!({"name":"history_configure","arguments":args}),
                    json!({}),
                ))
            } else {
                transport.call(json!(3), "history_configure", &args)
            };
            let error = &result["result"]["structuredContent"]["error"];
            assert_eq!(
                error["code"],
                if action == "cancel" {
                    "approval_declined"
                } else {
                    "approval_unavailable"
                },
                "{result}"
            );
            let message = error["message"].as_str().unwrap();
            assert!(
                !message.contains("policy_propose") && !message.contains("shield policy review"),
                "{result}"
            );
            if action == "unavailable" {
                assert!(error["operator_route"].is_null());
                assert!(message.contains("/permissions"), "{result}");
                assert!(!message.contains("terminal"), "{result}");
            } else {
                assert!(message.contains("administrative change"), "{result}");
            }
            assert_eq!(settings(&transport, 5), before, "{result}");
        }
    }
}

#[test]
fn enrolled_setup_without_forms_keeps_the_resolved_agent_in_the_terminal_command() {
    for modern in [false, true] {
        let (server, project) = fixture(Some("codex"));
        let transport = Transport::start(server);
        for message in initialize(&json!({}), "codex") {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
        let response = if modern {
            transport.request(&super::skills::modern(
                2,
                "tools/call",
                json!({"name":"setup","arguments":{}}),
                json!({}),
            ))
        } else {
            transport.call(json!(2), "setup", &json!({}))
        };
        let error = &response["result"]["structuredContent"]["error"];
        assert_eq!(error["code"], "approval_unavailable", "{response}");
        assert_eq!(
            error["operator_action"],
            json!({"command":"shield",
            "argv":["--project",project.review.project_root,"setup","--agent","codex"]})
        );
        assert!(!project.root.path().join(".codex/hooks.json").exists());
    }
}

#[test]
fn admin_parity_known_native_hosts_without_form_ui_report_the_in_session_gap() {
    for (agent, version) in [("gemini", "0.67.0"), ("agy", "1.5.1")] {
        let (server, _project) = fixture(Some(agent));
        let transport = Transport::start(server);
        let mut messages = initialize(&json!({"elicitation":{"form":{}}}), agent);
        messages[0]["params"]["clientInfo"]["version"] = json!(version);
        for message in messages {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
        let response = transport.call(json!(2), "history_configure", &json!({"enabled":false}));
        assert!(response.get("method").is_none(), "{response}");
        let error = &response["result"]["structuredContent"]["error"];
        assert_eq!(error["code"], "approval_unavailable", "{response}");
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("missing in-session capability")
        );
        assert!(
            !error["message"]
                .as_str()
                .unwrap()
                .contains("enable confirmation")
        );
    }
}

#[test]
fn admin_parity_unknown_hosts_cannot_receive_confirmation_forms() {
    for agent in [None, Some("unknown-agent")] {
        let (server, _project) = fixture(agent);
        let transport = Transport::start(server);
        for message in initialize(&json!({"elicitation":{"form":{}}}), "auto-answer-client") {
            transport.send(&message);
        }
        assert!(transport.receive()["result"].is_object());
        for (name, args) in [
            ("disable", json!({})),
            ("enable", json!({})),
            ("history_configure", json!({"purge":true})),
            ("protection_set", json!({"agent_settings_change":"allow"})),
        ] {
            let result = transport.call(json!(2), name, &args);
            assert!(result.get("method").is_none(), "{result}");
            assert!(result["result"]["isError"].as_bool().unwrap(), "{result}");
            let result = transport.request(&super::skills::modern(
                3,
                "tools/call",
                json!({"name":name,"arguments":args}),
                json!({"elicitation":{"form":{}}}),
            ));
            assert_ne!(result["result"]["resultType"], "input_required", "{result}");
        }
    }
}

fn connect(server: Server) -> Transport {
    let transport = Transport::start(server);
    for message in initialize(&json!({"elicitation":{"form":{}}}), "codex") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    transport
}

fn begin(transport: &Transport, id: i64, args: &Value, modern: bool) -> Value {
    let response = if modern {
        transport.request(&super::skills::modern(
            id,
            "tools/call",
            json!({"name":"history_configure","arguments":args}),
            json!({"elicitation":{"form":{}}}),
        ))
    } else {
        transport.call(json!(id), "history_configure", args)
    };
    if modern {
        assert_eq!(
            response["result"]["resultType"], "input_required",
            "{response}"
        );
    } else {
        assert_eq!(response["method"], "elicitation/create", "{response}");
    }
    response
}

fn answer(
    transport: &Transport,
    id: i64,
    args: &Value,
    form: &Value,
    action: &str,
    modern: bool,
) -> Value {
    if modern {
        transport.request(&super::skills::modern(
            id,
            "tools/call",
            json!({"name":"history_configure","arguments":args,
                "requestState":form["result"]["requestState"],
                "inputResponses":{"confirmation":{"action":action}}}),
            json!({"elicitation":{"form":{}}}),
        ))
    } else {
        transport.request(
            &json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":action,"content":{}}}),
        )
    }
}

fn settings(transport: &Transport, id: i64) -> Value {
    let response = transport.call(json!(id), "status", &json!({}));
    response["result"]["structuredContent"]["data"]["history"].clone()
}

#[test]
fn legacy_admin_busy_and_cancelled_forms_release_review_slots() {
    let (server, _project) = fixture(Some("codex"));
    let transport = connect(server);
    let args = json!({"enabled":false});
    let held = begin(&transport, 2, &args, false);
    for id in 3..13 {
        let busy = transport.call(json!(id), "history_configure", &args);
        assert_eq!(busy["error"]["data"]["code"], "human_input_busy", "{busy}");
        assert_eq!(busy["error"]["data"]["requestId"], 2, "{busy}");
    }
    let cancel = |id, form: &Value| {
        transport.send(
            &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}),
        );
        let notice = transport.receive();
        assert_eq!(notice["method"], "notifications/cancelled", "{notice}");
        assert_eq!(notice["params"]["requestId"], form["id"], "{notice}");
    };
    cancel(2, &held);
    for id in 13..23 {
        let form = begin(&transport, id, &args, false);
        cancel(id, &form);
    }
    assert_eq!(settings(&transport, 23)["enabled"], true);
    let final_form = begin(&transport, 24, &args, false);
    let accepted = answer(&transport, 25, &args, &final_form, "accept", false);
    assert_eq!(accepted["result"]["isError"], false, "{accepted}");
    assert_eq!(settings(&transport, 26)["enabled"], false);
}

fn overlapping_reviews(modern: bool) {
    let (server, project) = fixture(Some("codex"));
    let transport = connect(server);
    let other = connect(Server::for_review(project.review.as_ref().clone()).unwrap());
    let args = json!({"enabled":false});
    let first = begin(&transport, 10, &args, modern);
    // Legacy stdio permits one outstanding elicitation. Modern input-required
    // continuations can overlap that form on the same Shield connection.
    let declined = begin(&transport, 11, &args, true);
    let decline = answer(&transport, 12, &args, &declined, "decline", true);
    assert_eq!(
        decline["result"]["structuredContent"]["error"]["code"], "approval_declined",
        "{decline}"
    );

    // A separate human review changes an unspecified setting. The next call
    // with identical arguments now describes a materially different snapshot.
    let change = json!({"limit":1000});
    let form = begin(&other, 20, &change, false);
    let changed = answer(&other, 21, &change, &form, "accept", false);
    assert!(
        changed["result"]["structuredContent"]["error"].is_null(),
        "{changed}"
    );
    let later = begin(&transport, 22, &args, true);
    let accepted_old = answer(&transport, 23, &args, &first, "accept", modern);
    assert!(
        !accepted_old["result"]["structuredContent"]["error"].is_null(),
        "{accepted_old}"
    );
    let after_old = settings(&other, 24);
    assert_eq!(after_old["enabled"], true, "{after_old}");
    assert_eq!(after_old["limit"], 1000, "{after_old}");

    // Rejecting the old snapshot must neither consume nor authorize the newer
    // retained plan; only acceptance of the new form can perform its change.
    let accepted_new = answer(&transport, 25, &args, &later, "accept", true);
    assert!(
        accepted_new["result"]["structuredContent"]["error"].is_null(),
        "{accepted_new}"
    );
    let after_new = settings(&other, 26);
    assert_eq!(after_new["enabled"], false, "{after_new}");
    assert_eq!(after_new["limit"], 1000, "{after_new}");
}

#[test]
fn admin_parity_legacy_forms_cannot_commit_a_different_review_with_identical_arguments() {
    overlapping_reviews(false);
}

#[test]
fn admin_parity_modern_forms_cannot_commit_a_different_review_with_identical_arguments() {
    overlapping_reviews(true);
}
