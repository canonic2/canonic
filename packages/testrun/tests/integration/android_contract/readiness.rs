//! Android readiness through shipping CLI/MCP and the owned adb fixture.
use super::*;

async fn response(mcp: &mut Mcp) -> Value {
    serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(5), mcp.output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap()
}

async fn fixture() -> (Mcp, String) {
    let serial = format!("Acme-readiness-{}", uuid::Uuid::new_v4().simple());
    let mcp = Mcp::start_named(&serial).await;
    for (name, value) in [
        ("doctor-state", "device"),
        ("doctor-transport", "9"),
        ("doctor-release", "16"),
        ("doctor-boot", "1"),
    ] {
        std::fs::write(mcp._directory.path().join(name), value).unwrap();
    }
    std::fs::write(mcp._directory.path().join("doctor-tree"), r#"<hierarchy><node text="Acme private screen" class="android.widget.TextView" bounds="[0,0][100,200]"/></hierarchy>"#).unwrap();
    network_recovery::rewrite_adb(
        &mcp,
        &format!(
            "devices) printf 'List of devices attached\\n{serial} device usb:1-2 model:Acme transport_id:9\\n' ;;"
        ),
        &format!(
            "devices) state=$(cat doctor-state); transport=$(cat doctor-transport); printf 'List of devices attached\\n'; if [ \"$state\" != absent ]; then printf '%s %s usb:1-2 model:Acme transport_id:%s\\n' '{serial}' \"$state\" \"$transport\"; fi ;;"
        ),
    );
    network_recovery::rewrite_adb(
        &mcp,
        "  exec-out) cat screen.png ;;",
        r#"  exec-out)
    if [ -f doctor-hold ]; then echo $$ >> doctor-probe-pids; while [ -f doctor-hold ]; do sleep 0.02; done; fi
    if [ "$4" = uiautomator ]; then
      if [ -f doctor-replug ]; then echo 19 > doctor-transport; fi
      cat doctor-tree
    else cat screen.png; fi ;;"#,
    );
    network_recovery::rewrite_adb(
        &mcp,
        "    *acme-unhandled*)",
        r#"    "'echo' 'testrun-readiness'") if [ -f doctor-shell-fail ]; then exit 7; fi; echo testrun-readiness ;;
    "'getprop' 'ro.build.version.release'") cat doctor-release ;;
    "'getprop' 'sys.boot_completed'") cat doctor-boot ;;
    *acme-unhandled*)"#,
    );
    network_recovery::inspection_tree(
        &mcp,
        r#"if [ -f doctor-hold ]; then echo $$ >> doctor-probe-pids; while [ -f doctor-hold ]; do sleep 0.02; done; fi; if [ -f doctor-replug ]; then echo 19 > doctor-transport; fi; cat doctor-tree"#,
    );
    (mcp, serial)
}

#[tokio::test]
async fn unauthorized_offline_absent_and_unknown_states_do_not_probe_the_device() {
    let (mut mcp, serial) = fixture().await;
    for (observed, state) in [
        ("unauthorized", "blocked"),
        ("offline", "blocked"),
        ("absent", "blocked"),
        ("recovery", "unknown"),
    ] {
        std::fs::write(mcp._directory.path().join("doctor-state"), observed).unwrap();
        let result = mcp
            .tool("doctor", json!({"targetId":format!("android:{serial}")}))
            .await;
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["data"]["ready"], false, "{result}");
        assert_eq!(
            result["data"]["checks"]["authorization"]["state"], state,
            "{result}"
        );
        assert_eq!(
            result["data"]["checks"]["authorization"]["observedState"], observed,
            "{result}"
        );
        assert!(result["data"]["checks"]["authorization"]["manualAction"].is_string());
        assert_eq!(result["data"]["checks"]["shell"]["state"], "unknown");
        assert!(!mcp.log.exists());
    }
}

