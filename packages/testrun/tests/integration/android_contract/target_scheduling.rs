//! Target inventory, connection churn and bounded reads through the shipping MCP boundary.
use super::*;

const HELD_READS: &str = r#"  exec-out)
    held=0
    if [ "$4" = uiautomator ]; then held=1; else
      for slot in 0 1 2; do if mkdir "capture-$slot" 2>/dev/null; then held=1; break; fi; done
    fi
    if [ "$held" = 1 ]; then echo $$ >> readers; touch "reader-$$"; count=0; while [ -f hold-reads ] && [ $count -lt 300 ]; do sleep 0.02; count=$((count+1)); done; fi
    if [ "$4" = uiautomator ]; then printf '<hierarchy rotation="0"><node text="Acme" class="android.widget.TextView" bounds="[0,0][100,200]"/></hierarchy>'; else cat screen.png; fi ;;
"#;

async fn response(mcp: &mut Mcp) -> Value {
    serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(3), mcp.output.next_line())
            .await
            .expect("independent request was blocked")
            .unwrap()
            .unwrap(),
    )
    .unwrap()
}

fn held_inspection(mcp: &Mcp) {
    super::network_recovery::inspection_tree(
        mcp,
        r#"echo $$ >> readers; touch "reader-$$"; count=0; while [ -f hold-reads ] && [ $count -lt 400 ]; do sleep 0.02; count=$((count+1)); done; printf '<hierarchy><node text="Acme" class="android.widget.TextView" bounds="[0,0][100,200]"/></hierarchy>'"#,
    );
}

