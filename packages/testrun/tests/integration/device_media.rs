//! Live physical video acceptance through MCP and authenticated media delivery.
use super::simulator::Client;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod recovery;

async fn managed_cli(
    binary: &std::path::Path,
    context: &str,
    args: &[&str],
) -> Result<(Value, f64), String> {
    let started = std::time::Instant::now();
    let mut command = tokio::process::Command::new(binary);
    command.arg("--json");
    command
        .args(args)
        .env(
            "TESTRUN_DATA_ROOT",
            crate::acceptance_workspace().data_root(),
        )
        .env("TESTRUN_TASK_ID", context)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(130), command.output())
        .await
        .map_err(|_| "CLI deadline exceeded".to_string())?
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok((
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?,
        started.elapsed().as_secs_f64() * 1000.0,
    ))
}

struct ManagedCleanup {
    binary: std::path::PathBuf,
    context: String,
    blocker: Option<std::path::PathBuf>,
    armed: bool,
}

/// Observe real remote allocation, cancel the still-pending shipping MCP open,
/// and require release while that MCP server remains alive. No phone input.
#[tokio::test]
async fn startup_cancel_after_remote_allocation_releases_before_reacquisition() {
    let serial = std::env::var("TESTRUN_TEST_DEVICE").expect("Select the authorized USB fixture");
    let binary = std::env::var_os("TESTRUN_TEST_BINARY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
    let root = tempfile::tempdir().unwrap();
    let mut monitor = testrun::device_media::ConnectionOwner::connect(&serial)
        .await
        .unwrap();
    let baseline = monitor.status().await.unwrap();
    assert_eq!(
        baseline["running"], false,
        "Preserve existing capture: {baseline}"
    );
    assert!(baseline["sessions"].as_array().unwrap().is_empty());
    let mut client = Client::start_shared(root.path(), Duration::from_secs(100), None).await;
    let targets = client
        .call("target_list", json!({"kind":"device","limit":200}))
        .await;
    let target = targets["data"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["targetId"] == format!("device:{serial}"))
        .unwrap();
    let id = client
        .begin_request(
            "tools/call",
            json!({"name":"connect","arguments":{
                "targetId":target["targetId"],"controlPath":"hid"
            }}),
        )
        .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let allocation = loop {
        let status = monitor.status().await.unwrap();
        if !status["sessions"].as_array().unwrap().is_empty() {
            break status;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "No remote allocation: {status}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let mut bytes = serde_json::to_vec(
        &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}),
    )
    .unwrap();
    bytes.push(b'\n');
    client.input.write_all(&bytes).await.unwrap();
    let cancelled = client.request("ping", json!({})).await;
    // EOF always attempts retained owners before a phase assertion can fail.
    if cancelled["result"] != json!({}) {
        drop(client.input);
        client.child.wait().await.unwrap();
        panic!("Cancellation missed pending startup after allocation: {cancelled}");
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    loop {
        let status = monitor.status().await.unwrap();
        if status["running"] == false && status["sessions"].as_array().unwrap().is_empty() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Owned startup remains allocated: {status}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let closed = client.call("disconnect", json!({})).await;
    assert_eq!(closed["ok"], true, "{closed}");
    verify_native_task_reacquisition(&binary, &format!("device:{serial}")).await;
    drop(client.input);
    assert!(client.child.wait().await.unwrap().success());
    eprintln!(
        "Cancelled pending native startup after observed allocation; verified release and fresh CLI acquisition: {allocation}"
    );
}
impl Drop for ManagedCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Some(blocker) = &self.blocker {
            let _ = std::fs::remove_dir(blocker);
        }
        let result = std::process::Command::new(&self.binary)
            .arg("--json")
            .arg("disconnect")
            .env(
                "TESTRUN_DATA_ROOT",
                crate::acceptance_workspace().data_root(),
            )
            .env_remove("CODEX_THREAD_ID")
            .env("TESTRUN_TASK_ID", &self.context)
            .stdout(std::process::Stdio::null())
            .status();
        if !result.is_ok_and(|status| status.success()) {
            eprintln!(
                "Owned physical acceptance cleanup is unfinished; task {}",
                self.context
            );
        }
    }
}

