//! Native lifecycle callbacks retain vendor provenance and only link exact calls.
use super::*;

fn check(
    agent: &str,
    names: (&str, &str),
    payloads: &(Value, Value),
    root: &Path,
    context: &review::Context,
    callback: Event,
    failed: bool,
) {
    let send = |event, input: &Value| {
        translate_for(root, agent, event, &serde_json::to_vec(input).unwrap())
    };
    send(Event::Before, &payloads.0);
    let reply = send(callback, &payloads.1);
    assert!(reply.get("systemMessage").is_none(), "{agent}: {reply}");
    if agent == "opencode" {
        assert_eq!(reply["decision"], "observed", "{reply}");
    }

    let events: Vec<Value> = journal(root)
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["event"].take())
        .filter(|event: &Value| matches!(event["kind"].as_str(), Some("decision" | "completion")))
        .collect();
    let [decision, completion] = events.as_slice() else {
        panic!("{agent}: expected one decision and completion, got {events:?}");
    };
    assert_eq!(decision["kind"], "decision");
    assert_eq!(decision["details"]["host_decision"], "allow", "{decision}");
    assert_eq!(decision["provenance"]["host_event_name"], names.0);
    assert_eq!(completion["kind"], "completion");
    assert_eq!(completion["provenance"]["agent"], agent);
    assert_eq!(completion["provenance"]["host_event_name"], names.1);
    assert_eq!(
        completion["details"]["operation"],
        decision["details"]["operation"]
    );
    assert_eq!(completion["details"]["evidence"], "host-reported");
    assert_eq!(
        completion["details"].get("status"),
        failed.then(|| json!("failed")).as_ref()
    );
    assert!(completion["details"]["reported_request_id"].is_null());

    let history = review::mcp::call(
        context,
        &review::mcp::Connection::new("native-vendor-history".into()),
        "history_list",
        &json!({}),
        &|| Ok(()),
    );
    let entry = &history["data"]["entries"][0];
    if agent == "opencode" {
        assert_eq!(entry["observations_total"], 2, "{history}");
        assert_eq!(entry["outcome"]["evidence"], "host-reported");
    } else {
        assert!(completion["provenance"]["tool_use_id"].is_null());
        assert_eq!(entry["observations_total"], 1, "{history}");
        assert_eq!(entry["outcome"], "unobserved");
    }
}

#[test]
fn native_completion_history_records_google_successes_and_failures() {
    for agent in ["gemini", "agy"] {
        for failed in [false, true] {
            let (root, context) = fixture();
            let path = root.path().join("acme.txt");
            fs::write(&path, "Acme").unwrap();
            let (names, before, after) = if agent == "gemini" {
                let before = json!({"hook_event_name":"BeforeTool","session_id":"acme-session",
                    "cwd":root.path(),"tool_name":"read_file","tool_input":{"file_path":path}});
                let mut after = before.clone();
                after["hook_event_name"] = json!("AfterTool");
                after["tool_response"] = if failed {
                    json!({"error":{"message":"Acme read failed"}})
                } else {
                    json!({"llmContent":"Acme"})
                };
                (("BeforeTool", "AfterTool"), before, after)
            } else {
                let before = json!({"conversationId":"acme-session","workspacePaths":[root.path()],
                    "stepIdx":2,"toolCall":{"name":"read_file","args":{"Path":path}}});
                let mut after = before.clone();
                if failed {
                    after["error"] = json!("Acme read failed");
                }
                (("PreToolUse", "PostToolUse"), before, after)
            };
            check(
                agent,
                names,
                &(before, after),
                root.path(),
                &context,
                Event::Completed,
                failed,
            );
        }
    }
}

#[test]
fn native_completion_history_records_copilot_success_and_failure() {
    for failed in [false, true] {
        let (root, context) = fixture();
        let path = root.path().join("acme.txt");
        fs::write(&path, "Acme").unwrap();
        let before = json!({"sessionId":"acme-session","timestamp":1,"cwd":root.path(),
            "toolName":"view","toolArgs":{"path":path}});
        let mut after = before.clone();
        let (name, callback) = if failed {
            after["error"] = json!("Acme read failed");
            ("postToolUseFailure", Event::Failed)
        } else {
            after["toolResult"] = json!({"resultType":"success"});
            ("postToolUse", Event::Completed)
        };
        check(
            "copilot",
            ("preToolUse", name),
            &(before, after),
            root.path(),
            &context,
            callback,
            failed,
        );
    }
}

#[test]
fn native_completion_history_links_opencode_vendor_callbacks() {
    let (root, context) = fixture();
    let path = root.path().join("acme.txt");
    fs::write(&path, "Acme").unwrap();
    let before = json!({"protocol_version":1,"hook_event_name":"tool.execute.before",
        "sessionID":"acme-session","callID":"acme-call","cwd":root.path(),
        "tool":"read","arguments":{"filePath":path}});
    let mut after = before.clone();
    after["hook_event_name"] = json!("tool.execute.after");
    check(
        "opencode",
        ("tool.execute.before", "tool.execute.after"),
        &(before, after),
        root.path(),
        &context,
        Event::Completed,
        false,
    );
}