#[tokio::test]
async fn partial_boot_capture_and_hierarchy_checks_remain_distinct_and_replug_is_unknown() {
    let (mut mcp, serial) = fixture().await;
    std::fs::write(mcp._directory.path().join("doctor-boot"), "0").unwrap();
    std::fs::write(mcp._directory.path().join("sdk"), "unknown").unwrap();
    std::fs::write(
        mcp._directory.path().join("screen.png"),
        "Acme invalid capture",
    )
    .unwrap();
    let partial = mcp
        .tool("doctor", json!({"targetId":format!("android:{serial}")}))
        .await;
    let checks = &partial["data"]["checks"];
    assert_eq!(checks["os"]["state"], "unknown", "{partial}");
    assert_eq!(checks["os"]["release"], "16");
    assert_eq!(checks["boot"]["state"], "blocked");
    assert_eq!(checks["screenshot"]["state"], "blocked");
    assert_eq!(checks["accessibility"]["state"], "ready");
    image::RgbImage::new(100, 200)
        .save(mcp._directory.path().join("screen.png"))
        .unwrap();
    std::fs::write(mcp._directory.path().join("sdk"), "36").unwrap();
    std::fs::write(mcp._directory.path().join("doctor-boot"), "").unwrap();
    std::fs::write(mcp._directory.path().join("doctor-tree"), "<hierarchy/>").unwrap();
    let unknown = mcp
        .tool("doctor", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(unknown["data"]["state"], "unknown", "{unknown}");
    assert_eq!(unknown["data"]["checks"]["boot"]["state"], "unknown");
    assert_eq!(unknown["data"]["checks"]["screenshot"]["state"], "ready");
    assert_eq!(
        unknown["data"]["checks"]["accessibility"]["state"],
        "unknown"
    );
    std::fs::write(mcp._directory.path().join("doctor-boot"), "1").unwrap();
    std::fs::write(
        mcp._directory.path().join("doctor-tree"),
        "Acme unavailable hierarchy",
    )
    .unwrap();
    mcp.arm("doctor-replug");
    let replug = mcp
        .tool("doctor", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(replug["data"]["ready"], false, "{replug}");
    assert_eq!(replug["data"]["checks"]["transport"]["state"], "unknown");
    assert_eq!(
        replug["data"]["checks"]["accessibility"]["state"],
        "blocked"
    );
}

#[tokio::test]
async fn shell_refusal_and_missing_transport_never_certify_readiness() {
    let (mut mcp, serial) = fixture().await;
    mcp.arm("doctor-shell-fail");
    let failed = mcp
        .tool("doctor", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(
        failed["data"]["checks"]["shell"]["state"], "blocked",
        "{failed}"
    );
    assert_eq!(failed["data"]["checks"]["screenshot"]["state"], "unknown");
    std::fs::remove_file(mcp._directory.path().join("doctor-shell-fail")).unwrap();
    std::fs::write(mcp._directory.path().join("doctor-transport"), "unknown").unwrap();
    let unknown = mcp
        .tool("doctor", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(unknown["data"]["state"], "unknown", "{unknown}");
    assert_eq!(unknown["data"]["checks"]["transport"]["state"], "unknown");
    assert_eq!(unknown["data"]["checks"]["screenshot"]["state"], "ready");
    assert_eq!(unknown["data"]["checks"]["accessibility"]["state"], "ready");
}

#[tokio::test]
async fn cli_doctor_and_probed_listing_expose_the_same_readiness_without_selection() {
    let (mut mcp, serial) = fixture().await;
    let listed = mcp
        .tool("target_list", json!({"kind":"android","probe":true}))
        .await;
    assert_eq!(
        listed["data"]["targets"][0]["readiness"]["ready"], true,
        "{listed}"
    );
    let result = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .arg("--json")
        .args(["doctor", "--target", &format!("android:{serial}")])
        .env("TESTRUN_DATA_ROOT", mcp._directory.data_root())
        .env("TESTRUN_ADB", mcp.adb.as_ref().unwrap())
        .env("ANDROID_AVD_HOME", crate::NO_AVDS)
        .env_remove("TESTRUN_TASK_ID")
        .env_remove("CODEX_THREAD_ID")
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report, listed["data"]["targets"][0]["readiness"]);
    let no_id = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .arg("--json")
        .arg("doctor")
        .env("TESTRUN_DATA_ROOT", mcp._directory.data_root())
        .env("TESTRUN_ADB", mcp.adb.as_ref().unwrap())
        .env("ANDROID_AVD_HOME", crate::NO_AVDS)
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(!no_id.status.success());
    assert!(String::from_utf8_lossy(&no_id.stderr).contains("--target"));
}

#[tokio::test]
async fn cancelled_doctor_disposes_its_probes_and_preserves_status_under_saturation() {
    let (mut mcp, serial) = fixture().await;
    mcp.arm("doctor-hold");
    for index in 0..4 {
        mcp.send(json!({"jsonrpc":"2.0","id":format!("doctor-{index}"),"method":"tools/call","params":{"name":"doctor","arguments":{"targetId":format!("android:{serial}")}}})).await;
    }
    let processes = mcp._directory.path().join("doctor-probe-pids");
    tokio::time::timeout(Duration::from_secs(5), async {
        while std::fs::read_to_string(&processes)
            .unwrap_or_default()
            .lines()
            .count()
            < 4
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let pids = std::fs::read_to_string(&processes).unwrap();
    mcp.send(json!({"jsonrpc":"2.0","id":"excess","method":"tools/call","params":{"name":"doctor","arguments":{"targetId":format!("android:{serial}")}}})).await;
    let excess = response(&mut mcp).await;
    assert_eq!(excess["id"], "excess", "{excess}");
    assert_eq!(excess["error"]["code"], -32000, "{excess}");
    let status = mcp.tool("status", json!({})).await;
    assert_eq!(status["data"]["state"], "not_connected");
    assert_eq!(status["data"]["targets"], json!([]));
    for index in 0..4 {
        mcp.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":format!("doctor-{index}")}})).await;
    }
    mcp.cancellation_barrier().await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    for pid in pids.lines() {
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
                "doctor probe survived cancellation"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    std::fs::remove_file(mcp._directory.path().join("doctor-hold")).unwrap();
    let next = mcp
        .tool("doctor", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(next["data"]["ready"], true, "{next}");
}

#[tokio::test]
async fn diagnosis_preserves_an_existing_controller_and_its_action_history() {
    let (mut mcp, serial) = fixture().await;
    let connected = mcp
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    let before = mcp.target_status().await;
    let report = mcp
        .tool("doctor", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(report["data"]["ready"], true, "{report}");
    let cli = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .arg("--json")
        .args(["doctor", "--target", &format!("android:{serial}")])
        .env("TESTRUN_DATA_ROOT", mcp._directory.data_root())
        .env("TESTRUN_ADB", mcp.adb.as_ref().unwrap())
        .env("ANDROID_AVD_HOME", crate::NO_AVDS)
        .env("TESTRUN_TASK_ID", mcp.task.as_ref().unwrap())
        .env_remove("CODEX_THREAD_ID")
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let after = mcp.target_status().await;
    assert_eq!(after["data"]["state"], "ready", "{after}");
    assert_eq!(after["data"]["targetId"], before["data"]["targetId"]);
    assert_eq!(after["data"]["lastAction"], before["data"]["lastAction"]);
    let reused = mcp
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(reused["data"]["reused"], true, "{reused}");
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

#[tokio::test]
async fn explicit_doctor_reports_observed_readiness_without_connecting_or_input() {
    let (mut mcp, serial) = fixture().await;
    let report = mcp
        .tool("doctor", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(report["ok"], true, "{report}");
    let data = &report["data"];
    assert_eq!(data["ready"], true, "{data}");
    assert_eq!(data["checks"]["os"]["apiLevel"], 36, "{data}");
    assert_eq!(data["checks"]["screenshot"]["width"], 100, "{data}");
    assert_eq!(data["checks"]["accessibility"]["nodeCount"], 1, "{data}");
    assert!(!data.to_string().contains("Acme private screen"));
    let status = mcp.tool("status", json!({})).await;
    assert_eq!(status["data"]["state"], "not_connected");
    assert_eq!(status["data"]["targets"], json!([]));
    let calls = std::fs::read_to_string(mcp._directory.path().join("input.log")).unwrap();
    assert!(
        calls.contains("io.canonic.testrun.focus.Main snapshot"),
        "{calls}"
    );
    assert!(
        !calls
            .lines()
            .any(|line| ["'input'", "'am'", "'monkey'", "'svc'"]
                .iter()
                .any(|prefix| line.starts_with(prefix))),
        "{calls}"
    );
}
