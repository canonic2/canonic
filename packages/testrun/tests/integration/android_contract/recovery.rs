//! Startup cancellation and uncertain input recovery through shipping transports.
use super::*;

async fn wait_for(mcp: &Mcp, marker: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !mcp._directory.path().join(marker).exists() {
        assert!(tokio::time::Instant::now() < deadline, "Missing {marker}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn reply(mcp: &mut Mcp, id: &str) -> Value {
    loop {
        let line = tokio::time::timeout(Duration::from_secs(15), mcp.output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        if reply["id"] == id {
            return serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        }
    }
}

#[tokio::test]
async fn owner_death_after_input_dispatch_preserves_unknown_outcome_without_replay() {
    let mut mcp = super::network_recovery::connected().await;
    let mut foreign = super::network_recovery::connected().await;
    let (_, original) = super::host_lifecycle::selected(&mcp);
    let pid = mcp.owner_pid().await;
    super::network_recovery::rewrite_adb(
        &mcp,
        "    *acme-unhandled*)",
        "    *\"'input' 'tap'\"*) touch input-admitted; count=0; while [ -f hold-input ] && [ $count -lt 200 ]; do sleep 0.02; count=$((count+1)); done ;;\n    *acme-unhandled*)",
    );
    mcp.arm("hold-input");
    let request = json!({"jsonrpc":"2.0","id":"Acme-interrupted","method":"tools/call","params":{"name":"act","arguments":{"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}},{"action":"tap","point":{"x":0.2,"y":0.2}}]}}});
    mcp.send(request.clone()).await;
    wait_for(&mcp, "input-admitted").await;
    assert!(
        Command::new("kill")
            .args(["-9", &pid])
            .status()
            .await
            .unwrap()
            .success()
    );
    std::fs::remove_file(mcp._directory.path().join("hold-input")).unwrap();
    let failed = reply(&mut mcp, "Acme-interrupted").await;
    assert_eq!(failed["ok"], false, "{failed}");
    // Status reads the durable records while no owner process runs.
    let status = mcp.target_status().await;
    assert_eq!(
        status["data"]["lastAction"]["status"], "outcome_unknown",
        "{status}"
    );
    assert_eq!(status["data"]["owner"], "absent", "{status}");
    assert_eq!(
        super::host_lifecycle::selected(&mcp).1["epoch"],
        original["epoch"]
    );
    let history = status["data"]["lastAction"].clone();
    mcp.send(request.clone()).await;
    let repeated = reply(&mut mcp, "Acme-interrupted").await;
    assert_eq!(repeated["code"], "BATCH_OUTCOME_UNKNOWN", "{repeated}");
    let mut conflicting = request;
    conflicting["params"]["arguments"]["actions"][0]["point"]["x"] = json!(0.7);
    mcp.send(conflicting).await;
    assert_eq!(
        reply(&mut mcp, "Acme-interrupted").await["code"],
        "COMMAND_ID_CONFLICT"
    );
    let observed = mcp.tool("observe", json!({})).await;
    assert_eq!(observed["data"]["lastAction"], history, "{observed}");
    assert_eq!(mcp.commands(), ["'input' 'tap' '50' '100'"]);
    assert_eq!(foreign.target_status().await["data"]["state"], "ready");
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert_eq!(
        foreign.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

fn held_media_release(mcp: &Mcp) {
    // A real helper process is reaped before the nonce-checked remote cleanup
    // command finishes. Hold that cleanup boundary independently of networking.
    std::fs::write(mcp._directory.path().join("helper.py"), r#"import hashlib, json, os, pathlib, sys
root = pathlib.Path(__file__).parent
if len(sys.argv) > 1:
    print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())
    sys.exit()
(root / 'helper.pid').write_text(str(os.getpid()))
print(json.dumps({'version': 1, 'ready': True, 'uid': 2000, 'pid': os.getpid()}), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    with (root / 'helper-requests').open('a') as log:
        log.write(request['operation'] + '\n')
    closing = request['operation'] == 'quit'
    data = {'closed': True} if closing else {'source': 'android_media_controller', 'helperEpoch': 'fixture', 'sessions': [], 'count': 0, 'partial': False}
    print(json.dumps({'version': 1, 'id': request['id'], 'ok': True, 'data': data}), flush=True)
    if closing:
        break
"#).unwrap();
    super::network_recovery::rewrite_adb(
        mcp,
        "  shell) printf",
        "  push) python3 helper.py \"$4\" > helper.digest ;;\n  shell) if [ \"$4\" = -T ]; then exec python3 helper.py; fi; printf",
    );
    super::network_recovery::rewrite_adb(
        mcp,
        "    *acme-unhandled*)",
        "    'umask 077; mkdir '*) ;;\n    'sha256sum '*) cat helper.digest ;;\n    'cat '*) cat helper.pid ;;\n    'if [ ! -d '*) touch releasing; count=0; while [ -f hold-release ] && [ $count -lt 200 ]; do sleep 0.02; count=$((count+1)); done ;;\n    *acme-unhandled*)",
    );
}

#[tokio::test]
async fn reconnect_waits_for_disconnect_and_duplicate_close_preserves_replacement() {
    let mut mcp = super::network_recovery::connected().await;
    let (_, original) = super::host_lifecycle::selected(&mcp);
    assert_eq!(
        mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
        true
    );
    held_media_release(&mcp);
    let media = mcp.tool("media_status", json!({})).await;
    assert_eq!(media["ok"], true, "{media}");
    mcp.arm("hold-release");
    let close = json!({"jsonrpc":"2.0","id":"Acme-close","method":"tools/call","params":{"name":"disconnect","arguments":{}}});
    mcp.send(close.clone()).await;
    wait_for(&mcp, "releasing").await;
    // The second client attaches only after the close began, so that close
    // was the session's last reference and is releasing the controller.
    let mut successor = mcp.successor().await;
    successor.send(json!({"jsonrpc":"2.0","id":"Acme-reconnect","method":"tools/call","params":{"name":"connect","arguments":{"targetId":original["target"]}}})).await;
    assert_eq!(
        super::host_lifecycle::selected(&mcp).1["epoch"],
        original["epoch"]
    );
    std::fs::remove_file(mcp._directory.path().join("hold-release")).unwrap();
    let closed = reply(&mut mcp, "Acme-close").await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    let recovered = reply(&mut successor, "Acme-reconnect").await;
    assert_eq!(recovered["ok"], true, "{recovered}");
    let (_, replacement) = super::host_lifecycle::selected(&mcp);
    assert_ne!(replacement["epoch"], original["epoch"]);
    let cleanup: Value = serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(original["directory"].as_str().unwrap()).join("cleanup.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(cleanup["complete"], true);
    // A repeated close request is the same request: it returns the original
    // result and cannot close the replacement.
    mcp.send(close).await;
    let stale = reply(&mut mcp, "Acme-close").await;
    assert_eq!(stale["data"], closed["data"], "{stale}");
    assert_eq!(successor.target_status().await["data"]["state"], "ready");
    assert_eq!(
        super::host_lifecycle::selected(&mcp).1["epoch"],
        replacement["epoch"]
    );
    assert_eq!(mcp.switches()["airplane"], "1");
    let commands = mcp.commands();
    assert_eq!(
        commands
            .iter()
            .filter(|line| line.starts_with('\''))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["'cmd' 'connectivity' 'airplane-mode' 'enable'"]
    );
    assert_eq!(
        commands
            .iter()
            .filter(|line| line.starts_with("if [ ! -d "))
            .count(),
        1
    );
    let requests = std::fs::read_to_string(mcp._directory.path().join("helper-requests")).unwrap();
    assert_eq!(requests.lines().filter(|line| *line == "quit").count(), 1);
    assert_eq!(
        successor.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

#[tokio::test]
async fn task_end_during_startup_answers_at_once_and_leaves_the_connect_running() {
    let serial = format!("Acme-startup-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    super::network_recovery::rewrite_adb(
        &mcp,
        "  exec-out) cat screen.png ;;",
        "  exec-out) touch observing; count=0; while [ -f hold-observation ] && [ $count -lt 200 ]; do sleep 0.02; count=$((count+1)); done; cat screen.png ;;",
    );
    mcp.arm("hold-observation");
    mcp.send(json!({"jsonrpc":"2.0","id":"Acme-starting","method":"tools/call","params":{"name":"connect","arguments":{"targetId":format!("android:{serial}")}}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !mcp._directory.path().join("observing").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The shipping CLI ends the host task while the startup is blocked: it
    // answers without waiting for the startup and releases nothing.
    let ended = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new(env!("CARGO_BIN_EXE_testrun"))
            .arg("--json")
            .args(["task", "stop"])
            .env("TESTRUN_DATA_ROOT", mcp._directory.data_root())
            .env_remove("CODEX_THREAD_ID")
            .env("TESTRUN_TASK_ID", mcp.task.as_ref().unwrap())
            .env("TESTRUN_ADB", mcp.adb.as_ref().unwrap())
            .env("ANDROID_AVD_HOME", crate::NO_AVDS)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("Task end waited for blocked startup observation")
    .unwrap();
    assert!(
        ended.status.success(),
        "{}",
        String::from_utf8_lossy(&ended.stderr)
    );
    let released: Value = serde_json::from_slice(&ended.stdout).unwrap();
    assert_eq!(released["released"], json!([]), "{released}");
    std::fs::remove_file(mcp._directory.path().join("hold-observation")).unwrap();
    let response = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let line = mcp.output.next_line().await.unwrap().unwrap();
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == "Acme-starting" {
                return response;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(response["result"]["isError"], false, "{response}");
    let status = mcp.target_status().await;
    assert_eq!(status["data"]["state"], "ready", "{status}");
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert!(mcp.commands().is_empty());
}
