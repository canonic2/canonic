//! Native requests share inspection and human-confirmed denial over both wire protocols.
use super::*;

fn prepared(
    forms: bool,
) -> (
    Transport,
    assert_fs::TempDir,
    crate::review::Context,
    Vec<String>,
) {
    let (root, context, _) = crate::review::session_grants::native_fixture();
    let active = crate::review::runtime::current_identity(&context);
    let identity = crate::review::hook_approval::Identity {
        activation_id: active["activation_id"].as_str().unwrap(),
        policy_revision: active["policy_revision"].as_str().unwrap(),
        candidate_digest: active["candidate_digest"].as_str().unwrap(),
    };
    let ids = ["acme first", "acme second"]
        .map(|action| {
            let key = crate::review::hook_approval::Key {
                agent: "codex",
                session: "acme-session",
                cwd: &context.project_root,
                tool: "Bash",
                input_digest: crate::policy::fingerprint(action.as_bytes()),
            };
            let summary = crate::review::hook_approval::Summary {
                action: action.into(),
                reason: "the active policy asks for approval".into(),
                overrides_deny: false,
                deny_rules: Vec::new(),
                settings_change: false,
                policy_change: false,
            };
            match crate::review::hook_approval::refused(root.path(), &key, &identity, &summary)
                .unwrap()
            {
                crate::review::hook_approval::Outcome::Pending(id, _) => id,
                crate::review::hook_approval::Outcome::Allowed => {
                    panic!("fixture request must wait for approval")
                }
            }
        })
        .to_vec();
    let transport = Transport::start(Server::for_review(context.clone()).unwrap());
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
    (transport, root, context, ids)
}

fn pending(transport: &Transport, id: i64, request_id: &str) -> bool {
    let response = transport.call(json!(id), "request_list", &json!({}));
    response["result"]["structuredContent"]["data"]["requests"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["request_id"] == request_id)
}

#[test]
fn request_parity_legacy_denial_requires_human_acceptance_and_keeps_other_requests() {
    for action in ["accept", "decline", "cancel"] {
        let (transport, _root, _context, ids) = prepared(true);
        let shown = transport.call(json!(2), "request_show", &json!({"request_id":ids[0]}));
        assert_eq!(
            shown["result"]["structuredContent"]["data"]["request_id"],
            ids[0]
        );
        let form = transport.call(json!(3), "request_deny", &json!({"request_id":ids[0]}));
        assert_eq!(form["method"], "elicitation/create", "{form}");
        assert!(
            form["params"]["message"]
                .as_str()
                .unwrap()
                .contains("acme first")
        );
        let result = transport.request(
            &json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":action,"content":{}}}),
        );
        assert_eq!(
            result["result"]["structuredContent"]["error"].is_null(),
            action == "accept",
            "{result}"
        );
        assert_eq!(pending(&transport, 4, &ids[0]), action != "accept");
        assert!(pending(&transport, 5, &ids[1]));
    }
    let (transport, _root, _context, ids) = prepared(false);
    let result = transport.call(json!(3), "request_deny", &json!({"request_id":ids[0]}));
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"], "approval_unavailable",
        "{result}"
    );
    assert!(pending(&transport, 4, &ids[0]));
}

fn modern(id: i64, request_id: &str, state: Option<&Value>, action: Option<&str>) -> Value {
    let mut params = json!({"name":"request_deny","arguments":{"request_id":request_id}});
    if let Some(state) = state {
        params["requestState"] = state.clone();
    }
    if let Some(action) = action {
        params["inputResponses"] = json!({"confirmation":{"action":action}});
    }
    super::skills::modern(id, "tools/call", params, json!({"elicitation":{"form":{}}}))
}

#[test]
fn request_parity_modern_denial_binds_the_exact_request_and_consumes_continuation() {
    for action in ["accept", "decline", "cancel"] {
        let (transport, _root, _context, ids) = prepared(false);
        let form = transport.request(&modern(10, &ids[0], None, None));
        assert_eq!(form["result"]["resultType"], "input_required", "{form}");
        let token = &form["result"]["requestState"];
        let result = transport.request(&modern(11, &ids[0], Some(token), Some(action)));
        assert_eq!(result["result"]["resultType"], "complete", "{result}");
        assert_eq!(pending(&transport, 12, &ids[0]), action != "accept");
        assert!(pending(&transport, 13, &ids[1]));
        let replay = transport.request(&modern(14, &ids[0], Some(token), Some("accept")));
        assert_eq!(replay["error"]["code"], rpc::INVALID_PARAMS, "{replay}");
    }
    let (transport, _root, _context, ids) = prepared(false);
    let form = transport.request(&modern(10, &ids[0], None, None));
    let changed = transport.request(&modern(
        11,
        &ids[1],
        Some(&form["result"]["requestState"]),
        Some("accept"),
    ));
    assert_eq!(changed["error"]["code"], rpc::INVALID_PARAMS, "{changed}");
    assert!(pending(&transport, 12, &ids[0]));
    assert!(pending(&transport, 13, &ids[1]));
}
