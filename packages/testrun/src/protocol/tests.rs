//! MCP handshake, failure envelopes and ordered image contracts.
use super::presentation::image_attachments;
pub(super) fn ready_protocol() -> Protocol {
    let mut protocol = Protocol::default();
    let incoming = protocol.receive(br#"{"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme fixture","version":"1"}}}"#);
    let canonic_mcp_kit::Inbound::Initialize { id, .. } = incoming else {
        panic!("Fixture initializes");
    };
    protocol.session.lock().unwrap().accept_initialize(
        id,
        &canonic_mcp_kit::ServerInfo::new("canonic-testrun", "fixture"),
    );
    protocol.receive(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    protocol.connection_id = "fixture".into();
    protocol
}
#[test]
fn large_fixture_mcp_request_fits_the_catalog_and_stdio_transport() {
    use crate::browser::transfer::FIXTURE_BYTES;
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let fixture = json!({
        "name":"acme.bin","mimeType":"application/octet-stream",
        "dataBase64":STANDARD.encode(vec![42; FIXTURE_BYTES]),
        "provenance":"synthetic_generated","cleared":{"by":"Acme","method":"Generated"}
    });
    let target = format!("browser:{}", uuid::Uuid::new_v4());
    for (name, args) in [
        (
            "browser_send",
            json!({"targetId":target,"request":{"operation":"fixture_register","fixture":fixture}}),
        ),
        (
            "act",
            json!({"targetId":target,"actions":[{"action":"fixture_register","fixture":fixture}]}),
        ),
    ] {
        let frame = serde_json::to_vec(&json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":name,"arguments":args}
        }))
        .unwrap();
        match ready_protocol().receive(&frame) {
            canonic_mcp_kit::Inbound::Tool { .. } => {}
            canonic_mcp_kit::Inbound::InvalidArguments { error, .. } => panic!("{name}: {error}"),
            _ => panic!("{name}: fixture frame was refused before argument validation"),
        }
        let (route, args) = if name == "act" {
            crate::browser::api::translate(name, args).unwrap()
        } else {
            (name.to_owned(), args)
        };
        crate::browser::surface::validate(&route, &args).unwrap();
    }
}

#[test]
fn shared_catalog_accepts_every_published_schema() {
    let catalog =
        build_catalog().expect("Published Testrun schemas must be supported by canonic-mcp-kit");
    fn difference(left: &Value, right: &Value, path: &str) -> Option<String> {
        if left == right {
            return None;
        }
        match (left, right) {
            (Value::Object(left), Value::Object(right)) => {
                for (key, value) in left {
                    let Some(expected) = right.get(key) else {
                        return Some(format!("{path}/{key}: absent from source listing"));
                    };
                    if let Some(found) = difference(value, expected, &format!("{path}/{key}")) {
                        return Some(found);
                    }
                }
                right
                    .keys()
                    .find(|key| !left.contains_key(*key))
                    .map(|key| format!("{path}/{key}: absent from prepared listing"))
            }
            (Value::Array(left), Value::Array(right)) => {
                if left.len() != right.len() {
                    return Some(format!(
                        "{path}: prepared length {}; source length {}",
                        left.len(),
                        right.len()
                    ));
                }
                left.iter()
                    .zip(right)
                    .enumerate()
                    .find_map(|(index, (left, right))| {
                        difference(left, right, &format!("{path}/{index}"))
                    })
            }
            _ => Some(format!("{path}: prepared={left}; source={right}")),
        }
    }
    let source = json!(crate::catalog::tools());
    assert_eq!(
        difference(&catalog.listing()["tools"], &source, "/tools"),
        None,
        "Prepared catalog must preserve every published schema field"
    );
    assert_eq!(catalog.listing()["tools"], json!(crate::catalog::tools()));
    for name in ["run.begin", "session_open", "batch", "tap", "screenshot"] {
        assert!(catalog.get(name).is_none());
    }
}

