//! Host task end, client departure, manager loss, automatic release retry and
//! build handover through the shipping MCP and CLI.
use super::*;

fn cli(mcp: &Mcp, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_testrun"));
    command.arg("--json");
    command
        .args(args)
        .env("TESTRUN_DATA_ROOT", mcp._directory.data_root())
        .env_remove("CODEX_THREAD_ID")
        .env("TESTRUN_ADB", mcp.adb.as_ref().unwrap())
        .env("ANDROID_AVD_HOME", crate::NO_AVDS)
        .kill_on_drop(true);
    match &mcp.task {
        Some(task) => command.env("TESTRUN_TASK_ID", task),
        None => command
            .env_remove("TESTRUN_TASK_ID")
            .current_dir(mcp._directory.path()),
    };
    command
}
pub(super) async fn run(mcp: &Mcp, args: &[&str]) -> Value {
    let output = cli(mcp, args).output().await.unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
async fn fresh_connect_preserves_android_capture_state_from_the_device_actor() {
    let serial = format!("Acme-capture-state-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    let connected = mcp
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    assert_eq!(connected["data"]["reused"], false, "{connected}");
    assert_eq!(
        connected["data"]["androidCapture"],
        json!({"requested":"screenshot","state":"screenshot"}),
        "{connected}"
    );
    let status = mcp.target_status().await;
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(
        connected["data"]["androidCapture"], status["data"]["health"]["androidCapture"],
        "{status}"
    );
    let disconnected = mcp.tool("disconnect", json!({})).await;
    assert_eq!(disconnected["ok"], true, "{disconnected}");
    assert_eq!(disconnected["data"]["closed"], true, "{disconnected}");
}

#[tokio::test]
async fn host_task_end_releases_no_device_and_preserves_network() {
    let serial = format!("Acme-task-end-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    let target = format!("android:{serial}");
    assert_eq!(
        mcp.tool("connect", json!({"targetId":target})).await["ok"],
        true
    );
    assert_eq!(
        mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
        true
    );
    let ended = mcp.request("testrun/task_end", json!({})).await;
    assert_eq!(ended["result"]["released"], json!([]), "{ended}");
    // The device stays connected for every client, including the CLI.
    let status = run(&mcp, &["status", "--target", &target]).await;
    assert_eq!(status["state"], "ready", "{status}");
    let observed = run(&mcp, &["observe", "--target", &target]).await;
    assert!(observed["path"].is_string(), "{observed}");
    let released = run(&mcp, &["task", "stop"]).await;
    assert_eq!(released["released"], json!([]), "{released}");
    assert_eq!(mcp.switches()["airplane"], "1");
    let closed = run(&mcp, &["disconnect", "--target", &target]).await;
    assert_eq!(closed["closed"], true, "{closed}");
    assert_eq!(
        mcp.commands(),
        ["'cmd' 'connectivity' 'airplane-mode' 'enable'"]
    );
}

#[tokio::test]
async fn a_failed_release_keeps_only_its_device_and_is_retried_automatically() {
    let mut mcp = super::network_recovery::connected().await;
    let mut foreign = super::network_recovery::connected().await;
    let acted = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}}]}),
        )
        .await;
    assert_eq!(acted["ok"], true, "{acted}");
    let history = mcp.target_status().await["data"]["lastAction"].clone();
    assert_eq!(
        mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
        true
    );
    let blocker = super::network_recovery::block_summary(&mut mcp).await;
    let failed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(failed["code"], "CLEANUP_FAILED", "{failed}");
    assert_eq!(failed["details"]["retrying"], true, "{failed}");
    let status = mcp.target_status().await;
    assert_eq!(status["data"]["state"], "releasing", "{status}");
    let blockers = run(&mcp, &["manager", "blockers"]).await;
    assert_eq!(blockers["blockers"][0]["kind"], "release", "{blockers}");
    // New input reaches no device while its release is unfinished.
    let refused = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}}]}),
        )
        .await;
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(foreign.target_status().await["data"]["state"], "ready");
    // Once the cause clears, the manager finishes the release by itself.
    std::fs::remove_dir(blocker).unwrap();
    let cleanup = mcp.closed_cleanup().await;
    assert!(cleanup["attempts"].as_u64().unwrap() >= 2, "{cleanup}");
    assert_eq!(cleanup["errors"][0]["attempt"], 1);
    assert_eq!(mcp.archived_action(&history).await, history);
    assert_eq!(mcp.switches()["airplane"], "1");
    assert_eq!(
        mcp.commands(),
        [
            "'input' 'tap' '50' '100'",
            "'cmd' 'connectivity' 'airplane-mode' 'enable'"
        ]
    );
    assert_eq!(
        foreign.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

#[tokio::test]
async fn failed_build_handover_keeps_epoch_and_refuses_new_input_until_cleanup_succeeds() {
    let mut mcp = super::network_recovery::connected().await;
    assert_eq!(
        mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
        true
    );
    let blocker = super::network_recovery::block_summary(&mut mcp).await;
    let (path, mut old) = selected(&mcp);
    old["buildId"] = json!("Acme-previous-build");
    std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let status = mcp.target_status().await;
    assert_eq!(status["data"]["upgradeRequired"], true);
    let refused = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}}]}),
        )
        .await;
    assert_eq!(refused["code"], "VERSION_MISMATCH", "{refused}");
    let failed = mcp.tool("connect", json!({"targetId":old["target"]})).await;
    assert_eq!(failed["code"], "CLEANUP_FAILED", "{failed}");
    assert_eq!(selected(&mcp).1["epoch"], old["epoch"]);
    let status = mcp.target_status().await;
    assert_eq!(status["data"]["nextAction"], "disconnect");
    assert!(status["data"]["lastAction"].is_null());
    std::fs::remove_dir(blocker).unwrap();
    let recovered = mcp.tool("observe", json!({})).await;
    assert_eq!(recovered["ok"], true, "{recovered}");
    assert_ne!(selected(&mcp).1["epoch"], old["epoch"]);
    assert_eq!(mcp.switches()["airplane"], "1");
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert_eq!(
        mcp.commands(),
        ["'cmd' 'connectivity' 'airplane-mode' 'enable'"]
    );
}

