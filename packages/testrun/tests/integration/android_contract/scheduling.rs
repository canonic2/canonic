//! Shipping MCP admission under a blocked, independently owned discovery read.
use super::*;

#[tokio::test]
async fn blocked_discovery_preserves_connected_work_and_control_capacity() {
    let serial = format!("Acme-scheduling-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    assert_eq!(
        mcp.tool("connect", json!({"targetId":format!("android:{serial}")}))
            .await["ok"],
        true
    );
    super::network_recovery::rewrite_adb(
        &mcp,
        "devices) printf",
        &format!(
            "devices) if [ \"$PPID\" = \"{}\" ]; then echo $$ >> discovery-pids; touch discovering; count=0; while [ -f hold-discovery ] && [ $count -lt 300 ]; do sleep 0.02; count=$((count+1)); done; fi; printf",
            mcp.child.id().unwrap()
        ),
    );
    mcp.arm("hold-discovery");
    mcp.send(json!({"jsonrpc":"2.0","id":"slow","method":"tools/call","params":{"name":"target_list","arguments":{"kind":"android"}}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !mcp._directory.path().join("discovering").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mcp.send(json!({"jsonrpc":"2.0","id":"healthy","method":"tools/call","params":{"name":"observe","arguments":{}}})).await;
    let response = tokio::time::timeout(Duration::from_secs(2), mcp.output.next_line()).await;
    // Release even on the pre-fix failure; no fixture process may remain held.
    std::fs::remove_file(mcp._directory.path().join("hold-discovery")).unwrap();
    let response: Value = serde_json::from_str(
        &response
            .expect("connected observation blocked behind discovery")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(response["id"], "healthy", "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["ok"], true,
        "{response}"
    );
    // Drain the discovery before using the sequential fixture helper again.
    let response: Value =
        serde_json::from_str(&mcp.output.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(response["id"], "slow", "{response}");
    saturated_reads(&mut mcp).await;
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert!(mcp.commands().is_empty(), "reads must not replay input");
    eof_disposes_discovery(&mut mcp).await;
}

async fn eof_disposes_discovery(mcp: &mut Mcp) {
    std::fs::remove_file(mcp._directory.path().join("discovering")).unwrap();
    mcp.arm("hold-discovery");
    mcp.send(json!({"jsonrpc":"2.0","id":"eof-read","method":"tools/call","params":{"name":"target_list","arguments":{"kind":"android"}}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !mcp._directory.path().join("discovering").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    drop(mcp.input.take());
    let status = tokio::time::timeout(Duration::from_secs(2), mcp.child.wait())
        .await
        .expect("EOF waited for the blocked read")
        .unwrap();
    assert!(status.success(), "{status}");
    assert_discovery_stopped(mcp).await;
}

async fn receive(mcp: &mut Mcp) -> Value {
    serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(5), mcp.output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn duplicate_outstanding_id_ends_the_connection_and_disposes_owned_reads() {
    let serial = format!("Acme-duplicate-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    super::network_recovery::rewrite_adb(
        &mcp,
        "devices) printf",
        &format!(
            "devices) if [ \"$PPID\" = \"{}\" ]; then echo $$ >> discovery-pids; touch discovering; count=0; while [ -f hold-discovery ] && [ $count -lt 300 ]; do sleep 0.02; count=$((count+1)); done; fi; printf",
            mcp.child.id().unwrap()
        ),
    );
    mcp.arm("hold-discovery");
    mcp.send(json!({"jsonrpc":"2.0","id":"active","method":"tools/call","params":{"name":"target_list","arguments":{"kind":"android"}}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !mcp._directory.path().join("discovering").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mcp.send(json!({"jsonrpc":"2.0","id":"active","method":"ping"}))
        .await;
    let final_reply = receive(&mut mcp).await;
    assert_eq!(final_reply["id"], "active");
    assert_eq!(final_reply["error"]["code"], -32600);
    let remaining = tokio::time::timeout(Duration::from_secs(5), mcp.output.next_line())
        .await
        .unwrap()
        .unwrap();
    assert!(
        remaining.is_none(),
        "Fatal duplicate must have only one reply: {remaining:?}"
    );
    assert!(mcp.child.wait().await.unwrap().success());
    assert_discovery_stopped(&mcp).await;
}

async fn saturated_reads(mcp: &mut Mcp) {
    std::fs::remove_file(mcp._directory.path().join("discovering")).unwrap();
    mcp.arm("hold-discovery");
    for id in 0..5 {
        mcp.send(json!({"jsonrpc":"2.0","id":format!("read-{id}"),"method":"tools/call","params":{"name":"target_list","arguments":{"kind":"android"}}})).await;
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !mcp._directory.path().join("discovering").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mcp.send(json!({"jsonrpc":"2.0","id":"ping","method":"ping"}))
        .await;
    let target = mcp.status_target.clone().unwrap();
    mcp.send(json!({"jsonrpc":"2.0","id":"status","method":"tools/call","params":{"name":"status","arguments":{"targetId":target}}})).await;
    let mut responses = std::collections::HashMap::new();
    for _ in 0..3 {
        let response = receive(mcp).await;
        responses.insert(response["id"].as_str().unwrap().to_owned(), response);
    }
    assert_eq!(responses["read-4"]["error"]["code"], -32000);
    assert_eq!(responses["ping"]["result"], json!({}));
    assert_eq!(
        responses["status"]["result"]["structuredContent"]["data"]["state"],
        "ready"
    );
    for id in 0..4 {
        mcp.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":format!("read-{id}")}})).await;
    }
    mcp.cancellation_barrier().await;
    // The read's adapter owns a real process group. Cancellation must dispose
    // it while the fixture's gate is still held, without another read request.
    assert_discovery_stopped(mcp).await;
    std::fs::remove_file(mcp._directory.path().join("hold-discovery")).unwrap();
    // Cancellation must not quarantine the healthy controller or
    // clear a neighbor's request. Reuse an ID after its aborted task settles.
    assert_eq!(mcp.tool("observe", json!({})).await["ok"], true);
    mcp.send(json!({"jsonrpc":"2.0","id":"read-0","method":"ping"}))
        .await;
    let response = receive(mcp).await;
    assert_eq!(response["id"], "read-0");
    assert_eq!(response["result"], json!({}));
}

async fn assert_discovery_stopped(mcp: &Mcp) {
    let pids = std::fs::read_to_string(mcp._directory.path().join("discovery-pids")).unwrap();
    assert!(!pids.trim().is_empty());
    for pid in pids.lines() {
        assert!(pid.parse::<u32>().unwrap() > 1, "invalid discovery PID");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while Command::new("kill")
            .args(["-0", pid])
            .stderr(Stdio::null())
            .status()
            .await
            .unwrap()
            .success()
        {
            assert!(
                tokio::time::Instant::now() < deadline,
                "discovery process survived: {pid}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}
