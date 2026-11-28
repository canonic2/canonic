//! Project onboarding uses direct MCP dispatch without manager or terminal ownership.
use super::*;
#[tokio::test]
async fn onboarding_mcp_responsive_dispatch_uses_the_same_plan_and_creation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("index.html"), "<!doctype html>").unwrap();
    let server = Server::new();
    let protocol = ready_protocol();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"project_discover","arguments":{"root":root.path()}}});
    assert!(matches!(
        protocol.receive(&serde_json::to_vec(&request).unwrap()),
        canonic_mcp_kit::Inbound::Tool { .. }
    ));
    let response = protocol
        .responsive_request(&request, &server.managed)
        .expect("Discovery is responsive without a manager")
        .await;
    assert_eq!(response["result"]["isError"], false, "{response}");
    let digest = response["result"]["structuredContent"]["data"]["planDigest"].clone();
    protocol.finish(&json!(1));
    assert!(!root.path().join("testrun.yml").exists());
    let request = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"project_init_preview","arguments":{"root":root.path()}}});
    assert!(matches!(
        protocol.receive(&serde_json::to_vec(&request).unwrap()),
        canonic_mcp_kit::Inbound::Tool { .. }
    ));
    let response = protocol
        .responsive_request(&request, &server.managed)
        .expect("Preview is responsive without a manager")
        .await;
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["data"]["planDigest"],
        digest
    );
    assert!(!root.path().join("testrun.yml").exists());
    protocol.finish(&json!(2));
    let request = json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"project_init","arguments":{"root":root.path(),"planDigest":digest}}});
    assert!(matches!(
        protocol.receive(&serde_json::to_vec(&request).unwrap()),
        canonic_mcp_kit::Inbound::Tool { .. }
    ));
    let response = protocol
        .responsive_request(&request, &server.managed)
        .expect("Initialization is responsive without a manager")
        .await;
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert!(root.path().join("testrun.yml").exists());
    protocol.finish(&json!(3));
}
#[tokio::test]
async fn onboarding_mcp_protocol_previews_and_creates_without_runtime_targets() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("index.html"), "<!doctype html>").unwrap();
    let mut server = Server::new();
    let mut protocol = ready_protocol();
    let response=protocol.request(&mut server,json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"project_discover","arguments":{"root":root.path()}}})).await.unwrap();
    assert_eq!(response["result"]["isError"], false, "{response}");
    let text: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, response["result"]["structuredContent"]);
    assert!(!root.path().join("testrun.yml").exists());
    let plan = response["result"]["structuredContent"]["data"].clone();
    let response=protocol.request(&mut server,json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"project_init_preview","arguments":{"root":root.path()}}})).await.unwrap();
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["data"]["planDigest"],
        plan["planDigest"]
    );
    assert!(!root.path().join("testrun.yml").exists());
    let response=protocol.request(&mut server,json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"project_init","arguments":{"root":root.path(),"planDigest":plan["planDigest"]}}})).await.unwrap();
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["data"]["configuration"],
        plan["configuration"]
    );
    assert!(root.path().join("testrun.yml").is_file());
}
