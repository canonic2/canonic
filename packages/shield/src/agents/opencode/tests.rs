use super::*;
use std::collections::BTreeMap;

fn envelope(tool: &str, arguments: Value) -> Value {
    let mut host = json!({"protocol_version":1,"hook_event_name":"tool.execute.before","cwd":std::env::temp_dir(),"tool":tool,"sessionID":"opencode-session","callID":"fixture-call","shell":"bash"});
    host["arguments"] = arguments;
    host
}

#[test]
fn opencode_v1_input_keeps_workdir_session_call_and_shell_evidence() {
    let host = envelope(
        "bash",
        json!({"command":"pwd","description":"Print current directory","workdir":"nested"}),
    );
    let request = OpenCode.request(&host).unwrap();
    assert_eq!(request.tool_id, Some("fixture-call"));
    assert_eq!(request.session, "opencode-session");
    assert_eq!(
        request.cwd,
        std::env::temp_dir().join("nested").to_str().unwrap()
    );
    assert!(!request.context_complete);
    assert_eq!(request.operation.unwrap()["unmodeled"], true);
    let mut unknown = host.clone();
    unknown["shell"] = Value::Null;
    let request = OpenCode.request(&unknown).unwrap();
    assert!(!request.context_complete);
    assert_eq!(request.operation.unwrap()["unmodeled"], true);
    let mut completed = host;
    completed["hook_event_name"] = json!("tool.execute.after");
    assert!(matches!(
        OpenCode.request(&completed).unwrap().event,
        Event::Completed
    ));
    completed["protocol_version"] = json!(2);
    assert!(OpenCode.request(&completed).is_none());
}

#[test]
fn opencode_v1_mapped_arguments_refuse_unmodeled_fields_and_patch_input() {
    assert_eq!(
        OpenCode
            .request(&envelope("shield_status", json!({})))
            .unwrap()
            .shield_tool,
        Some("status")
    );
    assert!(
        OpenCode
            .request(&envelope("shield_unknown", json!({})))
            .is_none()
    );
    assert!(
        OpenCode
            .request(&envelope("multiedit", json!({})))
            .is_none()
    );
    let listing = OpenCode
        .request(&envelope("list", json!({"path":"src"})))
        .unwrap()
        .operation
        .unwrap();
    assert_eq!(listing["action"], "read");
    assert_eq!(listing["recursive"], true);
    for arguments in [
        json!({"command":"pwd","description":"Print folder","cwd":"elsewhere"}),
        json!({"command":"pwd","description":"Print folder","timeout":-1}),
    ] {
        assert!(OpenCode.request(&envelope("bash", arguments)).is_none());
    }
    assert!(
        OpenCode
            .request(&envelope("apply_patch", json!({"patchText":"anything"})))
            .is_none()
    );
    for pattern in ["../secret", "{..,safe}/secret", "@(..)/secret"] {
        assert!(
            OpenCode
                .request(&envelope("glob", json!({"pattern":pattern})))
                .is_none()
        );
    }
    assert_eq!(
        OpenCode
            .request(&envelope("read", json!({"filePath":"file.txt"})))
            .unwrap()
            .operation
            .unwrap()["path"],
        json!(std::env::temp_dir().join("file.txt"))
    );
    assert!(
        OpenCode
            .request(&envelope(
                "read",
                json!({"filePath":"file.txt","offset":"1"})
            ))
            .is_none()
    );
}

#[test]
fn opencode_v1_plugin_ownership_preserves_other_plugins_and_removes_exact_source() {
    let group = OpenCode.hook_group("ignored");
    let desired = BTreeMap::from([
        ("tool.execute.before".into(), group.clone()),
        ("tool.execute.after".into(), group),
    ]);
    let source = OpenCode
        .merge_hooks(None, &BTreeMap::new(), &desired)
        .unwrap();
    assert!(source.contains("binary:'shield'"));
    assert!(source.contains("protocol_version"));
    assert!(!source.contains("import { transport }"));
    assert_eq!(source.matches("export ").count(), 1);
    assert_eq!(
        OpenCode.capture_hooks(Some(&source), &desired).unwrap(),
        desired
    );
    assert!(
        OpenCode
            .merge_hooks(
                Some("export default async()=>({})"),
                &BTreeMap::new(),
                &desired
            )
            .is_err()
    );
    assert_eq!(
        OpenCode
            .merge_hooks(Some(&source), &desired, &BTreeMap::new())
            .unwrap(),
        ""
    );
    let before =
        json!({"plugin":["user-plugin"],"mcp":{"acme":{"type":"local","command":["acme"]}}})
            .to_string();
    let entry = OpenCode.mcp_entry(Path::new("ignored"), Path::new("ignored"), &[]);
    let installed = OpenCode
        .merge_mcp(Some(&before), None, Some(&entry))
        .unwrap();
    assert!(OpenCode.has_mcp_entry(&installed).unwrap());
    assert_eq!(
        serde_json::from_str::<Value>(
            &OpenCode
                .merge_mcp(Some(&installed), Some(&entry), None)
                .unwrap()
        )
        .unwrap(),
        serde_json::from_str::<Value>(&before).unwrap()
    );
    assert!(
        OpenCode
            .merge_mcp(Some("{\"plugins\":[]}"), None, Some(&entry))
            .is_err()
    );
}

#[test]
fn opencode_v1_standing_ask_defaults_to_deny_without_native_prompt() {
    assert!(matches!(
        OpenCode.default_ask_fallback(),
        crate::review::integrations::AskFallback::Block
    ));
    assert!(matches!(
        OpenCode.approval(Event::Before),
        NativeApproval::Fallback
    ));
    let value = OpenCode.reply(
        Event::Before,
        &Reply {
            decision: Decision::Deny,
            reason: "Fixture denied".into(),
            agent_context: None,
        },
    );
    assert_eq!(
        value,
        json!({"protocol_version":1,"event":"tool.execute.before","decision":"deny","reason":"Fixture denied"})
    );
    assert_eq!(
        OpenCode.completion("tool.execute.after", None)["decision"],
        "observed"
    );
}
