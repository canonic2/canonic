//! Pack forms through the shipping driver: answers prepare, final approval activates.
use super::*;
mod installation;

fn prepared(agent: &str, capabilities: &Value) -> (Transport, Project) {
    prepared_with_options(agent, capabilities, test_options(), VERSION)
}

fn prepared_with_options(
    agent: &str,
    capabilities: &Value,
    options: stdio::Options,
    version: &str,
) -> (Transport, Project) {
    let (server, project) = fixture(Some(agent));
    let source = std::path::Path::new(&project.review.project_root).join("registry");
    std::fs::create_dir(&source).unwrap();
    let source = source.canonicalize().unwrap();
    let bytes = include_str!("../../../fixtures/elicitation/pack.yml");
    std::fs::write(source.join("pack.yml"), bytes).unwrap();
    std::fs::write(source.join("catalog.json"), json!({"schema_version":1,"packs":[{"id":"acme/settings","version":"1.0.0","path":"pack.yml","sha256":crate::policy::fingerprint(bytes.as_bytes())}]}).to_string()).unwrap();
    crate::review::mcp::register_fixture(&project.review, &source);
    let connection = crate::review::mcp::Connection::new("install".into());
    let result = crate::review::mcp::call(
        &project.review,
        &connection,
        "policy_propose",
        &json!({"change":{"kind":"pack_add","reference":"fixture/acme/settings"}}),
        &|| Ok(()),
    );
    assert!(result["error"].is_null(), "{result}");
    let result = crate::review::mcp::call_with_confirmation(
        &project.review,
        &connection,
        "policy_apply",
        &result["data"]["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(result["error"].is_null(), "{result}");
    let transport = Transport::with_options(server, options);
    let mut messages = initialize(capabilities, agent);
    messages[0]["params"]["protocolVersion"] = json!(version);
    for message in messages {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    (transport, project)
}

fn args() -> Value {
    json!({"reference":"acme/settings","variables":["manager","regions","enabled","label"]})
}
fn values() -> Value {
    json!({"manager":"just","regions":["east","west"],"enabled":true,"label":"Acme"})
}
fn answer(form: &Value, action: &str, content: Value) -> Value {
    let mut response = json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":action}});
    response["result"]["content"] = content;
    response
}
fn data(response: &Value) -> &Value {
    &response["result"]["structuredContent"]
}
fn proposals(project: &Project) -> usize {
    crate::review::proposals(&project.review, None, false).unwrap()["proposals"]
        .as_array()
        .unwrap()
        .len()
}

#[test]
fn pack_forms_prepare_only_then_require_separate_adapter_approval() {
    for agent in ["claude", "codex"] {
        let (transport, project) = prepared(agent, &json!({"elicitation":{"form":{}}}));
        let before = project.revision();
        let count = proposals(&project);
        let form = transport.call(json!(10), "pack_configure", &args());
        assert_eq!(form["method"], "elicitation/create", "{form}");
        assert_eq!(
            form["params"]["requestedSchema"]["properties"]["regions"]["type"],
            "array"
        );
        assert!(
            form["params"]["requestedSchema"]["properties"]["manager"]
                .get("default")
                .is_none()
        );
        assert_eq!(proposals(&project), count);
        let proposal = transport.request(&answer(&form, "accept", values()));
        assert!(data(&proposal)["error"].is_null(), "{proposal}");
        assert_eq!(project.revision(), before);
        assert_eq!(proposals(&project), count + 1);
        let apply = &data(&proposal)["data"]["apply_arguments"];
        let result = transport.call(json!(11), "policy_apply", apply);
        if agent == "claude" {
            assert_eq!(result["method"], "elicitation/create");
            assert_eq!(project.revision(), before);
            let result = transport.request(&answer(&result, "accept", json!({})));
            assert!(data(&result)["error"].is_null(), "{result}");
        } else {
            assert!(data(&result)["error"].is_null(), "{result}");
            assert_eq!(
                data(&result)["data"]["operator"]["mechanism"],
                "host-tool-permission"
            );
        }
        assert_ne!(project.revision(), before);
    }
}

#[test]
fn pack_forms_unknown_variables_refuse_before_asking_or_preparing() {
    let (transport, project) = prepared("claude", &json!({"elicitation":{"form":{}}}));
    let before = project.revision();
    let count = proposals(&project);
    let result = transport.call(
        json!(10),
        "pack_configure",
        &json!({
            "reference":"acme/settings","variables":["manager","missing"]
        }),
    );
    assert_eq!(result["id"], 10, "{result}");
    assert_eq!(data(&result)["error"]["code"], "invalid_input", "{result}");
    let message = data(&result)["error"]["message"].as_str().unwrap();
    assert!(message.contains("unknown variable") && message.contains("pack_list"));
    assert!(!message.contains("policy_propose"));
    assert_eq!(project.revision(), before);
    assert_eq!(proposals(&project), count);
}

#[test]
fn pack_forms_refuse_decline_dismissal_invalid_answers_and_missing_capability() {
    for (action, content, code) in [
        ("decline", json!({}), "input_declined"),
        ("cancel", json!({}), "input_cancelled"),
        ("accept", json!({}), "invalid_input"),
        ("accept", json!({"manager":"other"}), "invalid_input"),
        (
            "accept",
            json!({"manager":"just","regions":["east","east"],"enabled":true,"label":"Acme"}),
            "invalid_input",
        ),
        (
            "accept",
            json!({"manager":"just","regions":["east"],"enabled":"true","label":"Acme"}),
            "invalid_input",
        ),
    ] {
        let (transport, project) = prepared("claude", &json!({"elicitation":{"form":{}}}));
        let before = project.revision();
        let count = proposals(&project);
        let form = transport.call(json!(10), "pack_configure", &args());
        let result = transport.request(&answer(&form, action, content));
        assert_eq!(data(&result)["error"]["code"], code, "{result}");
        assert_eq!(project.revision(), before);
        assert_eq!(proposals(&project), count);
    }
    let (transport, project) = prepared("claude", &json!({}));
    let count = proposals(&project);
    let result = transport.call(json!(10), "pack_configure", &args());
    assert_eq!(
        data(&result)["error"]["code"],
        "input_unavailable",
        "{result}"
    );
    assert_eq!(proposals(&project), count);
}

fn modern(id: i64, arguments: Value, state: Option<Value>, responses: Option<Value>) -> Value {
    let mut params = json!({"name":"pack_configure"});
    params["arguments"] = arguments;
    if let Some(state) = state {
        params["requestState"] = state;
    }
    if let Some(responses) = responses {
        params["inputResponses"] = responses;
    }
    super::skills::modern(id, "tools/call", params, json!({"elicitation":{"form":{}}}))
}

#[test]
fn pack_forms_modern_partial_invalid_replay_and_argument_binding() {
    let (transport, project) = prepared("codex", &json!({}));
    let before = project.revision();
    let count = proposals(&project);
    let first = transport.request(&modern(10, args(), None, None));
    assert_eq!(first["result"]["resultType"], "input_required", "{first}");
    let state = first["result"]["requestState"].clone();
    let missing = transport.request(&modern(11, args(), Some(state.clone()), Some(json!({}))));
    assert_eq!(
        missing["result"]["resultType"], "input_required",
        "{missing}"
    );
    let invalid = transport.request(&modern(
        12,
        args(),
        Some(state.clone()),
        Some(json!({"settings":{"action":"accept","content":{}}})),
    ));
    assert_eq!(invalid["error"]["code"], -32602, "{invalid}");
    assert_eq!(proposals(&project), count);
    let answers = json!({"settings":{"action":"accept","content":values()}});
    let accepted = transport.request(&modern(
        13,
        args(),
        Some(state.clone()),
        Some(answers.clone()),
    ));
    assert_eq!(accepted["result"]["resultType"], "complete", "{accepted}");
    assert_eq!(proposals(&project), count + 1);
    assert_eq!(project.revision(), before);
    let replay = transport.request(&modern(14, args(), Some(state), Some(answers.clone())));
    assert_eq!(replay["error"]["code"], -32602);
    let first = transport.request(&modern(15, args(), None, None));
    let mut changed = args();
    changed["layer"] = json!("local");
    let changed = transport.request(&modern(
        16,
        changed,
        Some(first["result"]["requestState"].clone()),
        Some(answers),
    ));
    assert_eq!(changed["error"]["code"], -32602);
    assert_eq!(proposals(&project), count + 1);
}

#[test]
fn pack_forms_revalidate_pack_constraints_and_active_revision() {
    let (transport, project) = prepared("claude", &json!({"elicitation":{"form":{}}}));
    let count = proposals(&project);
    let form = transport.call(json!(10), "pack_configure", &args());
    let mut content = values();
    content["label"] = json!("x".repeat(21));
    let result = transport.request(&answer(&form, "accept", content));
    assert!(!data(&result)["error"].is_null(), "{result}");
    assert_eq!(proposals(&project), count);
    let form = transport.call(json!(11), "pack_configure", &args());
    let change = json!({"change":{"kind":"rule_add","rule":{"id":"other","decision":"ask","conditions":{"domain":"argv","executable":"other","arguments":{"kind":"any"}}}}});
    let result = transport.call(json!(12), "policy_propose", &change);
    let applied = crate::review::mcp::call_with_confirmation(
        &project.review,
        &crate::review::mcp::Connection::new("other".into()),
        "policy_apply",
        &data(&result)["data"]["apply_arguments"],
        &|| Ok(()),
        true,
    );
    assert!(applied["error"].is_null(), "{applied}");
    let before = project.revision();
    let result = transport.request(&answer(&form, "accept", values()));
    assert_eq!(data(&result)["error"]["code"], "stale_revision", "{result}");
    assert_eq!(data(&result)["error"]["operator_route"], "pack_configure");
    let message = data(&result)["error"]["message"].as_str().unwrap();
    assert!(message.contains("pack_configure again") && message.contains("re-answer"));
    assert!(!message.contains("policy_propose"));
    assert_eq!(project.revision(), before);
    assert_eq!(proposals(&project), count + 1);
}

#[test]
fn pack_forms_cancellation_disconnect_and_busy_slot_create_no_proposals() {
    for disconnect in [false, true] {
        let (transport, project) = prepared("claude", &json!({"elicitation":{"form":{}}}));
        let before = project.revision();
        let count = proposals(&project);
        let form = transport.call(json!(10), "pack_configure", &args());
        let second = transport.call(json!(11), "pack_configure", &args());
        assert_eq!(
            data(&second)["error"]["code"],
            "input_unavailable",
            "{second}"
        );
        if disconnect {
            let (_, result) = transport.close();
            assert!(result.is_ok());
        } else {
            transport.send(&json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10}}));
            let withdrawn = transport.receive();
            assert_eq!(withdrawn["method"], "notifications/cancelled");
            transport.send(&answer(&form, "accept", values()));
            let response = transport.call(json!(12), "status", &json!({}));
            assert_eq!(response["id"], 12, "{response}");
        }
        assert_eq!(project.revision(), before);
        assert_eq!(proposals(&project), count);
    }
}

