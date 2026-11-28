//! Network failure boundaries through the published MCP and owned adb fixtures.
use super::*;

pub(super) fn rewrite_adb(mcp: &Mcp, from: &str, to: &str) {
    let path = mcp.adb.as_ref().unwrap();
    let source = std::fs::read_to_string(path).unwrap();
    assert!(source.contains(from), "The adb fixture hook must exist");
    // A recording can still be launching/reading this script. Publish a new
    // executable atomically so it sees either complete version, never a
    // truncated in-place rewrite or an executable currently open for writing.
    let replacement = path.with_extension("next");
    std::fs::write(&replacement, source.replace(from, to)).unwrap();
    std::fs::set_permissions(&replacement, std::fs::metadata(path).unwrap().permissions()).unwrap();
    std::fs::rename(replacement, path).unwrap();
}

/// Serve an owned tree through the one-shot automation process, preserving screencap.
pub(super) fn inspection_tree(mcp: &Mcp, body: &str) {
    rewrite_adb(
        mcp,
        "  shell) printf",
        "  shell) if [ \"$4\" = -T ]; then set -- \"$1\" \"$2\" \"$3\" \"$5\"; fi; printf",
    );
    rewrite_adb(
        mcp,
        "    *acme-unhandled*)",
        &format!(
            "    *io.canonic.testrun.focus.Main\\ snapshot*) cat > /dev/null; {body} ;;\n    *acme-unhandled*)"
        ),
    );
}

pub(super) async fn connected() -> Mcp {
    let serial = format!("Acme-network-recovery-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    let result = mcp
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(result["ok"], true, "{result}");
    mcp
}

/// Fail close at the real evidence-publication boundary, independently of radios.
pub(super) async fn block_summary(mcp: &mut Mcp) -> std::path::PathBuf {
    let observed = mcp.tool("observe", json!({})).await;
    assert_eq!(observed["ok"], true, "{observed}");
    let blocker = std::path::Path::new(observed["data"]["observation"]["path"].as_str().unwrap())
        .parent()
        .unwrap()
        .join("summary.json");
    std::fs::create_dir(&blocker).unwrap();
    blocker
}

#[tokio::test]
async fn disconnect_preserves_initially_disabled_radios_and_requested_airplane_mode() {
    let serial = format!("Acme-offline-baseline-{}", uuid::Uuid::new_v4().simple());
    let mut mcp = Mcp::start_named(&serial).await;
    for name in ["wifi", "data", "bt"] {
        std::fs::write(mcp._directory.path().join(name), "0\n").unwrap();
    }
    let connected = mcp
        .tool("connect", json!({"targetId":format!("android:{serial}")}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    let offline = mcp
        .tool("network_set", json!({"mode":"offline","method":"airplane"}))
        .await;
    assert_eq!(offline["ok"], true, "{offline}");
    assert_eq!(mcp.switches()["airplane"], "1");
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["cleanup"]["complete"], true, "{closed}");
    assert_eq!(
        mcp.switches(),
        json!({"airplane":"1","wifi":"0","data":"0","bt":"0"})
    );
    assert!(
        !mcp.commands()
            .iter()
            .any(|line| line == "'svc' 'wifi' 'enable'" || line == "'svc' 'data' 'enable'")
    );
}

#[tokio::test]
async fn malformed_managed_close_replies_without_starting_cleanup() {
    use std::os::unix::fs::MetadataExt;
    use tokio::io::AsyncReadExt;

    let mut mcp = connected().await;
    assert_eq!(
        mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
        true
    );
    let selected = mcp._directory.target_records().join("selected.json");
    let connection: Value = serde_json::from_slice(&std::fs::read(selected).unwrap()).unwrap();
    let socket =
        std::path::Path::new(connection["directory"].as_str().unwrap()).join("s/control.sock");
    let inode = std::fs::metadata(&socket).unwrap().ino();
    let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let input = serde_json::to_vec(
        &json!({"kind":"close","ownerEpoch":connection["epoch"],"unexpected":true}),
    )
    .unwrap();
    stream.write_u32(input.len() as u32).await.unwrap();
    stream.write_all(&input).await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(5), async {
        let length = stream
            .read_u32()
            .await
            .expect("Malformed close must receive a refusal");
        assert!(length <= 1024 * 1024);
        let mut bytes = vec![0; length as usize];
        stream.read_exact(&mut bytes).await.unwrap();
        serde_json::from_slice::<Value>(&bytes).unwrap()
    })
    .await
    .unwrap();
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["error"]["code"], "INVALID_ARGUMENT", "{response}");
    // Losing a refusal reply cannot turn invalid input into shutdown admission.
    let mut abandoned = tokio::net::UnixStream::connect(&socket).await.unwrap();
    abandoned.write_u32(input.len() as u32).await.unwrap();
    abandoned.write_all(&input).await.unwrap();
    drop(abandoned);
    let status = mcp.target_status().await;
    assert_eq!(status["data"]["state"], "ready", "{status}");
    assert_eq!(std::fs::metadata(&socket).unwrap().ino(), inode);
    assert_eq!(mcp.switches()["airplane"], "1");
    assert_eq!(mcp.tool("observe", json!({})).await["ok"], true);
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert_eq!(mcp.switches()["airplane"], "1");
    assert_eq!(
        mcp.commands(),
        ["'cmd' 'connectivity' 'airplane-mode' 'enable'"]
    );
}

