//! Prompt discovery and rendering remain read-only across protocol revisions.
use super::*;

#[test]
fn installation_prompts_render_without_acquisition_or_policy_changes() {
    for version in ["2025-06-18", VERSION] {
        let (server, project) = fixture(Some("claude"));
        let before = project.revision();
        let mut messages = initialize(&json!({}), "claude-code").to_vec();
        messages[0]["params"]["protocolVersion"] = json!(version);
        messages.extend([
            json!({"jsonrpc":"2.0","id":10,"method":"prompts/list","params":{}}),
            json!({"jsonrpc":"2.0","id":11,"method":"prompts/get","params":{"name":"install_pack","arguments":{"reference":"unregistered/acme/example","profile":"strict","layer":"local"}}}),
            json!({"jsonrpc":"2.0","id":12,"method":"prompts/get","params":{"name":"install_pack","arguments":{"reference":"acme/example","layer":"wrong"}}}),
            json!({"jsonrpc":"2.0","id":13,"method":"prompts/get","params":{"name":"install_pack","arguments":{}}}),
        ]);
        let outputs = exchange(server, &messages);
        assert_eq!(
            reply(&outputs, &json!(10))["result"]["prompts"][0]["name"],
            "install_pack"
        );
        let result = &reply(&outputs, &json!(11))["result"];
        let text = result["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(
            text.contains("pack_add")
                && text.contains("policy_apply")
                && text.contains("strict")
                && text.contains("local")
        );
        for id in [12, 13] {
            assert_eq!(reply(&outputs, &json!(id))["error"]["code"], -32602);
        }
        assert_eq!(project.revision(), before);
        assert_eq!(
            crate::review::proposals(&project.review, None, false).unwrap()["proposals"]
                .as_array()
                .unwrap()
                .as_slice(),
            &[] as &[Value]
        );
    }
}

#[test]
fn installation_prompts_support_modern_discovery_and_checked_arguments() {
    let (server, project) = fixture(Some("codex"));
    let before = project.revision();
    let outputs = exchange(
        server,
        &[
            super::skills::modern(10, "prompts/list", json!({}), json!({})),
            super::skills::modern(
                11,
                "prompts/get",
                json!({"name":"install_pack","arguments":{"reference":"acme/settings"}}),
                json!({}),
            ),
            super::skills::modern(
                12,
                "prompts/get",
                json!({"name":"install_pack","arguments":{"reference":"acme/settings","unknown":"value"}}),
                json!({}),
            ),
        ],
    );
    assert_eq!(
        reply(&outputs, &json!(10))["result"]["prompts"][0]["name"],
        "install_pack"
    );
    let result = &reply(&outputs, &json!(11))["result"];
    assert_eq!(result["resultType"], "complete", "{result}");
    assert!(
        result["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("policy_apply")
    );
    assert_eq!(reply(&outputs, &json!(12))["error"]["code"], -32602);
    assert_eq!(project.revision(), before);
}

#[test]
fn static_installation_prompt_explains_enrollment_without_advertising_tools() {
    let messages = [
        super::skills::modern(10, "prompts/list", json!({}), json!({})),
        super::skills::modern(
            11,
            "prompts/get",
            json!({"name":"install_pack","arguments":{"reference":"acme/settings"}}),
            json!({}),
        ),
        super::skills::modern(12, "tools/list", json!({}), json!({})),
        super::skills::modern(
            13,
            "tools/call",
            json!({"name":"pack_add","arguments":{"reference":"acme/settings"}}),
            json!({}),
        ),
    ];
    let mut output = Vec::new();
    serve_static(Cursor::new(frames(&messages)), &mut output, &test_options()).unwrap();
    let outputs = lines(&output);
    assert_eq!(
        reply(&outputs, &json!(10))["result"]["prompts"][0]["name"],
        "install_pack"
    );
    let rendered = reply(&outputs, &json!(11));
    let text = rendered["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("enrolled project") && text.contains("shield init"));
    assert_eq!(reply(&outputs, &json!(12))["result"]["tools"], json!([]));
    assert!(reply(&outputs, &json!(13))["error"].is_object());
}