#[tokio::test]
async fn managed_readonly_repeated_connect_observe_disconnect() {
    let serial = std::env::var("TESTRUN_TEST_DEVICE").expect("Select the authorized USB fixture");
    let target = format!("device:{serial}");
    let binary = std::env::var_os("TESTRUN_TEST_BINARY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
    let context = format!("testrun-readonly-acceptance-{}", uuid::Uuid::new_v4());
    for cycle in 0..10 {
        let connected = managed_cli(&binary, &context, &["connect", "--target", &target]).await;
        let mut frames = Vec::new();
        if connected.is_ok() {
            for _ in 0..10 {
                let frame = managed_cli(&binary, &context, &["observe"]).await;
                let failed = frame.is_err();
                frames.push(frame);
                if failed {
                    break;
                }
            }
        }
        // Always release this task before asserting; never repair another owner.
        let disconnected = managed_cli(&binary, &context, &["disconnect"]).await;
        let (connection, connect_ms) = connected.expect("Managed startup failed");
        assert_eq!(connection["state"], "ready");
        assert_eq!(connection["lastAction"], Value::Null);
        let (closed, close_ms) = disconnected.expect("Managed disconnect failed");
        assert_eq!(closed["closed"], true);
        let mut previous = connection["observation"]["captureTimingsMs"]["framePtsNs"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let mut observation_ms = Vec::new();
        let mut backend_ms = Vec::new();
        for frame in frames {
            let (frame, ms) = frame.expect("Fresh observation failed");
            assert_eq!(frame["state"], "ready");
            assert_eq!(frame["lastAction"], Value::Null);
            assert_eq!(frame["observation"]["source"], "coredevice_video");
            let pts = frame["observation"]["captureTimingsMs"]["framePtsNs"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap();
            assert!(pts > previous, "Observation reused an old decoded frame");
            previous = pts;
            observation_ms.push(ms);
            backend_ms.push(
                frame["observation"]["captureTimingsMs"]["observationMs"]
                    .as_f64()
                    .unwrap(),
            );
        }
        eprintln!(
            "{}",
            json!({"cycle":cycle+1,"connectMs":connect_ms,"disconnectMs":close_ms,"observationMs":observation_ms,"backendMs":backend_ms})
        );
    }
}

async fn refuse_malformed_close(socket: &std::path::Path, epoch: &Value) {
    let reply = tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = tokio::net::UnixStream::connect(socket).await.unwrap();
        let bytes =
            serde_json::to_vec(&json!({"kind":"close","ownerEpoch":epoch,"unexpected":true}))
                .unwrap();
        stream.write_u32(bytes.len() as u32).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
        let length = stream.read_u32().await.unwrap();
        assert!(length <= 1024 * 1024);
        let mut bytes = vec![0; length as usize];
        stream.read_exact(&mut bytes).await.unwrap();
        serde_json::from_slice::<Value>(&bytes).unwrap()
    })
    .await
    .unwrap();
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(reply["error"]["code"], "INVALID_ARGUMENT", "{reply}");
}

/// Native build handover, a task end that releases nothing and an explicit
/// release; no phone input or recording.
#[tokio::test]
async fn managed_build_handover_and_host_task_end_release_native_ownership() {
    let serial = std::env::var("TESTRUN_TEST_DEVICE").expect("Select the authorized USB fixture");
    let target = format!("device:{serial}");
    let root = tempfile::tempdir().unwrap();
    let context = format!("testrun-host-end-{}", uuid::Uuid::new_v4());
    let binary = std::env::var_os("TESTRUN_TEST_BINARY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
    let previous = std::env::var_os("TESTRUN_TEST_PREVIOUS_BINARY").map(std::path::PathBuf::from);
    let mut cleanup = ManagedCleanup {
        binary: binary.clone(),
        context: context.clone(),
        blocker: None,
        armed: true,
    };
    let connected = managed_cli(
        previous.as_deref().unwrap_or(&binary),
        &context,
        &["connect", "--target", &target],
    )
    .await
    .unwrap()
    .0;
    assert_eq!(connected["state"], "ready");
    let selection = crate::acceptance_workspace()
        .target_records()
        .join("selected.json");
    let mut old: Value = serde_json::from_slice(&std::fs::read(&selection).unwrap()).unwrap();
    if previous.is_none() {
        old["buildId"] = json!("Acme-previous-build");
        std::fs::write(&selection, serde_json::to_vec(&old).unwrap()).unwrap();
    }
    let mut client =
        Client::start_shared(root.path(), Duration::from_secs(100), Some(&context)).await;
    let handed_over = client.call("connect", json!({"targetId":target})).await;
    assert_eq!(handed_over["ok"], true, "{handed_over}");
    assert_eq!(handed_over["data"]["reused"], false, "{handed_over}");
    let replacement: Value = serde_json::from_slice(&std::fs::read(&selection).unwrap()).unwrap();
    assert_ne!(replacement["epoch"], old["epoch"]);
    let old_cleanup: Value = serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(old["directory"].as_str().unwrap()).join("cleanup.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(old_cleanup["complete"], true, "{old_cleanup}");
    let observed = client.call("observe", json!({})).await;
    assert_eq!(observed["ok"], true, "{observed}");
    // Task end releases nothing: the phone stays connected until a disconnect.
    let ended = client.request("testrun/task_end", json!({})).await;
    assert_eq!(ended["result"]["released"], json!([]), "{ended}");
    let still = managed_cli(&binary, &context, &["status", "--target", &target])
        .await
        .unwrap()
        .0;
    assert_eq!(still["state"], "ready", "{still}");
    let closed = managed_cli(&binary, &context, &["disconnect", "--target", &target])
        .await
        .unwrap()
        .0;
    assert_eq!(closed["closed"], true, "{closed}");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    loop {
        let status = managed_cli(&binary, &context, &["status"]).await.unwrap().0;
        if status["state"] == "closed" {
            assert!(status["lastAction"].is_null());
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{status}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let output = tokio::process::Command::new(&binary)
        .arg("--json")
        .arg("task", "stop")
        .env(
            "TESTRUN_DATA_ROOT",
            crate::acceptance_workspace().data_root(),
        )
        .env("TESTRUN_TASK_ID", &context)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    cleanup.armed = false;
    verify_native_task_reacquisition(&binary, &target).await;
    eprintln!(
        "Native build handover and task stop passed; separatePreviousExecutable={}",
        previous.is_some()
    );
}

async fn verify_native_task_reacquisition(binary: &std::path::Path, target: &str) {
    // A different task can acquire the same phone only after verified cleanup.
    let next_context = format!("testrun-host-next-{}", uuid::Uuid::new_v4());
    let mut next_cleanup = ManagedCleanup {
        binary: binary.into(),
        context: next_context.clone(),
        blocker: None,
        armed: true,
    };
    assert_eq!(
        managed_cli(binary, &next_context, &["connect", "--target", target])
            .await
            .unwrap()
            .0["state"],
        "ready"
    );
    assert_eq!(
        managed_cli(binary, &next_context, &["disconnect"])
            .await
            .unwrap()
            .0["closed"],
        true
    );
    next_cleanup.armed = false;
}

/// Refuse malformed close, then resume a real finalization failure after native
/// release through MCP/CLI, without unplugging or sending phone input.
#[tokio::test]
async fn managed_disconnect_resumes_after_native_release_and_finalization_failure() {
    let serial = std::env::var("TESTRUN_TEST_DEVICE").expect("Select the authorized USB fixture");
    let root = tempfile::tempdir().unwrap();
    let context = format!("testrun-cleanup-acceptance-{}", uuid::Uuid::new_v4());
    let binary = std::env::var_os("TESTRUN_TEST_BINARY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
    let mut client =
        Client::start_shared(root.path(), Duration::from_secs(100), Some(&context)).await;
    let mut cleanup = ManagedCleanup {
        binary: binary.clone(),
        context: context.clone(),
        blocker: None,
        armed: true,
    };
    let connected = client
        .call("connect", json!({"targetId":format!("device:{serial}")}))
        .await;
    assert_eq!(connected["ok"], true, "{connected}");
    let selected = crate::acceptance_workspace()
        .target_records()
        .join("selected.json");
    let connection: Value = serde_json::from_slice(&std::fs::read(selected).unwrap()).unwrap();
    let socket =
        std::path::Path::new(connection["directory"].as_str().unwrap()).join("s/control.sock");
    refuse_malformed_close(&socket, &connection["epoch"]).await;
    let ready = client.call("status", json!({})).await;
    assert_eq!(ready["data"]["state"], "ready", "{ready}");
    let runs: Vec<_> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("testrun-qa-")
        })
        .collect();
    assert_eq!(runs.len(), 1);
    let blocker = runs[0].join("summary.json");
    std::fs::create_dir(&blocker).unwrap();
    cleanup.blocker = Some(blocker.clone());
    let failed = client.call("disconnect", json!({})).await;
    // Remove only this fixture's obstruction before assertions so a failure
    // cannot strand an unrepairable filesystem condition in the owner.
    std::fs::remove_dir(&blocker).unwrap();
    std::fs::remove_file(socket).unwrap();
    let status = client.call("status", json!({})).await;
    let recovery = managed_cli(&binary, &context, &["disconnect"]).await;
    assert_eq!(failed["code"], "CLEANUP_FAILED", "{failed}");
    assert_eq!(status["data"]["state"], "failed", "{status}");
    assert_eq!(status["data"]["nextAction"], "disconnect");
    let (closed, _) = recovery.expect("Retry must finish owned finalization");
    assert_eq!(closed["closed"], true, "{closed}");
    assert_eq!(closed["cleanup"]["complete"], true);
    assert_eq!(closed["cleanup"]["attempts"], 2);
    assert!(
        closed["cleanup"]["errors"]
            .to_string()
            .contains("EVIDENCE_IO")
    );
    assert_eq!(closed["cleanup"]["sessions"][0]["progress"]["attempts"], 1);
    assert_eq!(closed["lastAction"], Value::Null);
    let status = client.call("status", json!({})).await;
    assert_eq!(status["data"]["state"], "closed", "{status}");
    assert_eq!(status["data"]["cleanup"], closed["cleanup"]);
    assert!(blocker.is_file());
    cleanup.armed = false;
    eprintln!("Resumed native finalization: {}", closed["cleanup"]);
    drop(client.input);
    client.child.wait().await.unwrap();
}

/// The grant broker's own protocol names the preview `streamId`.
async fn grant(broker: &str, stream: &Value) -> Value {
    let mut socket = tokio::net::UnixStream::connect(broker).await.unwrap();
    let request = json!({"streamId":stream}).to_string().into_bytes();
    socket
        .write_all(&(request.len() as u32).to_be_bytes())
        .await
        .unwrap();
    socket.write_all(&request).await.unwrap();
    let mut prefix = [0; 4];
    socket.read_exact(&mut prefix).await.unwrap();
    let mut bytes = vec![0; u32::from_be_bytes(prefix) as usize];
    socket.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One live MCP transaction verifies capture, viewer isolation, recording and cleanup"
)]
async fn native_hevc_preview_recording_and_independent_stop() {
    let serial = std::env::var("TESTRUN_TEST_DEVICE").expect("Select the authorized USB fixture");
    let target_id = format!("device:{serial}");
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::start_with_timeout(directory.path(), Duration::from_secs(45)).await;
    let targets = client
        .call("target_list", json!({"kind":"device","limit":200}))
        .await;
    assert_eq!(targets["ok"], true, "{targets}");
    assert!(
        targets["data"]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["targetId"] == target_id),
        "{targets}"
    );
    let session = client.call("connect", json!({"targetId":target_id})).await;
    assert_eq!(session["ok"], true, "{session}");
    let preview = client.call("preview_start", json!({})).await;
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["data"]["profile"]["codec"], "hevc");
    let broker = preview["data"]["grantBrokerPath"].as_str().unwrap();
    let token = grant(broker, &preview["data"]["previewId"]).await;
    assert!(
        !preview
            .to_string()
            .contains(token["token"].as_str().unwrap())
    );
    let url = preview["data"]["mediaUrl"]
        .as_str()
        .unwrap()
        .replacen("http:", "ws:", 1);
    let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            json!({"token":token["token"]}).to_string().into(),
        ))
        .await
        .unwrap();
    let mut received = 0;
    let mut key = false;
    let mut configuration = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while received < 60 || !key {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let tokio_tungstenite::tungstenite::Message::Binary(bytes) = message {
            let header = bytes[..24].try_into().unwrap();
            let (meta, payload) = testrun::media::lengths(&header).unwrap();
            assert_eq!(bytes.len(), 24 + meta + payload);
            let metadata: Value = serde_json::from_slice(&bytes[24..24 + meta]).unwrap();
            assert_eq!(metadata["codec"], "hevc");
            if bytes[6] == 2 {
                configuration = true;
                assert_eq!(bytes[24 + meta], 1);
            } else {
                assert!(configuration);
                if received == 0 {
                    assert_eq!(bytes[7] & 1, 1);
                }
                received += 1;
                key |= bytes[7] & 1 != 0;
            }
        }
    }
    let recording = client
        .call("recording_start", json!({"maxDurationMs":5000}))
        .await;
    assert_eq!(recording["ok"], true, "{recording}");
    assert_eq!(
        client
            .call(
                "preview_stop",
                json!({"previewId":preview["data"]["previewId"]})
            )
            .await["ok"],
        true
    );
    tokio::time::sleep(Duration::from_secs(4)).await;
    // No status/stop call triggers cleanup: duration expiry must release capture itself.
    tokio::time::sleep(Duration::from_secs(23)).await;
    assert_eq!(
        testrun::device_media::inspect(&serial).await.unwrap()["status"]["running"],
        false
    );
    let completed = client
        .call(
            "recording_stop",
            json!({"recordingId":recording["data"]["recordingId"]}),
        )
        .await;
    assert_eq!(completed["ok"], true, "{completed}");
    assert_eq!(completed["data"]["status"], "complete", "{completed}");
    assert!(completed["data"]["segments"].as_array().unwrap().len() >= 2);
    assert_eq!(
        completed["data"]["statistics"]["frames"],
        completed["data"]["statistics"]["decodedFrames"]
    );
    for segment in completed["data"]["segments"].as_array().unwrap() {
        let start: u64 = segment["startPtsNs"].as_str().unwrap().parse().unwrap();
        let end: u64 = segment["endPtsNs"].as_str().unwrap().parse().unwrap();
        assert!(end - start <= 5_000_000_000, "{segment}");
        assert!(
            segment["artifact"]["verification"]["decodedFrames"]
                .as_u64()
                .unwrap()
                > 0
        );
    }
    let artifact = completed["data"]["artifact"]["path"].as_str().unwrap();
    assert!(std::path::Path::new(artifact).is_file());
    println!(
        "{}",
        json!({"viewerFrames":received,"recordingFrames":completed["data"]["statistics"]["frames"],"recordingStatus":completed["data"]["status"]})
    );
    assert_eq!(
        testrun::device_media::inspect(&serial).await.unwrap()["status"]["running"],
        false
    );
    assert_eq!(client.call("disconnect", json!({})).await["ok"], true);
    socket.close(None).await.ok();
    drop(client.input);
    assert!(
        tokio::time::timeout(Duration::from_secs(10), client.child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(!std::path::Path::new(broker).exists());
}
