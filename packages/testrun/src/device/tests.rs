//! Owned runner startup rollback, retained cleanup and authenticated RPC contracts.
use crate::artifacts;

fn prepared_fixture(directory: std::path::PathBuf) -> super::PreparedRunner {
    super::PreparedRunner {
        runner: super::Runner::fixture(),
        configuration_dir: directory,
        process: std::sync::Arc::new(super::host::Host::new(
            tokio::process::Command::new("/bin/sleep")
                .arg("60")
                .kill_on_drop(true)
                .spawn()
                .unwrap(),
        )),
        heartbeat: None,
        stopped: false,
        serial: "Acme".into(),
        native_pid: Some(42),
        signing: serde_json::Value::Null,
        _cache: None,
    }
}

#[tokio::test]
async fn cancelled_runner_stop_retains_cleanup_and_failed_stop_can_be_retried() {
    use super::*;
    for failed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("owned-runner");
        artifacts::directory(&directory).unwrap();
        let (release, wait) = tokio::sync::oneshot::channel();
        let prepared = prepared_fixture(directory.clone());
        let mut runners = Runners::default();
        runners
            .runners
            .insert("Acme".into(), RunnerState::Ready(prepared));
        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                runners.stop_with("Acme", |prepared| async move {
                    wait.await.unwrap();
                    let result = if failed {
                        Err(Error::new(
                            "RUNNER_BUSY",
                            "Fixture native operation is active",
                        ))
                    } else {
                        Ok(())
                    };
                    (prepared, result)
                }),
            )
            .await
            .is_err()
        );
        assert!(directory.exists());
        assert_eq!(
            runners.runner("Acme").err().unwrap().code,
            "SESSION_CLOSING"
        );
        release.send(()).unwrap();
        let result = runners.stop("Acme").await;
        if failed {
            assert_eq!(result.unwrap_err().code, "RUNNER_BUSY");
            assert!(directory.exists());
            assert_eq!(
                runners.runner("Acme").err().unwrap().code,
                "SESSION_CLOSING"
            );
            runners
                .stop_with("Acme", |prepared| async move { (prepared, Ok(())) })
                .await
                .unwrap();
        } else {
            result.unwrap();
        }
        assert!(!directory.exists());
        runners.stop("Acme").await.unwrap();
        assert!(runners.runners.is_empty());
    }
}