#[tokio::test]
async fn a_task_end_notification_releases_nothing_and_a_malformed_callback_is_refused() {
    let mut mcp = super::network_recovery::connected().await;
    let invalid = mcp
        .request("testrun/task_end", json!({"taskId":"Acme-foreign-task"}))
        .await;
    assert_eq!(invalid["error"]["code"], -32602);
    mcp.send(json!({"jsonrpc":"2.0","method":"notifications/testrun/task_end","params":{"taskId":"Acme-foreign-task"}})).await;
    assert_eq!(mcp.tool("observe", json!({})).await["ok"], true);
    mcp.send(json!({"jsonrpc":"2.0","method":"notifications/testrun/task_end"}))
        .await;
    // The device stays connected; an unlabelled CLI sees it too.
    let unbound = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .arg("--json")
        .arg("status")
        .env("TESTRUN_DATA_ROOT", mcp._directory.data_root())
        .env_remove("TESTRUN_TASK_ID")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .await
        .unwrap();
    assert!(unbound.status.success());
    let status: Value = serde_json::from_slice(&unbound.stdout).unwrap();
    assert_eq!(status["state"], "connected", "{status}");
    assert_eq!(status["targets"][0]["state"], "ready", "{status}");
    assert_eq!(run(&mcp, &["disconnect"]).await["closed"], true);
}

