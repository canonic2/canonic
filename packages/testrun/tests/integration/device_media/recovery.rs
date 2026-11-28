//! Physical recovery on the authorized phone through shipping MCP: owner death
//! after one input, disconnect/reconnect races, lock/unlock and USB loss.
//! Steps that need a person wait for marker files in `TESTRUN_TEST_STEP_DIR`.
use super::*;
use std::path::PathBuf;

fn selected() -> (String, PathBuf) {
    let serial = std::env::var("TESTRUN_TEST_DEVICE").expect("Select the authorized USB fixture");
    let binary = std::env::var_os("TESTRUN_TEST_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
    (serial, binary)
}

/// The private records of the one connected target.
fn task_directory(_context: &str) -> PathBuf {
    crate::acceptance_workspace().target_records()
}

fn selected_epoch(context: &str) -> Value {
    let record: Value = serde_json::from_slice(
        &std::fs::read(task_directory(context).join("selected.json")).unwrap(),
    )
    .unwrap();
    record["epoch"].clone()
}

/// The acceptance run's owner process, from its own record.
fn owners(_server: u32) -> Vec<i32> {
    crate::acceptance_workspace()
        .owner_pid()
        .into_iter()
        .collect()
}

async fn task_client(context: &str) -> Client {
    let root = tempfile::tempdir().unwrap().keep();
    Client::start_shared(&root, Duration::from_secs(130), Some(context)).await
}

/// Send a tools/call with a caller-chosen JSON-RPC id, as a retrying host would.
async fn send_with_id(client: &mut Client, id: u64, name: &str, arguments: &Value) {
    let mut bytes = serde_json::to_vec(
        &json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
    )
    .unwrap();
    bytes.push(b'\n');
    client.input.write_all(&bytes).await.unwrap();
}

/// Wait until the phone reports no capture and no session.
async fn released(serial: &str, budget: Duration) {
    let mut monitor = testrun::device_media::ConnectionOwner::connect(serial)
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        let status = monitor.status().await.unwrap();
        if status["running"] == false && status["sessions"].as_array().unwrap().is_empty() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Owned capture remains allocated: {status}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// A person performs `step` on the phone; the operator creates the marker.
async fn person(step: &str) {
    let directory = PathBuf::from(
        std::env::var_os("TESTRUN_TEST_STEP_DIR").expect("Set TESTRUN_TEST_STEP_DIR"),
    );
    let marker = directory.join(step);
    eprintln!("WAITING-FOR-PERSON {step}: create {}", marker.display());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(900);
    while !marker.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "No person step {step}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    eprintln!("PERSON-DONE {step}");
}

fn frame_pts(observation: &Value) -> u64 {
    observation["captureTimingsMs"]["framePtsNs"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

/// One status-bar tap whose owner is killed as soon as its receipt exists.
/// The outcome is completed or unknown; neither repeat nor reconnect replays it.
#[tokio::test]
async fn owner_death_after_one_input_is_never_replayed() {
    let (serial, binary) = selected();
    let target = format!("device:{serial}");
    let context = format!("testrun-input-death-{}", uuid::Uuid::new_v4());
    let mut cleanup = ManagedCleanup {
        binary: binary.clone(),
        context: context.clone(),
        blocker: None,
        armed: true,
    };
    let mut client = task_client(&context).await;
    let connected = client.call("connect", json!({"targetId":target})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    let original = selected_epoch(&context);
    let server = client.child.id().unwrap();
    let owner = owners(server);
    assert_eq!(owner.len(), 1, "{owner:?}");
    // Top center is the camera housing: a tap there has no effect.
    let act = json!({"actions":[{"action":"tap","point":{"x":0.5,"y":0.02}}]});
    let receipt = task_directory(&context).join("last-action.json");
    send_with_id(&mut client, 9001, "act", &act).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !receipt.exists() {
        assert!(tokio::time::Instant::now() < deadline, "No action receipt");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    // SAFETY: the process is this task's own managed owner.
    unsafe {
        libc::kill(owner[0], libc::SIGKILL);
    }
    let interrupted = client.response(9001).await["result"]["structuredContent"].clone();
    let status = client.call("status", json!({})).await;
    let history = status["data"]["lastAction"].clone();
    let outcome = history["status"].as_str().unwrap_or_default().to_owned();
    assert!(
        matches!(outcome.as_str(), "outcome_unknown" | "completed"),
        "{status}"
    );
    // The same request is answered from the journal, never dispatched again.
    send_with_id(&mut client, 9001, "act", &act).await;
    let repeated = client.response(9001).await["result"]["structuredContent"].clone();
    if outcome == "outcome_unknown" {
        assert_eq!(repeated["code"], "BATCH_OUTCOME_UNKNOWN", "{repeated}");
    }
    let mut conflicting = act.clone();
    conflicting["actions"][0]["point"]["x"] = json!(0.55);
    send_with_id(&mut client, 9001, "act", &conflicting).await;
    let conflict = client.response(9001).await["result"]["structuredContent"].clone();
    assert_eq!(conflict["code"], "COMMAND_ID_CONFLICT", "{conflict}");
    let observed = client.call("observe", json!({})).await;
    assert_eq!(observed["ok"], true, "{observed}");
    assert_eq!(observed["data"]["lastAction"], history, "{observed}");
    assert_ne!(selected_epoch(&context), original, "no fresh owner");
    let closed = client.call("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    released(&serial, Duration::from_secs(60)).await;
    cleanup.armed = false;
    verify_native_task_reacquisition(&binary, &target).await;
    drop(client.input);
    client.child.wait().await.unwrap();
    eprintln!(
        "{}",
        json!({"outcome":outcome,"interrupted":interrupted["code"],"repeated":repeated["code"].as_str().unwrap_or("result")})
    );
}

/// Disconnect from one server races a reconnect from a successor server for
/// the same task. Both settle; a stale repeated close preserves the replacement.
#[tokio::test]
async fn disconnect_and_reconnect_race_preserves_the_replacement() {
    let (serial, binary) = selected();
    let target = format!("device:{serial}");
    let context = format!("testrun-race-{}", uuid::Uuid::new_v4());
    let mut cleanup = ManagedCleanup {
        binary: binary.clone(),
        context: context.clone(),
        blocker: None,
        armed: true,
    };
    let mut first = task_client(&context).await;
    let mut successor = task_client(&context).await;
    let connected = first.call("connect", json!({"targetId":target})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    let original = selected_epoch(&context);
    send_with_id(&mut first, 9101, "disconnect", &json!({})).await;
    send_with_id(&mut successor, 9102, "connect", &json!({"targetId":target})).await;
    let (closed, recovered) = tokio::join!(first.response(9101), successor.response(9102));
    let closed = closed["result"]["structuredContent"].clone();
    let recovered = recovered["result"]["structuredContent"].clone();
    assert_eq!(closed["ok"], true, "{closed}");
    assert_eq!(recovered["ok"], true, "{recovered}");
    assert_eq!(recovered["data"]["state"], "ready", "{recovered}");
    let replacement = selected_epoch(&context);
    assert_ne!(
        replacement, original,
        "the successor reused the closed owner"
    );
    // Repeating the first server's close request cannot close the replacement.
    send_with_id(&mut first, 9101, "disconnect", &json!({})).await;
    let stale = first.response(9101).await["result"]["structuredContent"].clone();
    assert_eq!(stale["ok"], true, "{stale}");
    assert_ne!(stale["data"]["closed"], true, "{stale}");
    let status = successor.call("status", json!({})).await;
    assert_eq!(status["data"]["state"], "ready", "{status}");
    assert_eq!(selected_epoch(&context), replacement);
    let closed = successor.call("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    released(&serial, Duration::from_secs(60)).await;
    cleanup.armed = false;
    for client in [first, successor] {
        drop(client.input);
        let mut child = client.child;
        child.wait().await.unwrap();
    }
    eprintln!(
        "{}",
        json!({"closed":closed["data"]["closed"],"staleClose":stale["data"],"replaced":true})
    );
}

/// Capture stays fresh while a person locks and unlocks the phone; no input.
#[tokio::test]
async fn lock_and_unlock_keep_observations_fresh() {
    let (serial, binary) = selected();
    let target = format!("device:{serial}");
    let context = format!("testrun-lock-{}", uuid::Uuid::new_v4());
    let mut cleanup = ManagedCleanup {
        binary: binary.clone(),
        context: context.clone(),
        blocker: None,
        armed: true,
    };
    let mut client = task_client(&context).await;
    let connected = client.call("connect", json!({"targetId":target})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    let mut previous = frame_pts(&connected["data"]["observation"]);
    let mut results = Vec::new();
    for step in ["unlocked", "locked", "unlocked-again"] {
        person(step).await;
        let observed = client.call("observe", json!({})).await;
        assert_eq!(observed["ok"], true, "{step}: {observed}");
        let pts = frame_pts(&observed["data"]["observation"]);
        assert!(pts > previous, "{step}: stale frame");
        previous = pts;
        let status = client.call("status", json!({})).await;
        assert_eq!(status["data"]["state"], "ready", "{step}: {status}");
        let expected = if step == "locked" {
            "locked"
        } else {
            "unlocked"
        };
        assert_eq!(
            status["data"]["health"]["lock"]["state"], expected,
            "{step}: {status}"
        );
        assert_eq!(status["data"]["lastAction"], Value::Null, "{step}: input");
        results.push(json!({"step":step,"inputHealth":status["data"]["health"]["inputHealth"],"lock":status["data"]["health"]["lock"]}));
    }
    let closed = client.call("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    released(&serial, Duration::from_secs(60)).await;
    cleanup.armed = false;
    drop(client.input);
    client.child.wait().await.unwrap();
    eprintln!("{}", json!(results));
}

/// A person unplugs the cable mid-session: observation must fail rather than
/// return a stale frame, and reconnect after replug must recover cleanly.
#[tokio::test]
async fn unplug_and_replug_reports_loss_and_recovers() {
    let (serial, binary) = selected();
    let target = format!("device:{serial}");
    let context = format!("testrun-unplug-{}", uuid::Uuid::new_v4());
    let mut cleanup = ManagedCleanup {
        binary: binary.clone(),
        context: context.clone(),
        blocker: None,
        armed: true,
    };
    let mut client = task_client(&context).await;
    let connected = client.call("connect", json!({"targetId":target})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    let before = frame_pts(&connected["data"]["observation"]);
    person("unplugged").await;
    let lost = client.call("observe", json!({})).await;
    assert_eq!(
        lost["ok"], false,
        "a frame was returned without USB: {lost}"
    );
    let status = client.call("status", json!({})).await;
    eprintln!(
        "{}",
        json!({"lostObserve":lost["code"],"status":status["data"]["state"]})
    );
    assert_eq!(status["data"]["state"], "lost", "{status}");
    person("replugged").await;
    let recovered = client.call("connect", json!({"targetId":target})).await;
    assert_eq!(recovered["ok"], true, "{recovered}");
    let after = frame_pts(&recovered["data"]["observation"]);
    assert_ne!(after, before, "reconnect reused a stale frame");
    assert_eq!(recovered["data"]["lastAction"], Value::Null);
    let closed = client.call("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    released(&serial, Duration::from_secs(60)).await;
    cleanup.armed = false;
    drop(client.input);
    client.child.wait().await.unwrap();
    eprintln!(
        "{}",
        json!({"lostObserve":lost["code"],"statusAfterLoss":status["data"]["state"],"reconnected":true})
    );
}

/// The operator pulls USB during a single ten-second top-center contact.
/// A late pull fails the timing prerequisite instead of counting as mid-input.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One owned physical fault journey with explicit recovery and no replay"
)]
async fn usb_loss_during_long_press_preserves_outcome_without_replay() {
    let (serial, binary) = selected();
    let target = format!("device:{serial}");
    let context = format!("testrun-input-unplug-{}", uuid::Uuid::new_v4());
    let mut cleanup = ManagedCleanup {
        binary: binary.clone(),
        context: context.clone(),
        blocker: None,
        armed: true,
    };
    let mut client = task_client(&context).await;
    let mut monitor = task_client(&context).await;
    let connected = client.call("connect", json!({"targetId":target})).await;
    assert_eq!(connected["ok"], true, "{connected}");
    // Operator is ready at the cable before the timed contact starts.
    person("ready-for-input-unplug").await;
    let action =
        json!({"actions":[{"action":"long_press","point":{"x":0.5,"y":0.02},"durationMs":10000}]});
    let started = tokio::time::Instant::now();
    send_with_id(&mut client, 9201, "act", &action).await;
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "No active contact within five seconds"
        );
        let status = monitor.call("status", json!({})).await;
        if status["data"]["health"]["inputHealth"]["heldContact"] == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    eprintln!(
        "PULL-USB-NOW: contact is active; unplug the selected phone before the ten-second contact ends"
    );
    let lost_after = loop {
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "Cable loss missed the active-input window; this is not a mid-input pass"
        );
        let devices = tokio::time::timeout(
            Duration::from_millis(500),
            testrun::usbmux::list(std::path::Path::new("/var/run/usbmuxd")),
        )
        .await
        .expect("USB inventory timed out")
        .unwrap();
        if !devices.iter().any(|device| device.serial == serial) {
            break started.elapsed();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let interrupted = client.response(9201).await["result"]["structuredContent"].clone();
    let status = monitor.call("status", json!({})).await;
    assert_eq!(status["data"]["state"], "lost", "{status}");
    let history = status["data"]["lastAction"].clone();
    assert!(
        matches!(
            history["status"].as_str(),
            Some("failed" | "outcome_unknown" | "completed")
        ),
        "{history}"
    );
    // Even a locally completed send does not acknowledge device delivery.
    assert!(
        !history
            .to_string()
            .contains("\"deliveryAcknowledged\":true"),
        "{history}"
    );
    send_with_id(&mut client, 9201, "act", &action).await;
    let repeated = client.response(9201).await["result"]["structuredContent"].clone();
    assert!(
        matches!(
            repeated["code"].as_str(),
            Some("BATCH_FAILED" | "BATCH_OUTCOME_UNKNOWN")
        ) || repeated["data"]["state"] == "recorded",
        "{repeated}"
    );
    assert_eq!(
        monitor.call("status", json!({})).await["data"]["lastAction"],
        history
    );
    person("replugged-after-input").await;
    let recovered = client.call("connect", json!({"targetId":target})).await;
    assert_eq!(recovered["ok"], true, "{recovered}");
    assert_eq!(recovered["data"]["lastAction"], history, "{recovered}");
    let closed = client.call("disconnect", json!({})).await;
    assert_eq!(closed["data"]["closed"], true, "{closed}");
    released(&serial, Duration::from_secs(60)).await;
    cleanup.armed = false;
    for client in [client, monitor] {
        drop(client.input);
        let mut child = client.child;
        child.wait().await.unwrap();
    }
    eprintln!(
        "{}",
        json!({"usbLossAfterMs":lost_after.as_millis(),"interrupted":interrupted["code"],"outcome":history["status"],"repeated":repeated["code"],"recovered":true})
    );
}