#[tokio::test]
async fn busy_runner_stop_preserves_live_resources_and_acknowledged_stop_is_not_repeated() {
    use super::*;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("owned-runner");
    artifacts::directory(&directory).unwrap();
    let mut prepared = prepared_fixture(directory.clone());
    prepared.heartbeat = Some(tokio::spawn(std::future::pending()));
    let error = prepared
        .stop_with(
            async { Err(Error::new("RUNNER_BUSY", "Fixture operation is active")) },
            async { panic!("Busy stop must not infer native death") },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "RUNNER_BUSY");
    assert!(directory.exists());
    assert!(prepared.process.exited().unwrap().is_none());
    assert!(!prepared.heartbeat.as_ref().unwrap().is_finished());
    assert!(!prepared.stopped);

    // Simulate the owned host exiting after the native operation settles.
    prepared.process.kill_fixture();
    prepared.process.wait().await.unwrap();
    // A file in place of the preparation directory makes local release fail
    // after remote acknowledgement, without changing host permissions.
    std::fs::remove_dir(&directory).unwrap();
    std::fs::write(&directory, b"fixture obstruction").unwrap();
    assert_eq!(
        prepared
            .stop_with(async { Ok(()) }, std::future::pending())
            .await
            .unwrap_err()
            .code,
        "CLEANUP_FAILED"
    );
    assert!(prepared.stopped);
    assert!(prepared.heartbeat.is_none());
    assert!(directory.is_file());
    // A local cleanup retry needs neither remote access nor another stop RPC.
    std::fs::remove_file(&directory).unwrap();
    std::fs::create_dir(&directory).unwrap();
    prepared
        .stop_with(std::future::pending(), std::future::pending())
        .await
        .unwrap();
    assert!(!directory.exists());
    prepared
        .stop_with(std::future::pending(), std::future::pending())
        .await
        .unwrap();
}
#[tokio::test]
async fn runner_host_loss_does_not_release_unverified_native_resources() {
    use super::*;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("owned-runner");
    artifacts::directory(&directory).unwrap();
    let mut prepared = prepared_fixture(directory.clone());
    prepared.process.kill_fixture();
    prepared.process.wait().await.unwrap();
    let failure = prepared
        .stop_with(
            async {
                Err(Error::new(
                    "DEVICE_DISCONNECTED",
                    "Fixture remote transport lost",
                ))
            },
            async {
                Err(Error::new(
                    "PLATFORM_PROTOCOL",
                    "Fixture process inventory unavailable",
                ))
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLEANUP_FAILED");
    assert_eq!(
        failure.details.unwrap()["stop"]["code"],
        "DEVICE_DISCONNECTED"
    );
    assert!(!prepared.stopped);
    assert!(directory.exists());
    prepared
        .stop_with(
            async {
                Err(Error::new(
                    "DEVICE_DISCONNECTED",
                    "Fixture native runner has exited",
                ))
            },
            async { Ok(()) },
        )
        .await
        .unwrap();
    assert!(prepared.stopped);
    assert!(!directory.exists());
}

#[tokio::test]
async fn startup_detects_an_exited_host_without_waiting_for_the_rpc_deadline() {
    use super::*;
    let root = tempfile::tempdir().unwrap();
    let mut runner = Runner::fixture();
    let prepared = prepared_fixture(root.path().join("owned-runner"));
    runner.host = Some(std::sync::Arc::downgrade(&prepared.process));
    prepared.process.kill_fixture();
    prepared.process.wait().await.unwrap();
    let error = tokio::time::timeout(Duration::from_millis(500), wait_ready(&runner))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, "RUNNER_EXITED");
}

#[tokio::test]
async fn readiness_without_native_identity_cannot_publish_a_runner() {
    use super::*;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("owned-runner");
    artifacts::directory(&directory).unwrap();
    let prepared = prepared_fixture(directory.clone());
    let mut runners = Runners::default();
    let error = runners
        .publish_when_ready("Acme", prepared, async {
            Ok(json!({"protocolVersion":1,"capabilities":[]}))
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "PLATFORM_PROTOCOL");
    assert!(runners.runners.is_empty());
    assert!(!directory.exists());
}

#[tokio::test]
async fn failed_or_cancelled_readiness_releases_the_unpublished_child_and_files() {
    use super::*;
    for cancelled in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("owned");
        artifacts::directory(&directory).unwrap();
        let process = tokio::process::Command::new("/bin/sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = process.id().unwrap() as i32;
        let prepared = PreparedRunner {
            runner: Runner {
                device_id: 0,
                session_id: "fixture".into(),
                token: "fixture".into(),
                port: 1,
                host: None,
            },
            configuration_dir: directory.clone(),
            process: std::sync::Arc::new(super::host::Host::new(process)),
            heartbeat: None,
            stopped: false,
            serial: "Acme".into(),
            native_pid: None,
            signing: Value::Null,
            _cache: None,
        };
        let mut runners = Runners::default();
        if cancelled {
            let result = tokio::time::timeout(
                Duration::from_millis(20),
                runners.publish_when_ready("Acme", prepared, std::future::pending()),
            )
            .await;
            assert!(result.is_err());
        } else {
            assert_eq!(
                runners
                    .publish_when_ready("Acme", prepared, async {
                        Err(Error::new(
                            "RUNNER_START_FAILED",
                            "Fixture readiness failed",
                        ))
                    })
                    .await
                    .unwrap_err()
                    .code,
                "RUNNER_START_FAILED"
            );
        }
        assert!(!directory.exists());
        assert!(runners.runners.is_empty());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        // SAFETY: signal zero observes only this fixture's recorded child.
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "Unready owned child survived rollback"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
#[test]
fn rpc_breakdown_preserves_missing_runner_measurements() {
    let elapsed = std::time::Duration::from_millis(750);
    let timings = super::rpc::rpc_timings(elapsed, &serde_json::json!(500));
    assert_eq!(timings["roundTrip"], 750.0);
    assert_eq!(timings["runner"], 500.0);
    assert_eq!(timings["hostAndTransport"], 250.0);
    assert!(super::rpc::rpc_timings(elapsed, &serde_json::Value::Null)["runner"].is_null());
    assert_eq!(
        super::rpc::rpc_timings(elapsed, &serde_json::json!(751))["hostAndTransport"],
        0.0
    );
}
#[cfg(feature = "acceptance-ios-runner")]
use super::*;

/// Uses a human-started owned runner; never builds, installs or controls apps.
#[tokio::test]
#[cfg(feature = "acceptance-ios-runner")]
async fn physical_owned_runner_authentication_and_typed_errors() {
    let serial = std::env::var("TESTRUN_TEST_DEVICE_SERIAL")
        .expect("Select the physical device serial explicitly");
    let path = std::env::var("TESTRUN_TEST_DEVICE_CONFIG")
        .expect("Supply the private owned runner session file");
    let bytes = std::fs::read(path).expect("Read private session file");
    assert!(bytes.len() <= 4096);
    let configuration: Value = serde_json::from_slice(&bytes).expect("Decode session file");
    let devices = crate::usbmux::list(Path::new("/var/run/usbmuxd"))
        .await
        .expect("Discover USB runners");
    let selected = devices
        .iter()
        .find(|d| d.serial == serial)
        .expect("Explicitly selected USB device is connected");
    let token = configuration["token"].as_str().expect("Session token");
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    let runner = Runner {
        device_id: selected.id,
        session_id: configuration["sessionId"]
            .as_str()
            .expect("Session identity")
            .into(),
        token: token.into(),
        port: u16::try_from(configuration["port"].as_u64().expect("Session port")).unwrap(),
        host: None,
    };
    let hello = runner
        .call("hello", json!({}), None, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(hello.data["protocolVersion"], 1);
    assert_eq!(hello.data["sourceMode"], "device_frame_sequence");
    assert!(hello.payload.is_empty());
    let mut unauthorized = runner.clone();
    unauthorized.token = "0".repeat(64);
    let error = unauthorized
        .call("hello", json!({}), None, Duration::from_secs(10))
        .await
        .err()
        .expect("Reject invalid token");
    assert_eq!(error.code, "UNAUTHORIZED");
    let error = runner
        .call(
            "unknown_test_operation",
            json!({}),
            None,
            Duration::from_secs(10),
        )
        .await
        .err()
        .expect("Reject unsupported operation");
    assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
    let error = runner
        .call("ui_snapshot", json!({}), None, Duration::from_secs(10))
        .await
        .err()
        .expect("Require explicit UI scope");
    assert_eq!(error.code, "INVALID_ARGUMENT");
    assert!(
        runner
            .call("hello", json!({}), None, Duration::from_secs(10))
            .await
            .is_ok()
    );
}