#[test]
fn mcp_prompts_and_completions_remain_disabled_without_consuming_request_ids() {
    let protocol = ready_protocol();
    for (method, params) in [
        ("prompts/list", json!({})),
        ("prompts/get", json!({"name":"Acme"})),
        (
            "completion/complete",
            json!({"ref":{"type":"ref/prompt","name":"Acme"},"argument":{"name":"query","value":"Acme"}}),
        ),
    ] {
        let frame = serde_json::to_vec(
            &json!({"jsonrpc":"2.0","id":"Acme-prompt","method":method,"params":params}),
        )
        .unwrap();
        let canonic_mcp_kit::Inbound::Reply(reply) = protocol.receive(&frame) else {
            panic!("Unadvertised prompts must be rejected: {method}");
        };
        assert_eq!(reply["error"]["code"], -32601, "{reply}");
    }
    let canonic_mcp_kit::Inbound::Reply(reply) =
        protocol.receive(br#"{"jsonrpc":"2.0","id":"Acme-prompt","method":"ping"}"#)
    else {
        panic!("Refused prompts must not consume the request ID");
    };
    assert!(reply.get("result").is_some(), "{reply}");
}

#[test]
fn mcp_task_management_remains_disabled_without_consuming_request_ids() {
    let protocol = ready_protocol();
    for method in ["tasks/get", "tasks/result", "tasks/list", "tasks/cancel"] {
        let frame = serde_json::to_vec(
            &json!({"jsonrpc":"2.0","id":"Acme-task","method":method,"params":{"taskId":"Acme"}}),
        )
        .unwrap();
        let canonic_mcp_kit::Inbound::Reply(reply) = protocol.receive(&frame) else {
            panic!("Unadvertised task management must be rejected: {method}");
        };
        assert_eq!(reply["error"]["code"], -32601, "{reply}");
    }
    assert!(matches!(
        protocol.receive(br#"{"jsonrpc":"2.0","id":"Acme-task","method":"ping"}"#),
        canonic_mcp_kit::Inbound::Reply(_)
    ));
}

#[test]
fn browser_actions_use_browser_validation_and_shared_status_keeps_ordered_images() {
    let page = json!({"connectionId":uuid::Uuid::new_v4(),"id":uuid::Uuid::new_v4(),"generation":uuid::Uuid::new_v4()});
    let args = json!({"targetId":format!("browser:{}", page["connectionId"].as_str().unwrap()),"actions":[{"action":"page_list"}]});
    let tool = definition("act").unwrap();
    validate_arguments("act", &tool["inputSchema"], &args).unwrap();
    let data = json!({"actionId":uuid::Uuid::new_v4(),"operation":{"result":{"completedActions":2,"results":[{"attachment":{"path":"first.png","mimeType":"image/png"}},{"attachment":{"path":"second.jpeg","mimeType":"image/jpeg"}}]}}});
    for name in ["status", "act", "browser_status", "browser_send"] {
        let images = image_attachments(name, &data);
        let paths: Vec<_> = images
            .iter()
            .filter_map(|(_, image)| image["path"].as_str())
            .collect();
        assert_eq!(paths, vec!["first.png", "second.jpeg"]);
    }
    assert_eq!(responsive::timed_out("act", 5).code, "OUTCOME_UNKNOWN");
    assert_eq!(responsive::timed_out("connect", 5).code, "OUTCOME_UNKNOWN");
}

#[test]
fn outstanding_ids_cancel_silently_and_duplicates_are_fatal() {
    let protocol = ready_protocol();
    let call = br#"{"jsonrpc":"2.0","id":"work","method":"tools/call","params":{"name":"status","arguments":{}}}"#;
    assert!(matches!(
        protocol.receive(call),
        canonic_mcp_kit::Inbound::Tool { .. }
    ));
    assert!(matches!(protocol.receive(br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"work"}}"#), canonic_mcp_kit::Inbound::Cancel { .. }));
    assert!(!protocol.finish(&json!("work")));
    assert!(matches!(
        protocol.receive(call),
        canonic_mcp_kit::Inbound::Tool { .. }
    ));
    let canonic_mcp_kit::Inbound::Fatal { reply, .. } =
        protocol.receive(br#"{"jsonrpc":"2.0","id":"work","method":"ping"}"#)
    else {
        panic!("Outstanding duplicate must end the connection");
    };
    assert_eq!(reply["error"]["code"], -32600);
}
#[test]
fn only_explicit_execution_timeouts_start_a_request_deadline() {
    for name in [
        "ui_snapshot",
        "app_list",
        "screenshot",
        "run.end",
        "docs_search",
    ] {
        assert!(requested_deadline(name, &json!({})).is_none(), "{name}");
    }
    for name in ["wait", "network_set"] {
        assert!(requested_deadline(name, &json!({"timeoutMs":10000})).is_none());
    }
    let timeout = std::time::Duration::from_secs(10);
    let before = tokio::time::Instant::now();
    let deadline = requested_deadline("session_open", &json!({"timeoutMs":10000})).unwrap();
    assert!(deadline >= before + timeout && deadline <= tokio::time::Instant::now() + timeout);
}
#[test]
fn only_valid_task_end_callbacks_on_a_ready_connection_fence_on_receipt() {
    let ready = ready_protocol();
    for valid in [
        json!({"jsonrpc":"2.0","method":"notifications/testrun/task_end"}),
        json!({"jsonrpc":"2.0","method":"notifications/testrun/task_end","params":{}}),
        json!({"jsonrpc":"2.0","id":1,"method":"testrun/task_end"}),
        json!({"jsonrpc":"2.0","id":"Acme-end","method":"testrun/task_end","params":{}}),
    ] {
        assert!(ready.task_end(&valid), "{valid}");
        assert!(!Protocol::default().task_end(&valid), "{valid}");
    }
    for invalid in [
        json!({"jsonrpc":"2.0","method":"notifications/testrun/task_end","params":{"taskId":"Acme"}}),
        json!({"jsonrpc":"2.0","id":1,"method":"testrun/task_end","params":{"taskId":"Acme"}}),
        json!({"jsonrpc":"2.0","id":1,"method":"testrun/task_end","params":[]}),
        json!({"jsonrpc":"2.0","id":null,"method":"testrun/task_end"}),
        json!({"jsonrpc":"2.0","id":1,"method":"notifications/testrun/task_end"}),
        json!({"jsonrpc":"1.0","method":"notifications/testrun/task_end"}),
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}),
    ] {
        assert!(!ready.task_end(&invalid), "{invalid}");
    }
}
#[test]
fn image_attachments_are_bounded_without_discarding_evidence_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("observation.png");
    let data =
        json!({"artifactId":"acme-observation","attachment":{"path":path,"mimeType":"image/png"}});
    std::fs::write(&path, [137, 80, 78, 71]).unwrap();
    let rendered =
        presentation::tool_result("screenshot", Ok(data.clone()), std::time::Duration::ZERO);
    assert_eq!(rendered["isError"], false);
    assert_eq!(rendered["content"][1]["type"], "image");
    assert_eq!(rendered["content"][1]["data"], "iVBORw==");

    std::fs::File::create(&path)
        .unwrap()
        .set_len(64 * 1024 * 1024)
        .unwrap();
    let oversized =
        presentation::tool_result("screenshot", Ok(data.clone()), std::time::Duration::ZERO);
    assert_eq!(oversized["isError"], false);
    assert_eq!(oversized["structuredContent"]["data"], data);
    assert!(
        oversized["content"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["type"] == "text")
    );

    std::fs::remove_file(&path).unwrap();
    let missing =
        presentation::tool_result("screenshot", Ok(data.clone()), std::time::Duration::ZERO);
    assert_eq!(missing["structuredContent"]["data"], data);
    assert_eq!(missing["isError"], false);
}
#[test]
fn batch_images_are_ordered_and_final_alias_is_not_duplicated() {
    let data = serde_json::json!({"steps":[
        {"index":0,"tool":"tap","observation":{"attachment":{"path":"first"}}},
        {"index":1,"tool":"swipe","observation":{"attachment":{"path":"second"}}}
    ],"observation":{"attachment":{"path":"second"}}});
    let images = image_attachments("batch", &data);
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].1["path"], "first");
    assert_eq!(images[1].1["path"], "second");
    let partial = serde_json::json!({"steps":data["steps"],"observation":null});
    assert_eq!(image_attachments("batch", &partial).len(), 2);
    // A call naming its target returns the bare batch; its step images keep
    // their labels instead of becoming an interruption picture.
    let bare = image_attachments("act", &data);
    assert_eq!(bare.len(), 2);
    assert!(
        bare[0]
            .0
            .as_deref()
            .unwrap()
            .starts_with("Observation after step 0")
    );
}
#[test]
fn observe_images_attach_from_the_session_envelope_and_the_bare_screenshot() {
    let screenshot = serde_json::json!({"artifactId":"acme","attachment":{"path":"screen"}});
    let envelope = serde_json::json!({"state":"ready","observation":screenshot});
    for data in [&envelope, &screenshot] {
        let images = image_attachments("observe", data);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].1["path"], "screen");
    }
}
use super::*;
mod documentation;
mod onboarding;
mod skills;
#[tokio::test]
async fn shared_argument_errors_keep_their_message_and_tool_result_shape() {
    let mut server = Server::new();
    let mut protocol = ready_protocol();
    let response = protocol.request(&mut server, json!({"jsonrpc":"2.0","id":"invalid","method":"tools/call","params":{"name":"target_list","arguments":{"probe":"false"}}})).await.unwrap();
    assert_eq!(response["result"]["isError"], true);
    let result = &response["result"]["structuredContent"];
    assert_eq!(result["code"], "INVALID_ARGUMENT");
    assert_eq!(result["message"], "arguments.probe must be boolean");
    assert_eq!(
        serde_json::from_str::<Value>(response["result"]["content"][0]["text"].as_str().unwrap())
            .unwrap(),
        *result
    );
    assert!(!protocol.finish(&json!("invalid")));
}