fn envelope(response: &Value) -> Value {
    response["result"]["structuredContent"].clone()
}
async fn wait_file(mcp: &Mcp, name: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !mcp._directory.path().join(name).exists() {
        assert!(tokio::time::Instant::now() < deadline, "missing {name}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
async fn send_tool(mcp: &mut Mcp, id: &str, name: &str, args: Value) {
    mcp.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}})).await;
}
async fn readers_stopped(mcp: &Mcp) {
    let pids = std::fs::read_to_string(mcp._directory.path().join("readers")).unwrap();
    assert!(!pids.trim().is_empty());
    for pid in pids.lines() {
        assert!(pid.parse::<u32>().unwrap() > 1, "invalid read PID");
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
                "read process survived cancellation: {pid}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}
fn tap() -> Value {
    json!({"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}}]})
}
/// A server connected to its own fake target.
async fn connected(prefix: &str) -> Mcp {
    let serial = format!("{prefix}-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    let connected = mcp
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    mcp
}

#[tokio::test]
async fn superseded_inventory_omits_stale_targets_and_preserves_healthy_generations() {
    let serial = format!("Acme-inventory-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    let healthy = format!("{serial} device usb:1-2 model:Acme transport_id:9\\n");
    let removed = format!("{serial}-removed device usb:1-3 model:Acme transport_id:10\\n");
    let replaced = format!("{serial}-replaced device usb:1-4 model:Acme transport_id:11\\n");
    let replacement = replaced.replace("transport_id:11", "transport_id:12");
    let old = format!("List of devices attached\\n{healthy}{removed}{replaced}");
    let new = format!("List of devices attached\\n{healthy}{replacement}");
    super::network_recovery::rewrite_adb(
        &mcp,
        &format!("devices) printf 'List of devices attached\\n{healthy}' ;;"),
        &format!(
            "devices) if mkdir inventory-first 2>/dev/null; then touch inventory-held; count=0; while [ -f hold-inventory ] && [ $count -lt 300 ]; do sleep 0.02; count=$((count+1)); done; printf '{old}'; else printf '{new}'; fi ;;"
        ),
    );
    mcp.arm("hold-inventory");
    send_tool(&mut mcp, "older", "target_list", json!({"kind":"android"})).await;
    wait_file(&mcp, "inventory-held").await;
    send_tool(&mut mcp, "newer", "target_list", json!({"kind":"android"})).await;
    let newer = response(&mut mcp).await;
    std::fs::remove_file(mcp._directory.path().join("hold-inventory")).unwrap();
    assert_eq!(newer["id"], "newer", "{newer}");
    let current = envelope(&newer);
    assert_eq!(current["ok"], true, "{current}");
    assert_eq!(current["data"]["targets"].as_array().unwrap().len(), 2);
    let healthy = current["data"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["targetId"] == format!("android:{serial}"))
        .unwrap();
    let older = response(&mut mcp).await;
    assert_eq!(older["id"], "older", "{older}");
    let reconciled = envelope(&older);
    assert_eq!(reconciled["ok"], true, "{reconciled}");
    assert_eq!(
        reconciled["data"]["targets"],
        json!([healthy]),
        "{reconciled}"
    );
    // The older completion must not remove or resurrect authoritative entries.
    let final_list = mcp.tool("target_list", json!({"kind":"android"})).await;
    assert_eq!(final_list["ok"], true, "{final_list}");
    assert_eq!(final_list["data"]["targets"], current["data"]["targets"]);
}

/// Connecting to a sequence of targets keeps the first action's outcome
/// readable after its connection closed, and shutdown leaves no scratch.
#[tokio::test]
async fn closed_target_churn_keeps_action_outcomes() {
    let serial = format!("Acme-retirement-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    let mut previous = serial.clone();
    let mut first = None;
    for index in 0..4 {
        let serial = format!("{serial}-{index}");
        super::network_recovery::rewrite_adb(&mcp, &previous, &serial);
        previous.clone_from(&serial);
        let connected = mcp
            .tool("connect", json!({"targetId":format!("android:{serial}")}))
            .await;
        assert_eq!(connected["ok"], true, "target {index}: {connected}");
        if index == 0 {
            let acted = mcp.tool("act", tap()).await;
            assert_eq!(acted["ok"], true, "{acted}");
            first = Some(acted["data"]["lastAction"]["actionId"].clone());
        }
        assert_eq!(
            mcp.tool("disconnect", json!({})).await["data"]["closed"],
            true
        );
    }
    let first = first.unwrap();
    assert!(first.is_string(), "{first}");
    let archived = mcp.tool("status", json!({"actionId":first})).await;
    assert_eq!(
        archived["data"]["action"]["status"], "completed",
        "{archived}"
    );
    assert_no_runtime_scratch(&mcp);
    drop(mcp.input.take());
    assert!(
        tokio::time::timeout(Duration::from_secs(3), mcp.child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert_no_runtime_scratch(&mcp);
}

#[tokio::test]
async fn runtime_scratch_is_not_left_behind_after_connection_shutdown() {
    let mut mcp = connected("Acme-scratch").await;
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    mcp.arm("Acme-unrelated-evidence");
    drop(mcp.input.take());
    assert!(
        tokio::time::timeout(Duration::from_secs(3), mcp.child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert_no_runtime_scratch(&mcp);
    assert!(
        mcp._directory
            .path()
            .join("Acme-unrelated-evidence")
            .exists()
    );
}

fn assert_no_runtime_scratch(mcp: &Mcp) {
    let scratch = std::fs::read_dir(mcp._directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with("testrun-runtime-"))
        .collect::<Vec<_>>();
    assert!(scratch.is_empty(), "unowned runtime scratch: {scratch:?}");
}

/// A disconnect queued behind held input and cancelled before admission
/// leaves the connection usable.
#[tokio::test]
async fn cancelled_queued_disconnect_leaves_the_connection_readable() {
    let mut mcp = connected("Acme-queued-close").await;
    super::network_recovery::rewrite_adb(
        &mcp,
        "    *acme-unhandled*)",
        "    *\"'input' 'tap'\"*) touch input-held; count=0; while [ -f hold-input ] && [ $count -lt 300 ]; do sleep 0.02; count=$((count+1)); done ;;\n    *acme-unhandled*)",
    );
    mcp.arm("hold-input");
    send_tool(&mut mcp, "input", "act", tap()).await;
    wait_file(&mcp, "input-held").await;
    send_tool(&mut mcp, "queued-close", "disconnect", json!({})).await;
    mcp.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"queued-close"}})).await;
    mcp.cancellation_barrier().await;
    std::fs::remove_file(mcp._directory.path().join("hold-input")).unwrap();
    let input = tokio::time::timeout(Duration::from_secs(15), mcp.output.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let input: Value = serde_json::from_str(&input).unwrap();
    assert_eq!(input["id"], "input");
    assert_eq!(envelope(&input)["ok"], true, "{input}");
    let status = mcp.target_status().await;
    assert_eq!(status["data"]["state"], "ready", "{status}");
    let observed = mcp.tool("observe", json!({})).await;
    assert_eq!(observed["ok"], true, "{observed}");
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

#[tokio::test]
async fn inspection_preserves_nodes_when_android_display_geometry_is_unavailable() {
    let mut mcp = connected("Acme-inspection-no-display").await;
    super::network_recovery::inspection_tree(
        &mcp,
        r#"printf '<hierarchy><node text="Acme" class="android.widget.TextView" bounds="[0,0][100,200]"/></hierarchy>'"#,
    );
    // No input viewport in this fixture, and the screenshot fallback refuses.
    super::network_recovery::rewrite_adb(
        &mcp,
        "  exec-out) cat screen.png ;;",
        "  exec-out) exit 1 ;;",
    );
    for tool in ["ui_snapshot", "ui_find"] {
        let mut args = json!({"scope":{"kind":"display"},"allowPartial":true});
        if tool == "ui_find" {
            args["query"] = json!({"role":"text"});
        }
        let result = mcp.tool(tool, args).await;
        assert_eq!(result["ok"], true, "{result}");
        let data = &result["data"];
        assert_eq!(data["frameUnit"], "pixels", "{result}");
        assert_eq!(data.get("display"), Some(&Value::Null), "{result}");
        let artifact: Value =
            serde_json::from_slice(&std::fs::read(data["artifactPath"].as_str().unwrap()).unwrap())
                .unwrap();
        assert_eq!(artifact.get("display"), Some(&Value::Null));
        if tool == "ui_find" {
            let matches = data["data"]["matches"].as_array().unwrap();
            assert_eq!(matches.len(), 1, "{result}");
            assert_eq!(matches[0]["label"], "Acme");
            assert!(matches[0].get("center").is_none(), "{result}");
        } else {
            assert!(artifact.to_string().contains("Acme"));
        }
    }
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

#[tokio::test]
async fn mcp_observe_requires_a_connection_and_cli_observe_connects_once() {
    let serial = format!("Acme-unconnected-image-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    // Model the raw format so each acquisition needs only one screencap call.
    let mut raw = Vec::new();
    for word in [100_u32, 200, 2] {
        raw.extend_from_slice(&word.to_le_bytes());
    }
    raw.resize(12 + 100 * 200 * 4, 0);
    std::fs::write(mcp._directory.path().join("screen.raw"), raw).unwrap();
    super::network_recovery::rewrite_adb(
        &mcp,
        "  exec-out) cat screen.png ;;",
        "  exec-out) echo capture >> captures; if [ \"${5-}\" = -p ]; then cat screen.png; else cat screen.raw; fi ;;",
    );
    super::network_recovery::rewrite_adb(
        &mcp,
        "    *acme-unhandled*)",
        "    *dumpsys*input*) printf 'Viewport INTERNAL: displayId=0, logicalFrame=[0, 0, 100, 200]\\n' ;;\n    *acme-unhandled*)",
    );
    let target = format!("android:{serial}");
    let capture_log = mcp._directory.path().join("captures");
    let captures = || {
        std::fs::read_to_string(&capture_log)
            .map(|log| log.lines().count())
            .unwrap_or(0)
    };
    let refused = mcp.tool("observe", json!({"targetId":target})).await;
    assert_eq!(refused["code"], "NOT_CONNECTED", "{refused}");
    assert_eq!(refused["details"]["nextAction"], "connect", "{refused}");
    assert_eq!(captures(), 0, "A refused observe must not touch the device");
    let status = mcp.target_status().await;
    assert_eq!(status["code"], "NOT_CONNECTED", "{status}");
    assert_eq!(status["details"]["dispatched"], false, "{status}");
    let connected = super::host_lifecycle::run(&mcp, &["--target", &target, "observe"]).await;
    assert!(connected["path"].is_string(), "{connected}");
    assert_eq!(
        captures(),
        1,
        "The first CLI read should reuse the startup capture"
    );
    let observed = mcp.tool("observe", json!({"targetId":target})).await;
    assert_eq!(observed["ok"], true, "{observed}");
    assert!(captures() > 1, "{observed}");
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

#[tokio::test]
async fn blocked_action_observations_preserve_completed_inputs_and_later_steps() {
    let mut mcp = connected("Acme-blocked-observations").await;
    super::network_recovery::rewrite_adb(
        &mcp,
        "  exec-out) cat screen.png ;;",
        "  exec-out) exit 0 ;;",
    );
    super::network_recovery::rewrite_adb(
        &mcp,
        "    *mCurrentFocus*)",
        "    *dumpsys*input*) printf 'Viewport INTERNAL: displayId=0, logicalFrame=[0, 0, 100, 200]\\n' ;;\n    *dumpsys*window*) printf 'Window #0 Window{acme u0 com.example.acme/.Main}:\\n  fl=SECURE\\n  isVisible=true\\nmCurrentFocus=Window{acme u0 com.example.acme/.Main}\\n' ;;\n    *mCurrentFocus*)",
    );
    for each in [true, false] {
        let result = mcp
            .tool(
                "act",
                json!({"actions":[
            {"action":"tap","point":{"x":0.5,"y":0.5}},
            {"action":"tap","point":{"x":0.5,"y":0.5}}
        ],"settle":false,"observeAfterEach":each}),
            )
            .await;
        assert_eq!(result["ok"], true, "{result}");
        let batch = &result["data"]["result"];
        assert_eq!(batch["status"], "completed", "{result}");
        let steps = batch["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 2);
        assert!(steps.iter().all(|step| step["ok"] == true));
        if each {
            assert!(
                steps
                    .iter()
                    .all(|step| step["observationError"]["code"] == "CAPTURE_BLOCKED")
            );
        } else {
            assert_eq!(batch["observationError"]["code"], "CAPTURE_BLOCKED");
        }
    }
    assert_eq!(
        mcp.commands()
            .iter()
            .filter(|line| line.as_str() == "'input' 'tap' '50' '100'")
            .count(),
        4
    );
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

#[tokio::test]
async fn inspection_keeps_its_budget_while_status_remains_responsive() {
    let mut mcp = connected("Acme-inspection").await;
    super::network_recovery::rewrite_adb(&mcp, "  exec-out) cat screen.png ;;", HELD_READS);
    held_inspection(&mcp);
    mcp.arm("hold-reads");
    for (id, tool) in [("snapshot", "ui_snapshot"), ("find", "ui_find")] {
        let mut args = json!({"scope":{"kind":"display"},"allowPartial":true});
        if id == "find" {
            args["query"] = json!({"role":"text"});
        }
        send_tool(&mut mcp, id, tool, args).await;
    }
    wait_file(&mcp, "readers").await;
    send_target_status(&mut mcp, "status").await;
    let status = response(&mut mcp).await;
    assert_eq!(status["id"], "status", "{status}");
    assert_eq!(envelope(&status)["data"]["state"], "ready");
    // Native inspection has its own adapter deadline. It must not acquire
    // the screenshot's shorter five-second budget during scheduling.
    tokio::time::sleep(Duration::from_secs(6)).await;
    std::fs::remove_file(mcp._directory.path().join("hold-reads")).unwrap();
    for _ in 0..2 {
        let result = response(&mut mcp).await;
        assert!(matches!(result["id"].as_str(), Some("snapshot" | "find")));
        assert_eq!(envelope(&result)["ok"], true, "{result}");
    }
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

#[tokio::test]
async fn capture_and_inspection_saturation_preserve_input_status_and_owned_close() {
    let mut mcp = connected("Acme-reads").await;
    super::network_recovery::rewrite_adb(&mcp, "  exec-out) cat screen.png ;;", HELD_READS);
    held_inspection(&mcp);
    mcp.arm("hold-reads");
    for id in 0..3 {
        send_tool(&mut mcp, &format!("capture-{id}"), "observe", json!({})).await;
    }
    send_tool(
        &mut mcp,
        "inspection",
        "ui_snapshot",
        json!({"scope":{"kind":"display"},"allowPartial":true}),
    )
    .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if std::fs::read_to_string(mcp._directory.path().join("readers"))
            .unwrap_or_default()
            .lines()
            .count()
            == 4
        {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    send_tool(&mut mcp, "overflow", "observe", json!({})).await;
    send_target_status(&mut mcp, "status").await;
    send_tool(&mut mcp, "input", "act", tap()).await;
    let mut replies = std::collections::HashMap::new();
    let mut overflow = Value::Null;
    for _ in 0..3 {
        let reply = response(&mut mcp).await;
        if reply["id"] == "overflow" {
            overflow = reply.clone();
        }
        replies.insert(reply["id"].as_str().unwrap().to_owned(), envelope(&reply));
    }
    // A fifth concurrent read is refused at the MCP read capacity, not queued.
    assert_eq!(overflow["error"]["code"], -32000, "{overflow}");
    assert_eq!(replies["status"]["data"]["state"], "ready");
    assert_eq!(replies["input"]["ok"], true, "{replies:?}");
    // Closing the owner cancels all held reads, including their process groups.
    send_tool(&mut mcp, "close", "disconnect", json!({})).await;
    let mut closed = false;
    for _ in 0..5 {
        let reply = tokio::time::timeout(Duration::from_secs(15), mcp.output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(&reply).unwrap();
        if reply["id"] == "close" {
            assert_eq!(envelope(&reply)["data"]["closed"], true, "{reply}");
            closed = true;
        } else {
            assert_eq!(envelope(&reply)["ok"], false, "{reply}");
        }
    }
    assert!(closed);
    // Reads held across the disconnect fail; none reconnects the device.
    mcp.closed_cleanup().await;
    readers_stopped(&mcp).await;
    assert_eq!(
        std::fs::read_to_string(mcp.log.clone())
            .unwrap()
            .lines()
            .filter(|line| line.contains("'input' 'tap'"))
            .count(),
        1
    );
    std::fs::remove_file(mcp._directory.path().join("hold-reads")).unwrap();
}

#[tokio::test]
async fn retained_reads_are_bounded_across_cli_clients_and_do_not_hold_input() {
    let serial = format!("Acme-retained-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    assert_eq!(
        mcp.tool("connect", json!({"targetId":format!("android:{serial}")}))
            .await["ok"],
        true
    );
    super::network_recovery::rewrite_adb(
        &mcp,
        "  exec-out) cat screen.png ;;",
        r#"  exec-out)
      held=0; for slot in 0 1 2 3; do if mkdir "capture-$slot" 2>/dev/null; then held=1; break; fi; done
      if [ "$held" = 1 ]; then echo $$ >> readers; count=0; while [ -f hold-reads ] && [ $count -lt 300 ]; do sleep 0.02; count=$((count+1)); done; fi; cat screen.png ;;
"#,
    );
    mcp.arm("hold-reads");
    for id in 0..4 {
        send_tool(&mut mcp, &format!("read-{id}"), "observe", json!({})).await;
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if std::fs::read_to_string(mcp._directory.path().join("readers"))
            .unwrap_or_default()
            .lines()
            .count()
            == 4
        {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let overflow = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .arg("--json")
        .arg("observe")
        .env("TESTRUN_DATA_ROOT", mcp._directory.data_root())
        .env_remove("CODEX_THREAD_ID")
        .env("TESTRUN_TASK_ID", mcp.task.as_ref().unwrap())
        .env("TESTRUN_ADB", mcp.adb.as_ref().unwrap())
        .env("ANDROID_AVD_HOME", crate::NO_AVDS)
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(!overflow.status.success());
    assert!(
        String::from_utf8_lossy(&overflow.stderr).contains("OVERLOADED"),
        "{}",
        String::from_utf8_lossy(&overflow.stderr)
    );
    send_target_status(&mut mcp, "status").await;
    send_tool(&mut mcp, "input", "act", tap()).await;
    let mut replies = std::collections::HashMap::new();
    for _ in 0..2 {
        let reply = response(&mut mcp).await;
        replies.insert(reply["id"].as_str().unwrap().to_owned(), envelope(&reply));
    }
    assert_eq!(replies["status"]["data"]["state"], "ready");
    assert_eq!(replies["input"]["ok"], true, "{replies:?}");
    for id in 0..4 {
        mcp.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":format!("read-{id}")}})).await;
    }
    mcp.cancellation_barrier().await;
    readers_stopped(&mcp).await;
    std::fs::remove_file(mcp._directory.path().join("hold-reads")).unwrap();
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

async fn send_target_status(mcp: &mut Mcp, id: &str) {
    let target = mcp.status_target.clone().unwrap();
    send_tool(mcp, id, "status", json!({"targetId":target})).await;
}
