//! Modern discovery, bundled skill reads and both legacy fallbacks on one transport.
use super::*;

const SKILL: &str = "skill://testrun/ui-verification/SKILL.md";

fn modern(id: Value, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({
        canonic_mcp_kit::modern::VERSION_KEY: canonic_mcp_kit::modern::PROTOCOL_VERSION,
        canonic_mcp_kit::modern::CAPABILITIES_KEY: {},
        canonic_mcp_kit::modern::CLIENT_KEY: {"name":"Acme host","version":"1"},
    });
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

fn source() -> &'static str {
    include_str!("../../../skills/ui-verification/SKILL.md")
}

#[tokio::test]
async fn modern_subscriptions_remain_disabled_without_retaining_request_ids() {
    let mut server = Server::new();
    let mut protocol = Protocol::default();
    for filter in [
        json!({}),
        json!({"toolsListChanged":true}),
        json!({"resourceSubscriptions":["skill://testrun/ui-verification/SKILL.md"]}),
    ] {
        let response = protocol
            .request(
                &mut server,
                modern(
                    json!("Acme-listen"),
                    "subscriptions/listen",
                    json!({"notifications":filter}),
                ),
            )
            .await
            .unwrap();
        assert_eq!(response["error"]["code"], -32601, "{response}");
    }
    let discovered = protocol
        .request(
            &mut server,
            modern(json!("Acme-listen"), "server/discover", json!({})),
        )
        .await
        .unwrap();
    assert!(discovered.get("result").is_some(), "{discovered}");
}

#[tokio::test]
async fn modern_discovery_and_skill_reads_need_no_handshake_or_runtime() {
    let mut server = Server::new();
    let mut protocol = Protocol::default();
    let discovered = protocol
        .request(&mut server, modern(json!(1), "server/discover", json!({})))
        .await
        .unwrap();
    let result = &discovered["result"];
    assert_eq!(result["resultType"], "complete");
    assert_eq!(
        result["capabilities"]["extensions"],
        json!({"io.modelcontextprotocol/skills":{}})
    );
    assert_eq!(
        result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "canonic-testrun"
    );
    assert!(result["instructions"].as_str().unwrap().contains(SKILL));

    let listed = protocol
        .request(&mut server, modern(json!(2), "skills/list", json!({})))
        .await
        .unwrap();
    let skills = listed["result"]["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0]["uri"], SKILL);
    let got = protocol
        .request(
            &mut server,
            modern(json!(3), "skills/get", json!({"uri":SKILL})),
        )
        .await
        .unwrap();
    assert_eq!(got["result"]["skill"], skills[0]);
    assert_eq!(got["result"]["cacheScope"], "public");

    let read = protocol
        .request(
            &mut server,
            modern(json!(4), "resources/read", json!({"uri":SKILL})),
        )
        .await
        .unwrap();
    assert_eq!(read["result"]["resultType"], "complete");
    assert_eq!(read["result"]["cacheScope"], "public");
    assert_eq!(read["result"]["contents"][0]["text"], source());
    assert_eq!(read["result"]["contents"][0]["mimeType"], "text/markdown");
    // The read neither initialized the connection nor reached the device runtime.
    assert!(!protocol.ready());
}

#[tokio::test]
async fn modern_tool_results_keep_their_envelope_with_request_identity_and_no_cache_hints() {
    let mut server = Server::new();
    let mut protocol = Protocol::default();
    let response = protocol
        .request(
            &mut server,
            modern(
                json!("docs"),
                "tools/call",
                json!({"name":"docs_list","arguments":{}}),
            ),
        )
        .await
        .unwrap();
    let result = &response["result"];
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"]["ok"], true);
    assert_eq!(
        result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "canonic-testrun"
    );
    assert!(result.get("ttlMs").is_none());
    assert!(result.get("cacheScope").is_none());
    // The ID was settled once; a late duplicate reply is not written.
    assert!(protocol.settle(&response).is_none());
}

#[tokio::test]
async fn unknown_resources_use_each_revisions_error_code() {
    let mut server = Server::new();
    let mut protocol = ready_protocol();
    for uri in [
        "skill://testrun/ui-verification/missing.md",
        "https://example.com/acme",
    ] {
        let legacy = protocol
            .request(
                &mut server,
                json!({"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":uri}}),
            )
            .await
            .unwrap();
        assert_eq!(legacy["error"]["code"], -32002, "{uri}");
        assert_eq!(legacy["error"]["data"]["uri"], uri);
        let current = protocol
            .request(
                &mut server,
                modern(json!(2), "resources/read", json!({"uri":uri})),
            )
            .await
            .unwrap();
        assert_eq!(current["error"]["code"], -32602, "{uri}");
        assert_eq!(current["error"]["data"]["uri"], uri);
    }
}