#[tokio::test]
async fn mcp_fixture_path_refusal_points_to_the_cli() {
    let mut server = Server::new();
    let mut protocol = ready_protocol();
    let target = format!("browser:{}", uuid::Uuid::new_v4());
    let fixture = json!({"path":"acme-private.txt"});
    let registration = json!({"operation":"fixture_register","fixture":fixture});
    for (name, args) in [
        (
            "browser_send",
            json!({"targetId":target,"request":registration}),
        ),
        (
            "browser_send",
            json!({"targetId":target,"request":{"operation":"batch","requests":[registration]}}),
        ),
        (
            "act",
            json!({"targetId":target,"actions":[{"action":"fixture_register","fixture":fixture}]}),
        ),
    ] {
        let response = protocol.request(&mut server, json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}
        })).await.unwrap();
        let result = &response["result"]["structuredContent"];
        assert_eq!(result["code"], "INVALID_ARGUMENT", "{response}");
        assert!(
            result["message"]
                .as_str()
                .unwrap()
                .contains("testrun browser fixture-register PATH"),
            "{response}"
        );
        assert!(!response.to_string().contains("acme-private.txt"));
    }
}

#[tokio::test]
async fn session_operations_are_not_published_over_mcp() {
    let mut server = Server::new();
    let mut protocol = ready_protocol();
    for name in ["run.begin", "batch", "session_open", "tap", "screenshot"] {
        let response = protocol.request(&mut server,json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":{}}})).await.unwrap();
        assert_eq!(response["error"]["code"], -32602, "{name}");
    }
}

#[tokio::test]
async fn an_old_queued_request_can_still_enter_the_workflow() {
    let mut server = Server::new();
    let mut protocol = ready_protocol();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"status","arguments":{}}});
    let delayed = protocol
        .admitted_request(
            &mut server,
            request.clone(),
            Instant::now() - std::time::Duration::from_secs(31),
        )
        .await
        .unwrap();
    let current = protocol.request(&mut server, request).await.unwrap();
    assert_ne!(delayed["result"]["structuredContent"]["code"], "TIMEOUT");
    assert_eq!(
        delayed["result"]["structuredContent"]["code"],
        current["result"]["structuredContent"]["code"]
    );
}
#[tokio::test]
async fn handshake_and_error_boundaries() {
    let mut server = Server::new();
    let mut protocol = Protocol::default();
    let first = protocol
        .request(
            &mut server,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        )
        .await
        .unwrap();
    assert!(first.get("error").is_some());
    let initialized=protocol.request(&mut server,json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).await.unwrap();
    assert_eq!(initialized["result"]["protocolVersion"], VERSION);
    assert!(
        protocol
            .request(
                &mut server,
                json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .await
            .is_none()
    );
    let list = protocol
        .request(
            &mut server,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
        )
        .await
        .unwrap();
    assert!(!list["result"]["tools"].as_array().unwrap().is_empty());
    let failed=protocol.request(&mut server,json!({"jsonrpc":"2.0","id":"four","method":"tools/call","params":{"name":"act","arguments":{"actions":[]}}})).await.unwrap();
    assert_eq!(failed["id"], "four");
    assert_eq!(failed["result"]["isError"], true);
}