#[tokio::test]
async fn task_end_lets_dispatched_input_settle_and_keeps_the_device() {
    let mut mcp = super::network_recovery::connected().await;
    super::network_recovery::rewrite_adb(
        &mcp,
        "    *acme-unhandled*)",
        "    *\"'input' 'tap'\"*) touch input-admitted; while [ -f hold-input ]; do sleep 0.02; done ;;\n    *acme-unhandled*)",
    );
    mcp.arm("hold-input");
    mcp.send(json!({"jsonrpc":"2.0","id":"Acme-in-flight","method":"tools/call","params":{"name":"act","arguments":{"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}}]}}})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !mcp._directory.path().join("input-admitted").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let released = run(&mcp, &["task", "stop"]).await;
    assert_eq!(released["released"], json!([]), "{released}");
    std::fs::remove_file(mcp._directory.path().join("hold-input")).unwrap();
    let response = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let line = mcp.output.next_line().await.unwrap().unwrap();
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == "Acme-in-flight" {
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
        status["data"]["lastAction"]["status"], "completed",
        "{status}"
    );
    assert_eq!(mcp.commands(), ["'input' 'tap' '50' '100'"]);
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

/// Hold one dispatched tap, queue a second behind it, then send task end.
/// Only the dispatched tap reaches the device: the server refuses queued
/// input it received before the task end. The device stays connected.
async fn task_end_refuses_queued_input(notification: bool) {
    let mut mcp = super::network_recovery::connected().await;
    super::network_recovery::rewrite_adb(
        &mcp,
        "    *acme-unhandled*)",
        "    *\"'input' 'tap'\"*) touch input-admitted; while [ -f hold-input ]; do sleep 0.02; done ;;\n    *acme-unhandled*)",
    );
    mcp.arm("hold-input");
    let tap =
        json!({"name":"act","arguments":{"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}}]}});
    mcp.send(json!({"jsonrpc":"2.0","id":"Acme-in-flight","method":"tools/call","params":tap}))
        .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !mcp._directory.path().join("input-admitted").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mcp.send(json!({"jsonrpc":"2.0","id":"Acme-queued","method":"tools/call","params":tap}))
        .await;
    if notification {
        mcp.send(json!({"jsonrpc":"2.0","method":"notifications/testrun/task_end"}))
            .await;
    } else {
        mcp.send(json!({"jsonrpc":"2.0","id":"Acme-end","method":"testrun/task_end","params":{}}))
            .await;
    }
    // Writing stdin does not prove receipt. A later responsive ping confirms
    // task end is queued ahead of the second tap while the first is held.
    let received = mcp.request("ping", json!({})).await;
    assert_eq!(received["result"], json!({}), "{received}");
    std::fs::remove_file(mcp._directory.path().join("hold-input")).unwrap();
    let mut pending: Vec<&str> = vec!["Acme-in-flight", "Acme-queued"];
    if !notification {
        pending.push("Acme-end");
    }
    let mut responses = std::collections::HashMap::new();
    while !pending.is_empty() {
        let line = tokio::time::timeout(Duration::from_secs(30), mcp.output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        if let Some(index) = pending.iter().position(|id| response["id"] == *id) {
            responses.insert(pending.remove(index), response);
        }
    }
    let envelope = |id: &str| -> Value {
        serde_json::from_str(
            responses[id]["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("{id}: {}", responses[id])),
        )
        .unwrap()
    };
    assert_eq!(envelope("Acme-in-flight")["ok"], true);
    assert_eq!(envelope("Acme-queued")["code"], "TASK_ENDED");
    if !notification {
        assert_eq!(
            responses["Acme-end"]["result"]["released"],
            json!([]),
            "{}",
            responses["Acme-end"]
        );
    }
    assert_eq!(mcp.commands(), ["'input' 'tap' '50' '100'"]);
    assert_eq!(run(&mcp, &["disconnect"]).await["closed"], true);
}

#[tokio::test]
async fn task_end_request_refuses_queued_input() {
    task_end_refuses_queued_input(false).await;
}

#[tokio::test]
async fn task_end_notification_refuses_queued_input() {
    task_end_refuses_queued_input(true).await;
}

#[tokio::test]
async fn manager_loss_is_reconciled_when_the_device_is_released() {
    let mut mcp = super::network_recovery::connected().await;
    let target = mcp.target_status().await["data"]["targetId"].clone();
    let (_, lost) = selected(&mcp);
    mcp.kill_owner().await;
    let recovered = mcp.tool("connect", json!({"targetId":target})).await;
    assert_eq!(recovered["ok"], true, "{recovered}");
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    let cleanup: Value = serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(lost["directory"].as_str().unwrap()).join("cleanup.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(cleanup["complete"], true, "{cleanup}");
    assert_eq!(cleanup["reconciled"]["reason"], "owner_lost", "{cleanup}");
}

#[tokio::test]
async fn recovery_after_manager_loss_preserves_network_without_reconnecting() {
    let mut mcp = super::network_recovery::connected().await;
    assert_eq!(
        mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
        true
    );
    assert_eq!(mcp.switches()["airplane"], "1");
    mcp.kill_owner().await;
    // Task end needs no manager: nothing is live.
    let ended = mcp.request("testrun/task_end", json!({})).await;
    assert_eq!(ended["result"]["owner"], "absent", "{ended}");
    let lost = mcp.target_status().await;
    assert_eq!(lost["data"]["owner"], "absent", "{lost}");
    assert_ne!(lost["data"]["state"], "ready", "{lost}");
    // Durable records show unfinished cleanup, so recovery starts a manager.
    let recovered = run(&mcp, &["manager", "retry"]).await;
    assert_eq!(recovered["state"], "completed", "{recovered}");
    assert_eq!(mcp.switches()["airplane"], "1");
    mcp.closed_cleanup().await;
    assert!(mcp.commands().iter().all(|line| !line.contains("disable")));
}

#[tokio::test]
async fn after_manager_loss_two_clients_share_the_reconnected_device() {
    let mut mcp = super::network_recovery::connected().await;
    let target = mcp.target_status().await["data"]["targetId"].clone();
    assert_eq!(
        mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
        true
    );
    mcp.kill_owner().await;
    let mut other = Mcp::spawn(
        mcp._directory.clone(),
        mcp.adb.clone(),
        Some(format!("testrun-android-other-{}", uuid::Uuid::new_v4())),
    )
    .await;
    assert_eq!(
        other.tool("connect", json!({"targetId":target})).await["ok"],
        true
    );
    // The first client's connect reuses the device the other one connected.
    let shared = mcp.tool("connect", json!({"targetId":target})).await;
    assert_eq!(shared["ok"], true, "{shared}");
    assert_eq!(shared["data"]["reused"], true, "{shared}");
    assert_eq!(mcp.switches()["airplane"], "1");
    assert_eq!(other.target_status().await["data"]["state"], "ready");
    assert_eq!(
        other.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    // Released for every client.
    let gone = mcp.tool("observe", json!({})).await;
    assert_eq!(gone["code"], "NOT_CONNECTED", "{gone}");
    assert_eq!(mcp.switches()["airplane"], "1");
}

#[tokio::test]
async fn unsupported_owner_protocol_is_refused_without_touching_the_controller() {
    let mut mcp = super::network_recovery::connected().await;
    let (path, original) = selected(&mcp);
    let mut unsupported = original.clone();
    unsupported["version"] = json!(9001);
    std::fs::write(&path, serde_json::to_vec(&unsupported).unwrap()).unwrap();
    let refused = mcp
        .tool("connect", json!({"targetId":original["target"]}))
        .await;
    assert_eq!(refused["code"], "OWNER_PROTOCOL_UNSUPPORTED", "{refused}");
    assert!(
        !std::path::Path::new(original["directory"].as_str().unwrap())
            .join("stop.json")
            .exists()
    );
    assert_eq!(selected(&mcp).1, unsupported);
    std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    assert_eq!(mcp.tool("observe", json!({})).await["ok"], true);
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert!(mcp.commands().is_empty());
}

#[tokio::test]
async fn foreign_metadata_is_reported_as_a_blocker_and_never_closed() {
    let mut mcp = super::network_recovery::connected().await;
    let mut foreign = super::network_recovery::connected().await;
    let (selection, _) = selected(&mcp);
    let foreign_record = selected(&foreign).1;
    let misplaced = selection.parent().unwrap().join("o-Acme-foreign-metadata");
    std::fs::create_dir(&misplaced).unwrap();
    std::fs::write(
        misplaced.join("owner.json"),
        serde_json::to_vec(&foreign_record).unwrap(),
    )
    .unwrap();
    // The device's own actor closes; the record it cannot settle is the
    // release's blocker and is never closed or deleted.
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["code"], "CLEANUP_FAILED", "{closed}");
    assert_eq!(closed["details"]["retrying"], true, "{closed}");
    let status = mcp.target_status().await;
    assert_eq!(
        status["data"]["cleanup"]["complete"], true,
        "Valid owned cleanup must finish: {status}"
    );
    assert_eq!(foreign.target_status().await["data"]["state"], "ready");
    assert!(
        !std::path::Path::new(foreign_record["directory"].as_str().unwrap())
            .join("stop.json")
            .exists()
    );
    assert!(misplaced.join("owner.json").exists());
    std::fs::remove_file(misplaced.join("owner.json")).unwrap();
    std::fs::remove_dir(misplaced).unwrap();
    let recovered = run(&mcp, &["manager", "retry"]).await;
    assert_eq!(recovered["state"], "completed", "{recovered}");
    mcp.closed_cleanup().await;
    assert_eq!(
        foreign.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

#[tokio::test]
async fn ending_a_task_with_nothing_connected_is_idempotent_and_not_terminal() {
    let serial = format!("Acme-empty-task-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    for _ in 0..2 {
        let ended = mcp.request("testrun/task_end", json!({})).await;
        assert_eq!(
            ended["result"]["released"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0),
            0,
            "{ended}"
        );
        let status = mcp.tool("status", json!({})).await;
        assert_eq!(status["data"]["state"], "not_connected");
        assert_eq!(status["data"]["targets"], json!([]));
    }
    let connected = mcp
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert!(mcp.commands().is_empty());
}

pub(super) fn selected(mcp: &Mcp) -> (std::path::PathBuf, Value) {
    let path = mcp._directory.target_records().join("selected.json");
    let record = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    (path, record)
}

#[tokio::test]
async fn connect_hands_over_legacy_build_without_replaying_input_or_losing_history() {
    let serial = format!("Acme-build-handover-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    let target = format!("android:{serial}");
    assert_eq!(
        mcp.tool("connect", json!({"targetId":target})).await["ok"],
        true
    );
    let action = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}}]}),
        )
        .await;
    assert_eq!(action["ok"], true, "{action}");
    assert_eq!(
        mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
        true
    );
    let history = mcp.target_status().await["data"]["lastAction"].clone();
    let (path, mut old) = selected(&mcp);
    // A wire-compatible record from a build without runtime identity.
    old.as_object_mut().unwrap().remove("buildId");
    std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let connected = mcp.tool("connect", json!({"targetId":target})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    assert_eq!(
        connected["data"]["reused"], false,
        "An unidentified build must hand over: {connected}"
    );
    let (_, replacement) = selected(&mcp);
    assert_ne!(replacement["epoch"], old["epoch"]);
    assert!(
        replacement["buildId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(mcp.switches()["airplane"], "1");
    let status = mcp.target_status().await;
    assert_eq!(status["data"]["lastAction"], history);
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    assert_eq!(
        mcp.commands(),
        [
            "'input' 'tap' '50' '100'",
            "'cmd' 'connectivity' 'airplane-mode' 'enable'",
        ]
    );
}

/// Wait for the manager to exit once nothing is connected.
async fn manager_exits(owner: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while process_alive(owner).await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "a manager must exit once nothing is connected"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// An MCP server's exit releases nothing: the device stays connected for
/// the next client, and the manager exits only once the device is
/// disconnected. Neither writes network settings back.
#[tokio::test]
async fn manager_lifetime_follows_connected_devices_and_preserves_network_settings() {
    let serial = format!("Acme-unbound-{}", uuid::Uuid::new_v4().simple());
    let mut unbound = Mcp::start_unbound(&serial).await;
    let connected = unbound
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    let offline = unbound.tool("network_set", json!({"mode":"offline"})).await;
    assert_eq!(offline["ok"], true, "{offline}");
    assert_eq!(unbound.switches()["airplane"], "1");
    let owner = unbound.owner_pid().await;
    unbound.kill_server().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        process_alive(&owner).await,
        "the connected device keeps the manager past its client's exit"
    );
    let closed = run(&unbound, &["disconnect"]).await;
    assert_eq!(closed["closed"], true, "{closed}");
    manager_exits(&owner).await;
    assert_eq!(
        unbound.switches(),
        json!({"airplane":"1","wifi":"1","data":"1","bt":"1"}),
        "closing preserves requested settings"
    );

    let serial = format!("Acme-bound-{}", uuid::Uuid::new_v4().simple());
    let target_id = format!("android:{serial}");
    let mut bound = Mcp::start_named(&serial).await;
    let connected = bound.tool("connect", json!({"targetId":target_id})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    let offline = bound.tool("network_set", json!({"mode":"offline"})).await;
    assert_eq!(offline["ok"], true, "{offline}");
    let owner = bound.owner_pid().await;
    let mut successor = bound.successor().await;
    assert_eq!(successor.tool("observe", json!({})).await["ok"], true);
    bound.kill_server().await;
    assert_eq!(bound.switches()["airplane"], "1");
    let resumed = successor
        .tool("connect", json!({"targetId":target_id}))
        .await;
    assert_eq!(resumed["data"]["reused"], true, "{resumed}");
    let status = successor.target_status().await;
    assert_eq!(status["data"]["lastNetworkOperation"]["state"], "completed");
    // Any client's disconnect releases the device for every client.
    let released = successor.tool("disconnect", json!({})).await;
    assert_eq!(released["data"]["closed"], true, "{released}");
    successor.kill_server().await;
    manager_exits(&owner).await;
    assert_eq!(bound.switches()["airplane"], "1");
}
