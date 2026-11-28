//! Documentation registration, text envelopes and offline protocol dispatch.
use super::*;

#[test]
fn docs_have_host_compatible_object_schemas_and_read_only_hints() {
    let catalog = tools();
    for name in ["docs_list", "docs_search", "docs_read"] {
        let tool = catalog.get(name).unwrap();
        let definition = serde_json::to_value(tool).unwrap();
        assert_eq!(definition["inputSchema"]["type"], "object");
        for union in ["oneOf", "anyOf", "allOf"] {
            assert!(definition["inputSchema"].get(union).is_none());
        }
        assert_eq!(definition["annotations"]["readOnlyHint"], true);
        assert_eq!(definition["annotations"]["openWorldHint"], false);
    }
}

#[tokio::test]
async fn docs_round_trip_in_normal_text_and_structured_envelopes() {
    let mut server = Server::new();
    let mut protocol = ready_protocol();
    for (name, arguments) in [
        ("docs_list", json!({})),
        ("docs_search", json!({"query":"```","path":"mcp.md"})),
        ("docs_read", json!({"path":"mcp.md","limit":400})),
    ] {
        let response = protocol.request(&mut server, json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}})).await.unwrap();
        assert_eq!(response["result"]["isError"], false, "{response}");
        let structured = &response["result"]["structuredContent"];
        let text: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(text, *structured);
        assert_eq!(structured["data"]["version"], env!("CARGO_PKG_VERSION"));
        if name == "docs_read" {
            let source = include_str!("../../../docs/mcp.md");
            assert_eq!(
                structured["data"]["content"],
                source.split_inclusive('\n').take(400).collect::<String>()
            );
        }
    }
}
