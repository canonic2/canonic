//! Shipping managed handoff and authenticated local viewer input without media authority.
use super::*;
use tokio::{io::AsyncReadExt, net::UnixStream};

async fn send(socket: &mut UnixStream, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    socket.write_u32(bytes.len() as u32).await.unwrap();
    socket.write_all(&bytes).await.unwrap();
}
async fn read(socket: &mut UnixStream) -> Value {
    tokio::time::timeout(Duration::from_secs(12), async {
        let size = socket.read_u32().await.unwrap() as usize;
        assert!(size <= 1024 * 1024);
        let mut bytes = vec![0; size];
        socket.read_exact(&mut bytes).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    })
    .await
    .unwrap()
}
async fn viewer(channel: &Value) -> UnixStream {
    let mut broker = UnixStream::connect(channel["grantBrokerPath"].as_str().unwrap())
        .await
        .unwrap();
    send(
        &mut broker,
        json!({"kind":"control_grant","channelId":channel["channelId"]}),
    )
    .await;
    let grant = read(&mut broker).await;
    assert_eq!(grant["ok"], true, "{grant}");
    let mut socket = UnixStream::connect(channel["controlSocketPath"].as_str().unwrap())
        .await
        .unwrap();
    send(&mut socket, json!({"token":grant["data"]["token"]})).await;
    assert_eq!(read(&mut socket).await["kind"], "ready");
    // A consumed grant cannot authorize another connection.
    let mut replay = UnixStream::connect(channel["controlSocketPath"].as_str().unwrap())
        .await
        .unwrap();
    send(&mut replay, json!({"token":grant["data"]["token"]})).await;
    assert_eq!(read(&mut replay).await["error"]["code"], "UNAUTHORIZED");
    socket
}
fn tap() -> Value {
    json!({"action":"tap","point":{"x":0.5,"y":0.5}})
}
async fn tool_request(mcp: &mut Mcp, id: &str, name: &str, arguments: Value) {
    mcp.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})).await;
}
async fn wait_file(mcp: &Mcp, name: &str) {
    let end = tokio::time::Instant::now() + Duration::from_secs(3);
    while !mcp._directory.path().join(name).exists() {
        assert!(tokio::time::Instant::now() < end, "Input never dispatched");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
#[tokio::test]
async fn handoff_fences_queued_agent_input_before_the_active_action_finishes() {
    let serial = format!("Acme-fence-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    assert_eq!(
        mcp.tool("connect", json!({"targetId":format!("android:{serial}")}))
            .await["ok"],
        true
    );
    let mut inspector = mcp.successor().await;
    network_recovery::rewrite_adb(
        &mcp,
        "    *acme-unhandled*)",
        "    *\"'input' 'tap'\"*) touch input-held; count=0; while [ -f hold-input ] && [ $count -lt 400 ]; do sleep 0.02; count=$((count+1)); done ;;\n    *acme-unhandled*)",
    );
    mcp.arm("hold-input");
    tool_request(&mut mcp, "active", "act", json!({"actions":[tap()]})).await;
    wait_file(&mcp, "input-held").await;
    tool_request(&mut mcp, "queued", "act", json!({"actions":[tap()]})).await;
    tool_request(
        &mut mcp,
        "handoff",
        "control_acquire",
        json!({"owner":"viewer","viewerId":"acme-host","expectedVersion":1}),
    )
    .await;
    let end = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let status = inspector.tool("control_status", json!({})).await;
        if status["data"]["state"] == "handoff_pending" {
            break;
        }
        assert!(tokio::time::Instant::now() < end, "{status}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    std::fs::remove_file(mcp._directory.path().join("hold-input")).unwrap();
    let mut replies = std::collections::BTreeMap::new();
    while replies.len() < 3 {
        let line = tokio::time::timeout(Duration::from_secs(15), mcp.output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        if let Some(id) = reply["id"].as_str() {
            replies.insert(id.to_owned(), reply["result"]["structuredContent"].clone());
        }
    }
    assert_eq!(replies["active"]["ok"], true, "{replies:?}");
    assert_eq!(replies["queued"]["ok"], false, "{replies:?}");
    assert_eq!(replies["handoff"]["ok"], true, "{replies:?}");
    assert_eq!(
        mcp.commands()
            .iter()
            .filter(|line| line.contains("'input' 'tap'"))
            .count(),
        1
    );
    let handed = &replies["handoff"]["data"];
    acquire(&mut inspector, "agent", &handed["controlVersion"]).await;
    assert_eq!(
        inspector.tool("act", json!({"actions":[tap()]})).await["ok"],
        true
    );
    // Either client's disconnect releases the device for both.
    let released = inspector.tool("disconnect", json!({})).await;
    assert_eq!(released["data"]["closed"], true, "{released}");
    let gone = mcp.tool("act", json!({"actions":[tap()]})).await;
    assert_eq!(gone["code"], "NOT_CONNECTED", "{gone}");
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}
async fn acquire(mcp: &mut Mcp, owner: &str, version: &Value) -> Value {
    let mut args = json!({"owner":owner,"expectedVersion":version});
    if owner == "viewer" {
        args["viewerId"] = json!("acme-local-viewer");
    }
    let reply = mcp.tool("control_acquire", args).await;
    assert_eq!(reply["ok"], true, "{reply}");
    assert!(reply["data"].get("leaseId").is_none());
    reply["data"].clone()
}
#[tokio::test]
async fn managed_viewer_handoff_revocation_and_expiry_use_the_input_owner() {
    let serial = format!("Acme-handoff-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    assert_eq!(
        mcp.tool("connect", json!({"targetId":format!("android:{serial}")}))
            .await["ok"],
        true
    );
    let status = mcp.tool("control_status", json!({})).await;
    let handed = acquire(&mut mcp, "viewer", &status["data"]["controlVersion"]).await;
    let refused = mcp.tool("act", json!({"actions":[tap()]})).await;
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        !mcp.commands()
            .iter()
            .any(|line| line.contains("'input' 'tap'"))
    );
    let mut socket = viewer(&handed["channel"]).await;
    let command = uuid::Uuid::new_v4().to_string();
    send(&mut socket, json!({"kind":"act","commandId":command,"display":{"displayId":"main","geometryVersion":"100x200"},"actions":[tap()]})).await;
    let result = read(&mut socket).await;
    assert_eq!(result["commandId"], command);
    assert_eq!(result["result"]["ok"], true, "{result}");
    assert_eq!(
        mcp.commands()
            .iter()
            .filter(|line| line.contains("'input' 'tap'"))
            .count(),
        1
    );
    // Read-only observation continues while input belongs to the viewer.
    assert_eq!(mcp.tool("observe", json!({})).await["ok"], true);
    drop(socket);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let released = mcp.tool("control_status", json!({})).await;
    assert_eq!(released["data"]["state"], "released", "{released}");
    let stale = mcp
        .tool(
            "control_acquire",
            json!({"owner":"agent","expectedVersion":handed["controlVersion"]}),
        )
        .await;
    assert_eq!(stale["ok"], false, "{stale}");
    let agent = acquire(&mut mcp, "agent", &released["data"]["controlVersion"]).await;
    assert_eq!(
        mcp.tool("act", json!({"actions":[tap()]})).await["ok"],
        true
    );
    let handed = acquire(&mut mcp, "viewer", &agent["controlVersion"]).await;
    let mut socket = viewer(&handed["channel"]).await;
    network_recovery::rewrite_adb(
        &mcp,
        "    *acme-unhandled*)",
        "    *\"'input' 'tap'\"*) touch input-held; count=0; while [ -f hold-input ] && [ $count -lt 400 ]; do sleep 0.02; count=$((count+1)); done ;;\n    *acme-unhandled*)",
    );
    mcp.arm("hold-input");
    send(&mut socket, json!({"kind":"act","commandId":uuid::Uuid::new_v4().to_string(),"display":{"displayId":"main","geometryVersion":"100x200"},"actions":[tap()]})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while !mcp._directory.path().join("input-held").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    send(&mut socket, json!({"kind":"heartbeat"})).await;
    assert_eq!(read(&mut socket).await["kind"], "heartbeat");
    tokio::time::sleep(Duration::from_millis(5200)).await;
    let expired = mcp.tool("control_status", json!({})).await;
    assert_eq!(expired["data"]["state"], "released", "{expired}");
    assert_eq!(expired["data"]["reason"], "heartbeat_expired");
    std::fs::remove_file(mcp._directory.path().join("hold-input")).unwrap();
    let uncertain = mcp
        .tool(
            "control_acquire",
            json!({"owner":"agent","expectedVersion":expired["data"]["controlVersion"]}),
        )
        .await;
    assert_eq!(uncertain["code"], "SESSION_UNCERTAIN", "{uncertain}");
    assert_eq!(
        mcp.commands()
            .iter()
            .filter(|line| line.contains("'input' 'tap'"))
            .count(),
        3
    );
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}
