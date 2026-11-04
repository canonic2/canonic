//! Static publication and modern per-call context exercised through shared stdio.
use super::*;

pub(super) fn modern(id: i64, method: &str, mut params: Value, capabilities: Value) -> Value {
    let mut metadata = json!({
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
    });
    metadata["io.modelcontextprotocol/clientCapabilities"] = capabilities;
    params["_meta"] = metadata;
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

#[test]
fn static_discovery_get_and_read_need_no_project_or_handshake() {
    let uri = "skill://shield/policy-review/SKILL.md";
    let messages = [
        modern(1, "server/discover", json!({}), json!({})),
        modern(2, "skills/list", json!({}), json!({})),
        modern(3, "skills/get", json!({"uri":uri}), json!({})),
        modern(4, "resources/read", json!({"uri":uri}), json!({})),
        modern(
            5,
            "resources/read",
            json!({"uri":"skill://shield/policy-review/missing"}),
            json!({}),
        ),
        modern(6, "tools/list", json!({}), json!({})),
    ];
    let mut output = Vec::new();
    serve_static(Cursor::new(frames(&messages)), &mut output, &test_options()).unwrap();
    let outputs = lines(&output);
    let discovery = reply(&outputs, &json!(1));
    assert!(
        discovery["result"]["capabilities"]["extensions"]["io.modelcontextprotocol/skills"]
            .is_object()
    );
    let listed = reply(&outputs, &json!(2));
    let entry = &listed["result"]["skills"][0];
    assert_eq!(entry, &reply(&outputs, &json!(3))["result"]["skill"]);
    let read = reply(&outputs, &json!(4));
    let source = include_str!("../../../skills/policy-review/SKILL.md");
    let tools = catalog(None).unwrap();
    for reference in source.split('`').skip(1).step_by(2) {
        if reference.contains('_') && reference != "apply_arguments" {
            assert!(
                tools.tools().iter().any(|tool| tool.name == reference),
                "unknown bundled tool reference: {reference}"
            );
        }
    }
    assert_eq!(read["result"]["contents"][0]["text"], source);
    assert_eq!(entry["resources"].as_array().unwrap().len(), 1);
    assert_eq!(entry["resources"][0]["uri"], uri);
    assert_eq!(entry["resources"][0]["size"], source.len());
    assert_eq!(
        entry["resources"][0]["digest"],
        format!("sha256:{}", crate::policy::fingerprint(source.as_bytes()))
    );
    for id in 1..=4 {
        let result = reply(&outputs, &json!(id))["result"].clone();
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["cacheScope"], "private");
        assert_eq!(result["ttlMs"], 0);
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "shield"
        );
    }
    assert_eq!(
        reply(&outputs, &json!(5))["error"]["code"],
        rpc::INVALID_PARAMS
    );
    assert_eq!(reply(&outputs, &json!(6))["result"]["tools"], json!([]));
}

#[test]
fn bundled_reads_remain_responsive_while_the_evaluator_is_busy() {
    use canonic_mcp_kit::stdio::Handler;
    struct Busy {
        server: Server,
        started: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl Handler for Busy {
        fn initialize(&self, client: &Client) -> Result<ServerInfo, RpcError> {
            self.server.initialize(client)
        }
        fn admit(&self, request: &Request) -> stdio::Admission {
            self.server.admit(request)
        }
        fn call(&self, request: Request, context: &stdio::Context) -> Outcome {
            self.started.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            self.server.call(request, context)
        }
    }
    let (server, project) = fixture(None);
    let before = project.revision();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let (input_tx, input_rx) = mpsc::sync_channel(8);
    let (output_tx, output_rx) = mpsc::sync_channel(8);
    let worker = thread::spawn(move || {
        stdio::serve(
            Arc::new(Busy {
                server,
                started: started_tx,
                release: Mutex::new(release_rx),
            }),
            Arc::new(catalog(None).unwrap()),
            BufReader::new(Frames {
                incoming: input_rx,
                current: Cursor::new(Vec::new()),
            }),
            Responses {
                outgoing: output_tx,
                current: Vec::new(),
            },
            &bundled_options(&test_options()).unwrap(),
        )
        .unwrap();
    });
    input_tx
        .send(frames(&[modern(
            1,
            "tools/call",
            json!({"name":"status","arguments":{}}),
            json!({}),
        )]))
        .unwrap();
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    input_tx
        .send(frames(&[modern(
            2,
            "resources/read",
            json!({"uri":"skill://shield/policy-review/SKILL.md"}),
            json!({}),
        )]))
        .unwrap();
    let output = output_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(lines(&output)[0]["id"], 2);
    release_tx.send(()).unwrap();
    assert_eq!(
        lines(&output_rx.recv_timeout(Duration::from_secs(5)).unwrap())[0]["id"],
        1
    );
    drop(input_tx);
    worker.join().unwrap();
    assert_eq!(project.revision(), before);
}

#[test]
fn static_reads_respect_the_output_frame_limit() {
    let mut options = test_options();
    options.max_output_bytes = 1024;
    let mut output = Vec::new();
    serve_static(
        Cursor::new(frames(&[modern(
            1,
            "resources/read",
            json!({"uri":"skill://shield/policy-review/SKILL.md"}),
            json!({}),
        )])),
        &mut output,
        &options,
    )
    .unwrap();
    let replies = lines(&output);
    assert!(replies[0]["error"].is_object(), "{replies:?}");
    assert!(output.len() <= 1025);
    assert!(!String::from_utf8_lossy(&output).contains("# Review Shield permissions"));
}

#[test]
fn modern_and_legacy_inspection_share_no_request_capabilities() {
    let (server, project) = fixture(Some("claude"));
    let before = project.revision();
    let transport = Transport::start(server);
    let request = modern(
        10,
        "tools/call",
        json!({"name":"policy_show","arguments":{"status":"pending"}}),
        json!({}),
    );
    let response = transport.request(&request);
    assert_eq!(response["result"]["resultType"], "complete");
    assert_eq!(
        response["result"]["structuredContent"]["data"]["proposals"],
        json!([])
    );
    for message in initialize(&json!({"elicitation":{"form":{}}}), "codex") {
        transport.send(&message);
    }
    assert!(transport.receive()["result"].is_object());
    let legacy = transport.call(json!(11), "history_list", &json!({}));
    assert!(legacy["result"].get("resultType").is_none());
    let modern = transport.request(&modern(
        12,
        "resources/read",
        json!({"uri":"skill://shield/policy-review/SKILL.md"}),
        json!({}),
    ));
    assert_eq!(modern["result"]["resultType"], "complete");
    assert_eq!(project.revision(), before);
}
