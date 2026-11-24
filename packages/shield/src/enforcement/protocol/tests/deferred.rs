//! SDK receipt versions distinguish raw completion publication from a verified link.
use super::*;

#[test]
fn completion_history_failure_retains_a_diagnostic_category() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let path = root.path().canonicalize().unwrap();
    let lock = path.join(".shield/state/history.lock");
    fs::remove_file(&lock).unwrap();
    fs::create_dir(&lock).unwrap();
    let input = json!({"protocol_version":1,"request_id":"acme-callback",
        "expected_project_id":context.project_id,"cwd":path,
        "operation":{"kind":"completion","delivery_id":"acme-delivery","request_id":null,
            "status":"completed","operation":{"kind":"shell","source":"git status",
                "dialect":"posix","dialect_version":null}},
        "host":{"agent":"codex","session_id":"acme-session","tool_use_id":"acme-call",
            "event":"PostToolUse","consumer_id":null},"approval_request_id":null});
    let stages = std::cell::RefCell::new(Vec::new());
    let (receipt, exit) = evaluate_native(
        root.path(),
        &serde_json::to_vec(&input).unwrap(),
        &json!({}),
        NativeContext {
            integration: "codex",
            approval: NativeApproval::Fallback,
            context_complete: true,
            working_files: None,
        },
        None,
        &|stage| stages.borrow_mut().push(stage),
    );
    assert_eq!(exit, 0, "{receipt}");
    assert_eq!(receipt["code"], "completion-observed");
    assert_eq!(
        receipt["history"],
        json!({"status":"failed","request_id":null,"event_id":null})
    );
    assert!(stages.borrow().contains(&"history_failed_invalid"));
}

#[test]
fn sdk_v2_completion_receipts_defer_links_and_v1_remains_compatible() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let root_path = root.path().canonicalize().unwrap();
    for version in [1, 2] {
        for reported in [Value::Null, json!("unverified-request")] {
            let input = json!({"protocol_version":version,"request_id":"acme-callback","expected_project_id":context.project_id,
                "cwd":root_path,"operation":{"kind":"completion","delivery_id":format!("delivery-{version}-{reported}"),
                    "request_id":reported,"status":"completed","operation":{"kind":"read-text","path":root_path.join("acme.txt"),"offset":null,"limit":null}},
                "host":{"agent":"claude-sdk","session_id":"acme-session","tool_use_id":"acme-callback","event":"PostToolUse","consumer_id":"acme-consumer"},
                "approval_request_id":null});
            let (receipt, exit) = evaluate(root.path(), &serde_json::to_vec(&input).unwrap());
            assert_eq!(exit, 0, "{receipt}");
            assert_eq!(receipt["protocol_version"], version);
            assert_eq!(receipt["code"], "completion-observed");
            assert_eq!(receipt["decision"], "deny");
            assert!(receipt["identity"].is_null());
            assert!(receipt["operation"].is_null());
            assert_eq!(receipt["history"]["status"], "recorded");
            assert!(receipt["history"]["request_id"].is_null());
            assert!(receipt["history"]["event_id"].is_string());
            if version == 2 {
                assert_eq!(receipt["history"].as_object().unwrap().len(), 5);
                assert_eq!(receipt["history"]["correlation"], "deferred");
                assert_eq!(receipt["history"]["reported_request_id"], reported);
            } else {
                assert_eq!(receipt["history"].as_object().unwrap().len(), 3);
                assert!(receipt["history"].get("correlation").is_none());
            }
        }
    }
}

#[test]
fn sdk_v2_registration_cannot_claim_completion_correlation() {
    let root = assert_fs::TempDir::new().unwrap();
    let bundle =
        crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None).unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let input = json!({"protocol_version":2,"request_id":"registration","expected_project_id":context.project_id,
        "cwd":root.path(),"operation":{"kind":"register"},"host":null,"approval_request_id":null});
    let (receipt, exit) = evaluate(root.path(), &serde_json::to_vec(&input).unwrap());
    assert_eq!(exit, 0, "{receipt}");
    assert_eq!(receipt["protocol_version"], 2);
    assert!(receipt["identity"]["registry_revision"].is_string());
    assert_eq!(receipt["identity"].as_object().unwrap().len(), 7);
    assert_eq!(
        receipt["history"],
        json!({"status":"not-requested","request_id":null,"event_id":null,
        "correlation":"not-applicable","reported_request_id":null})
    );
    let mut legacy = input;
    legacy["protocol_version"] = json!(1);
    let (receipt, exit) = evaluate(root.path(), &serde_json::to_vec(&legacy).unwrap());
    assert_eq!(exit, 0);
    assert_eq!(receipt["identity"].as_object().unwrap().len(), 6);
    assert!(receipt["identity"].get("registry_revision").is_none());
}