#[tokio::test]
async fn legacy_clients_read_skill_files_as_ordinary_resources_without_skills() {
    let mut server = Server::new();
    let mut protocol = Protocol::default();
    for version in canonic_mcp_kit::SUPPORTED_PROTOCOL_VERSIONS {
        let mut protocol = Protocol::default();
        let initialized = protocol.request(&mut server, json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"Acme host","version":"1"}}})).await.unwrap();
        let capabilities = &initialized["result"]["capabilities"];
        assert_eq!(capabilities["resources"], json!({}), "{version}");
        assert!(capabilities.get("extensions").is_none(), "{version}");
    }
    protocol.request(&mut server, json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"Acme host","version":"1"}}})).await;
    protocol
        .request(
            &mut server,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
    let refused = protocol
        .request(
            &mut server,
            json!({"jsonrpc":"2.0","id":1,"method":"skills/list","params":{}}),
        )
        .await
        .unwrap();
    assert_eq!(refused["error"]["code"], -32601);
    let listed = protocol
        .request(
            &mut server,
            json!({"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}),
        )
        .await
        .unwrap();
    assert_eq!(listed["result"]["resources"][0]["uri"], SKILL);
    let read = protocol
        .request(
            &mut server,
            json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":SKILL}}),
        )
        .await
        .unwrap();
    assert!(read["result"].get("resultType").is_none());
    assert_eq!(read["result"]["contents"][0]["text"], source());
}

#[tokio::test]
async fn task_end_keeps_its_initialized_connection_contract_on_modern_requests() {
    let mut server = Server::new();
    let mut protocol = Protocol::default();
    let refused = protocol
        .request(&mut server, modern(json!(1), "testrun/task_end", json!({})))
        .await
        .unwrap();
    assert_eq!(refused["error"]["code"], -32602);
    assert_eq!(
        refused["error"]["message"],
        "Task end requires an initialized connection"
    );
    let ready = ready_protocol();
    assert!(ready.empty_params(&json!(2), &json!({})));
    assert!(!ready.empty_params(&json!(2), &json!({"_meta":{}})));
}

#[tokio::test]
async fn bundled_reads_cancel_without_answering_a_reused_id() {
    let protocol = Protocol::default();
    let mut reads = responsive::Reads::default();
    let request = modern(json!("read"), "resources/read", json!({"uri":SKILL}));
    let frame = serde_json::to_vec(&request).unwrap();
    let canonic_mcp_kit::Inbound::BundledResource { id, read, .. } = protocol.receive(&frame)
    else {
        panic!("skill read is library-owned");
    };
    assert!(reads.admit_bundled(id.clone(), read).is_none());
    let cancel =
        br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"read"}}"#;
    assert!(matches!(
        protocol.receive(cancel),
        canonic_mcp_kit::Inbound::Cancel { .. }
    ));
    assert!(reads.cancel(&id));
    // The ID is reusable; only the new request's reply is written.
    let canonic_mcp_kit::Inbound::BundledResource { id, read, .. } = protocol.receive(&frame)
    else {
        panic!("reused ID is admitted again");
    };
    assert!(reads.admit_bundled(id, read).is_none());
    let mut written = Vec::new();
    while !reads.is_empty() {
        if let Some(response) = reads.next().await.unwrap()
            && let Some(encoded) = protocol.settle(&response)
        {
            written.push(serde_json::from_slice::<Value>(&encoded).unwrap());
        }
    }
    assert_eq!(written.len(), 1);
    assert_eq!(written[0]["result"]["contents"][0]["text"], source());
    assert_eq!(written[0]["result"]["resultType"], "complete");
}