#[test]
fn pack_forms_timeout_withdraws_input_and_ignores_late_acceptance() {
    let mut options = test_options();
    options.elicitation.timeout = Duration::from_millis(500);
    let (transport, project) = prepared_with_options(
        "claude",
        &json!({"elicitation":{"form":{}}}),
        options,
        VERSION,
    );
    let count = proposals(&project);
    let before = project.revision();
    let form = transport.call(json!(10), "pack_configure", &args());
    assert_eq!(form["method"], "elicitation/create", "{form}");
    let withdrawn = transport.receive();
    assert_eq!(
        withdrawn["method"], "notifications/cancelled",
        "{withdrawn}"
    );
    let timeout = transport.receive();
    assert_eq!(timeout["error"]["code"], -32001, "{timeout}");
    transport.send(&answer(&form, "accept", values()));
    let response = transport.call(json!(11), "status", &json!({}));
    assert_eq!(response["id"], 11, "{response}");
    assert_eq!(project.revision(), before);
    assert_eq!(proposals(&project), count);
}

#[test]
fn pack_forms_modern_capacity_rounds_capability_isolation_and_concurrent_replay() {
    let (transport, project) = prepared("codex", &json!({}));
    let count = proposals(&project);
    let first = transport.request(&modern(10, args(), None, None));
    let state = first["result"]["requestState"].clone();
    for id in 11..18 {
        let response = transport.request(&modern(id, args(), None, None));
        assert_eq!(
            response["result"]["resultType"], "input_required",
            "{response}"
        );
    }
    let full = transport.request(&modern(18, args(), None, None));
    assert_eq!(full["error"]["code"], -32602, "{full}");
    let mut no_capability = modern(
        19,
        args(),
        Some(state.clone()),
        Some(json!({"settings":{"action":"accept","content":values()}})),
    );
    no_capability["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"] = json!({});
    let refused = transport.request(&no_capability);
    assert_eq!(refused["error"]["code"], -32021, "{refused}");
    assert_eq!(proposals(&project), count);
    let answers = json!({"settings":{"action":"accept","content":values()}});
    transport.send(&modern(
        20,
        args(),
        Some(state.clone()),
        Some(answers.clone()),
    ));
    transport.send(&modern(21, args(), Some(state), Some(answers)));
    let responses = [transport.receive(), transport.receive()];
    assert_eq!(
        responses
            .iter()
            .filter(|response| response["result"]["resultType"] == "complete")
            .count(),
        1,
        "{responses:?}"
    );
    assert_eq!(
        responses
            .iter()
            .filter(|response| response["error"]["code"] == -32602)
            .count(),
        1
    );
    assert_eq!(proposals(&project), count + 1);
    let next = transport.request(&modern(22, args(), None, None));
    let state = next["result"]["requestState"].clone();
    for id in 23..30 {
        let response = transport.request(&modern(id, args(), Some(state.clone()), Some(json!({}))));
        assert_eq!(
            response["result"]["resultType"], "input_required",
            "{response}"
        );
    }
    let expired_rounds = transport.request(&modern(30, args(), Some(state), Some(json!({}))));
    assert_eq!(expired_rounds["error"]["code"], -32602, "{expired_rounds}");
}

#[test]
fn pack_forms_earliest_revision_preserves_primitive_forms_and_refuses_multiple_choices() {
    let (transport, project) = prepared_with_options(
        "codex",
        &json!({"elicitation":{}}),
        test_options(),
        "2025-06-18",
    );
    let before = project.revision();
    let count = proposals(&project);
    let unsupported = transport.call(json!(10), "pack_configure", &args());
    assert_eq!(
        data(&unsupported)["error"]["code"],
        "input_unavailable",
        "{unsupported}"
    );
    assert_eq!(proposals(&project), count);
    let form = transport.call(
        json!(11),
        "pack_configure",
        &json!({"reference":"acme/settings","variables":["manager","enabled","label"]}),
    );
    assert_eq!(form["method"], "elicitation/create", "{form}");
    assert!(form["params"].get("mode").is_none());
    let result = transport.request(&answer(
        &form,
        "accept",
        json!({"manager":"just","enabled":true,"label":"Acme"}),
    ));
    assert!(data(&result)["error"].is_null(), "{result}");
    assert_eq!(proposals(&project), count + 1);
    assert_eq!(project.revision(), before);
}