#[tokio::test]
async fn failed_foreground_close_preserves_error_status_and_retry() {
    let serial = format!("Acme-close-retry-{}", uuid::Uuid::new_v4().simple());
    let fixture = Mcp::start_named(&serial).await;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("controller");
    let mut child = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .arg("--json")
        .args([
            "session",
            "--target",
            &format!("android:{serial}"),
            "--directory",
        ])
        .arg(&directory)
        .env_remove("TESTRUN_TASK_ID")
        .env_remove("CODEX_THREAD_ID")
        .env("TESTRUN_ADB", fixture.adb.as_ref().unwrap())
        .env("ANDROID_AVD_HOME", crate::NO_AVDS)
        .env("TMPDIR", root.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let ready: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(30), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ready["ready"], true, "{ready}");
    let evidence = std::path::Path::new(ready["observation"]["path"].as_str().unwrap())
        .parent()
        .unwrap();
    let blocker = evidence.join("summary.json");
    std::fs::create_dir(&blocker).unwrap();
    let socket = directory.join("control.sock");
    let command = exchange(&socket, &json!({"kind":"batch","commandId":"Acme-before-close","observe":false,"steps":[{"tool":"tap","arguments":{"point":{"x":0.5,"y":0.5}}}]})).await;
    assert_eq!(command["ok"], true, "{command}");
    let journal = exchange(
        &socket,
        &json!({"kind":"status","commandId":"Acme-before-close"}),
    )
    .await;
    assert_eq!(journal["ok"], true, "{journal}");
    let failed = exchange(&socket, &json!({"kind":"close"})).await;
    assert_eq!(failed["ok"], false, "{failed}");
    assert_eq!(failed["error"]["code"], "EVIDENCE_IO", "{failed}");
    let status = exchange(&socket, &json!({"kind":"status"})).await;
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(status["data"]["state"], "failed", "{status}");
    assert_eq!(status["data"]["closed"], false, "{status}");
    assert_eq!(status["data"]["cleanupError"], failed["error"], "{status}");
    assert!(child.try_wait().unwrap().is_none());
    let archived = exchange(
        &socket,
        &json!({"kind":"status","commandId":"Acme-before-close"}),
    )
    .await;
    assert_eq!(archived, journal);
    let repeated = exchange(&socket, &json!({"kind":"close"})).await;
    assert_eq!(repeated["error"], failed["error"], "{repeated}");
    let refused = exchange(&socket, &json!({"kind":"observe"})).await;
    assert_eq!(refused["error"]["code"], "SESSION_CLOSING", "{refused}");
    std::fs::remove_dir(&blocker).unwrap();
    let closed = exchange(&socket, &json!({"kind":"close"})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    assert!(
        tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(blocker.is_file());
}

#[tokio::test]
async fn foreground_signal_cleanup_failure_exits_instead_of_waiting_for_close() {
    let serial = format!("Acme-foreground-{}", uuid::Uuid::new_v4().simple());
    let fixture = Mcp::start_named(&serial).await;
    for signal in ["-INT", "-TERM"] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("controller");
        let mut child = Command::new(env!("CARGO_BIN_EXE_testrun"))
            .arg("--json")
            .args([
                "session",
                "--target",
                &format!("android:{serial}"),
                "--directory",
            ])
            .arg(&directory)
            .env_remove("TESTRUN_TASK_ID")
            .env_remove("CODEX_THREAD_ID")
            .env("TESTRUN_ADB", fixture.adb.as_ref().unwrap())
            .env("ANDROID_AVD_HOME", crate::NO_AVDS)
            .env("TMPDIR", root.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
        let ready: Value = serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(30), output.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(ready["ready"], true, "{ready}");
        let evidence = std::path::Path::new(ready["observation"]["path"].as_str().unwrap())
            .parent()
            .unwrap();
        let blocker = evidence.join("summary.json");
        std::fs::create_dir(&blocker).unwrap();
        // Fail an explicit close first; termination must still finish shutdown.
        let refused = Command::new(env!("CARGO_BIN_EXE_testrun"))
            .arg("--json")
            .args(["close", "--endpoint"])
            .arg(directory.join("control.sock"))
            .env_remove("TESTRUN_TASK_ID")
            .env_remove("CODEX_THREAD_ID")
            .kill_on_drop(true)
            .output()
            .await
            .unwrap();
        assert!(!refused.status.success());
        let close: Value = serde_json::from_slice(&refused.stdout).unwrap();
        assert_eq!(close["error"]["code"], "EVIDENCE_IO", "{close}");
        assert!(child.try_wait().unwrap().is_none());
        assert!(
            Command::new("kill")
                .args([signal, &child.id().unwrap().to_string()])
                .status()
                .await
                .unwrap()
                .success()
        );
        let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .expect("Foreground cleanup failure must exit without another close")
            .unwrap();
        let mut error = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut child.stderr.take().unwrap(), &mut error)
            .await
            .unwrap();
        assert!(!status.success(), "{signal}: {status}");
        assert!(error.contains("CLEANUP_FAILED"), "{error}");
        assert!(blocker.is_dir());
        assert!(!directory.join("control.sock").exists());
        std::fs::remove_dir(blocker).unwrap();
    }
}

#[tokio::test]
async fn managed_signal_after_failed_cleanup_exits_and_preserves_recovery() {
    for signal in ["-INT", "-TERM"] {
        let serial = format!("Acme-managed-signal-{}", uuid::Uuid::new_v4().simple());
        let mut mcp = Mcp::start_named(&serial).await;
        assert_eq!(
            mcp.tool("connect", json!({"targetId":format!("android:{serial}")}))
                .await["ok"],
            true
        );
        let pid = mcp.owner_pid().await;
        assert_eq!(
            mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
            true
        );
        let blocker = block_summary(&mut mcp).await;
        let failed = mcp.tool("disconnect", json!({})).await;
        assert_eq!(failed["code"], "CLEANUP_FAILED", "{failed}");
        assert!(
            Command::new("kill")
                .args([signal, &pid])
                .status()
                .await
                .unwrap()
                .success()
        );
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while Command::new("kill")
            .args(["-0", &pid])
            .stderr(Stdio::null())
            .status()
            .await
            .unwrap()
            .success()
        {
            assert!(
                tokio::time::Instant::now() < deadline,
                "Managed failed cleanup ignored {signal}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let status = mcp.target_status().await;
        assert_eq!(status["data"]["state"], "failed", "{status}");
        assert_eq!(mcp.switches()["airplane"], "1");
        std::fs::remove_dir(blocker).unwrap();
        let recovered = mcp
            .tool("connect", json!({"targetId":format!("android:{serial}")}))
            .await;
        assert_eq!(recovered["ok"], true, "{recovered}");
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
}

#[tokio::test]
async fn failed_disconnect_resumes_evidence_publication_without_replaying_input() {
    let mut mcp = connected().await;
    assert_eq!(
        mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
        true
    );
    let acted = mcp
        .tool(
            "act",
            json!({"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}}]}),
        )
        .await;
    assert_eq!(acted["ok"], true, "{acted}");
    let blocker = block_summary(&mut mcp).await;
    let failed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(failed["code"], "CLEANUP_FAILED", "{failed}");
    let status = mcp.target_status().await;
    assert_eq!(status["data"]["state"], "releasing", "{status}");
    assert_eq!(mcp.switches()["airplane"], "1");
    let history = status["data"]["lastAction"].clone();
    std::fs::remove_dir(blocker).unwrap();
    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    mcp.closed_cleanup().await;
    assert_eq!(mcp.archived_action(&history).await, history);
    let aggregate = mcp.tool("status", json!({})).await;
    assert_eq!(aggregate["data"]["targets"], json!([]));
    assert_eq!(mcp.switches()["airplane"], "1");
    assert_eq!(
        mcp.commands(),
        [
            "'cmd' 'connectivity' 'airplane-mode' 'enable'",
            "'input' 'tap' '50' '100'"
        ]
    );
    let log = std::fs::read_to_string(&mcp.log).unwrap();
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("'input' 'tap'"))
            .count(),
        1
    );
}

#[tokio::test]
async fn failed_disconnect_resumes_without_its_socket_and_preserves_replacements() {
    use std::os::unix::fs::MetadataExt;

    for replacement in [false, true] {
        let mut mcp = connected().await;
        assert_eq!(
            mcp.tool("network_set", json!({"mode":"offline"})).await["ok"],
            true
        );
        let blocker = block_summary(&mut mcp).await;
        assert_eq!(
            mcp.tool("disconnect", json!({})).await["code"],
            "CLEANUP_FAILED"
        );
        let selected = mcp._directory.target_records().join("selected.json");
        let connection: Value = serde_json::from_slice(&std::fs::read(selected).unwrap()).unwrap();
        let directory = std::path::Path::new(connection["directory"].as_str().unwrap());
        let socket = directory.join("s/control.sock");
        std::fs::remove_file(&socket).unwrap();
        let foreign = replacement.then(|| std::os::unix::net::UnixListener::bind(&socket).unwrap());
        let identity = foreign
            .as_ref()
            .map(|_| std::fs::metadata(&socket).unwrap().ino());
        // A request from another epoch must neither retry nor remove its marker.
        let stale = json!({"epoch":"Acme-stale-epoch","requestId":"Acme-stale-request"});
        std::fs::write(
            directory.join("stale-stop.json"),
            serde_json::to_vec(&stale).unwrap(),
        )
        .unwrap();
        std::fs::rename(
            directory.join("stale-stop.json"),
            directory.join("stop.json"),
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        // The stale marker started no attempt: every actor attempt is one
        // the manager made, by disconnect or its own retry.
        let status = mcp.target_status().await;
        assert_eq!(status["data"]["state"], "releasing", "{status}");
        assert_eq!(status["data"]["health"]["closed"], false, "{status}");
        let release = &status["data"]["release"];
        let made = release["attempts"].as_u64().unwrap() + u64::from(release["running"] == true);
        assert!(
            status["data"]["cleanup"]["attempts"].as_u64().unwrap() <= made,
            "{status}"
        );
        assert_eq!(
            mcp.tool("disconnect", json!({})).await["code"],
            "CLEANUP_FAILED"
        );
        let failed = mcp.target_status().await;
        assert!(failed["data"]["cleanup"]["attempts"].as_u64().unwrap() >= 2);
        std::fs::remove_dir(blocker).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_testrun"))
            .arg("--json")
            .arg("disconnect")
            .env("TESTRUN_DATA_ROOT", mcp._directory.data_root())
            .env_remove("CODEX_THREAD_ID")
            .env("TESTRUN_TASK_ID", mcp.task.as_ref().unwrap())
            .env("TESTRUN_ADB", mcp.adb.as_ref().unwrap())
            .env("ANDROID_AVD_HOME", crate::NO_AVDS)
            .kill_on_drop(true)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let closed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(closed["closed"], true, "{closed}");
        // The CLI or the manager's own retry finished it, keeping every error.
        let cleanup = mcp.closed_cleanup().await;
        assert!(cleanup["attempts"].as_u64().unwrap() >= 3);
        assert!(cleanup["errors"].as_array().unwrap().len() >= 2);
        assert_eq!(mcp.switches()["airplane"], "1");
        if let Some(identity) = identity {
            assert_eq!(std::fs::metadata(&socket).unwrap().ino(), identity);
            drop(foreign);
            std::fs::remove_file(&socket).unwrap();
        } else {
            assert!(!socket.exists());
        }
    }
}

#[tokio::test]
async fn each_request_reads_current_settings_and_disconnect_preserves_them() {
    let mut mcp = connected().await;
    let airplane = mcp._directory.path().join("airplane");
    std::fs::write(&airplane, "1\n").unwrap();
    let noop = mcp.tool("network_set", json!({"mode":"offline"})).await;
    assert_eq!(noop["ok"], true, "{noop}");
    assert_eq!(noop["data"]["changed"], false);
    assert_eq!(noop["data"]["before"]["airplane"], true);
    assert!(noop["data"].get("baseline").is_none());
    assert!(mcp.commands().is_empty());

    // A later request reads an external change instead of reusing old state.
    std::fs::write(&airplane, "0\n").unwrap();
    let offline = mcp.tool("network_set", json!({"mode":"offline"})).await;
    assert_eq!(offline["ok"], true, "{offline}");
    assert_eq!(offline["data"]["changed"], true);
    assert_eq!(offline["data"]["before"]["airplane"], false);
    assert_eq!(offline["data"]["before"]["connectivity"], "online");
    assert_eq!(mcp.switches()["airplane"], "1");
    let status = mcp.target_status().await;
    assert_eq!(
        status["data"]["lastNetworkOperation"]["result"]["changed"],
        true
    );

    let closed = mcp.tool("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    assert_eq!(
        mcp.switches(),
        json!({"airplane":"1","wifi":"1","data":"1","bt":"1"})
    );
    assert_eq!(
        mcp.commands(),
        ["'cmd' 'connectivity' 'airplane-mode' 'enable'"]
    );
}

#[tokio::test]
async fn initial_readback_failure_sends_no_switches_and_does_not_block_disconnect() {
    let mut mcp = connected().await;
    rewrite_adb(
        &mcp,
        "readback() {\n",
        "readback() {\n  if [ -f fail-initial-readback ]; then exit 7; fi\n",
    );
    mcp.arm("fail-initial-readback");
    let failed = mcp.tool("network_set", json!({"mode":"offline"})).await;
    assert_eq!(failed["code"], "NETWORK_READBACK_FAILED", "{failed}");
    let status = mcp.target_status().await;
    assert_eq!(status["data"]["lastNetworkOperation"]["state"], "failed");
    assert_eq!(
        status["data"]["lastNetworkOperation"]["error"]["dispatched"],
        false
    );
    assert!(status["data"]["network"].is_null(), "{status}");
    assert!(mcp.commands().is_empty());
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert_eq!(mcp.switches()["airplane"], "0");
}

#[tokio::test]
async fn online_without_switch_commands_still_requires_connectivity_readback() {
    let mut mcp = connected().await;
    for name in ["wifi", "data"] {
        std::fs::write(mcp._directory.path().join(name), "0\n").unwrap();
    }
    let failed = mcp
        .tool("network_set", json!({"mode":"online","timeoutMs":1000}))
        .await;
    assert_eq!(failed["code"], "NETWORK_UNSETTLED", "{failed}");
    let status = mcp.target_status().await;
    let error = &status["data"]["lastNetworkOperation"]["error"];
    assert_eq!(error["dispatched"], false, "{status}");
    assert_eq!(error["result"]["changed"], false);
    assert_eq!(error["result"]["agreed"], false);
    assert_eq!(error["result"]["after"]["connectivity"], "offline");
    assert!(status["data"].get("network").is_none());
    assert!(mcp.commands().is_empty());
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
    assert_eq!(mcp.switches()["wifi"], "0");
    assert_eq!(mcp.switches()["data"], "0");
}

#[tokio::test]
async fn owner_loss_during_radio_dispatch_preserves_partial_settings_until_explicit_recovery() {
    let mut mcp = connected().await;
    let target = mcp.target_status().await["data"]["targetId"].clone();
    rewrite_adb(
        &mcp,
        "      echo 0 > data ;;",
        "      if [ -f pause-data-disable ]; then touch data-disable-started; sleep 5; touch data-disable-finished; exit 7; fi\n      echo 0 > data ;;",
    );
    mcp.arm("pause-data-disable");
    mcp.next += 1;
    mcp.send(json!({"jsonrpc":"2.0","id":mcp.next,"method":"tools/call","params":{"name":"network_set","arguments":{"mode":"offline","method":"radios"}}})).await;
    wait_for(&mcp, "data-disable-started").await;
    assert_eq!(mcp.switches()["wifi"], "0");
    mcp.kill_owner().await;
    // The fake command has a finite lifetime and never changes data. Finish its
    // remaining work before dropping the owned fixture directory.
    wait_for(&mcp, "data-disable-finished").await;
    let status = mcp.target_status().await;
    assert_eq!(
        status["data"]["lastNetworkOperation"]["state"], "outcome_unknown",
        "{status}"
    );
    let recovered = mcp.tool("connect", json!({"targetId":target})).await;
    assert_eq!(recovered["ok"], true, "{recovered}");
    assert!(recovered["data"].get("networkRestore").is_none());
    assert_eq!(mcp.switches()["wifi"], "0");
    assert!(!mcp.commands().contains(&"'svc' 'wifi' 'enable'".into()));
    let online = mcp
        .tool("network_set", json!({"mode":"online","method":"radios"}))
        .await;
    assert_eq!(online["ok"], true, "{online}");
    assert_eq!(online["data"]["changed"], false);
    assert_eq!(
        mcp.switches(),
        json!({"airplane":"0","wifi":"0","data":"1","bt":"1"})
    );
    assert_eq!(
        mcp.tool("disconnect", json!({})).await["data"]["closed"],
        true
    );
}

async fn wait_for(mcp: &Mcp, name: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !mcp._directory.path().join(name).exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "Fixture did not publish {name}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn frame(stream: &mut tokio::net::UnixStream, input: &Value) {
    let bytes = serde_json::to_vec(input).unwrap();
    stream.write_u32(bytes.len() as u32).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
}
async fn reply(stream: &mut tokio::net::UnixStream) -> Value {
    use tokio::io::AsyncReadExt;
    tokio::time::timeout(Duration::from_secs(10), async {
        let length = stream.read_u32().await.unwrap();
        assert!(length <= 1024 * 1024);
        let mut bytes = vec![0; length as usize];
        stream.read_exact(&mut bytes).await.unwrap();
        serde_json::from_slice::<Value>(&bytes).unwrap()
    })
    .await
    .expect("controller reply")
}
async fn exchange(socket: &std::path::Path, input: &Value) -> Value {
    let mut stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    frame(&mut stream, input).await;
    reply(&mut stream).await
}

#[tokio::test]
async fn abandoned_queued_close_leaves_the_foreground_session_usable() {
    let serial = format!("Acme-abandoned-close-{}", uuid::Uuid::new_v4().simple());
    let fixture = Mcp::start_named(&serial).await;
    rewrite_adb(
        &fixture,
        "    *acme-unhandled*)",
        "    *\"'input' 'tap'\"*) touch input-held; count=0; while [ -f hold-input ] && [ $count -lt 300 ]; do sleep 0.02; count=$((count+1)); done ;;\n    *acme-unhandled*)",
    );
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("controller");
    let mut child = Command::new(env!("CARGO_BIN_EXE_testrun"))
        .arg("--json")
        .args([
            "session",
            "--target",
            &format!("android:{serial}"),
            "--directory",
        ])
        .arg(&directory)
        .env_remove("TESTRUN_TASK_ID")
        .env_remove("CODEX_THREAD_ID")
        .env("TESTRUN_ADB", fixture.adb.as_ref().unwrap())
        .env("ANDROID_AVD_HOME", crate::NO_AVDS)
        .env("TMPDIR", root.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let ready: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(30), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ready["ready"], true, "{ready}");
    let socket = directory.join("control.sock");
    fixture.arm("hold-input");
    let mut input = tokio::net::UnixStream::connect(&socket).await.unwrap();
    frame(
        &mut input,
        &json!({"kind":"batch","commandId":"held","observe":false,"steps":[{"tool":"tap","arguments":{"point":{"x":0.5,"y":0.5}}}]}),
    )
    .await;
    let held = fixture._directory.path().join("input-held");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !held.exists() {
        if tokio::time::Instant::now() >= deadline {
            panic!("input was not held: {}", reply(&mut input).await);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Queue a close behind the held input, then abandon it before dispatch.
    let mut close = tokio::net::UnixStream::connect(&socket).await.unwrap();
    frame(&mut close, &json!({"kind":"close"})).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let queued = exchange(&socket, &json!({"kind":"status"})).await;
    assert_eq!(queued["data"]["state"], "closing", "{queued}");
    drop(close);
    std::fs::remove_file(fixture._directory.path().join("hold-input")).unwrap();
    let completed = reply(&mut input).await;
    assert_eq!(completed["ok"], true, "{completed}");
    let status = exchange(&socket, &json!({"kind":"status"})).await;
    assert_eq!(status["data"]["state"], "ready", "{status}");
    let closed = exchange(&socket, &json!({"kind":"close"})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    assert_ne!(closed["data"]["state"], "closing", "{closed}");
    let exit = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("controller exits after close")
        .unwrap();
    assert!(exit.success(), "{exit}");
}
