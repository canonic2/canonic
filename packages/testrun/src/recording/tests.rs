//! Recording job outcomes and evidence retention.
use super::*;

#[test]
fn manifest_failure_reports_the_finalized_artifact_and_preserves_existing_evidence() {
    let root = tempfile::tempdir().unwrap();
    let temporary = root.path().join("owned.partial.mp4");
    let final_path = root.path().join("owned.mp4");
    artifacts::write(&temporary, b"verified video fixture").unwrap();
    let manifest = final_path.with_extension("json");
    std::fs::create_dir(&manifest).unwrap();
    std::fs::write(manifest.join("existing"), b"foreign evidence").unwrap();
    let error = publish(
        &temporary,
        &final_path,
        "owned",
        json!({"status":"complete"}),
        json!({"verification":"fixture"}),
    )
    .unwrap_err();
    assert_eq!(error.code, "EVIDENCE_IO");
    assert_eq!(
        error.details.as_ref().unwrap()["artifact"]["path"],
        final_path.to_str().unwrap()
    );
    assert_eq!(error.details.unwrap()["manifestPublished"], false);
    assert_eq!(
        std::fs::read(&final_path).unwrap(),
        b"verified video fixture"
    );
    assert_eq!(
        std::fs::read(manifest.join("existing")).unwrap(),
        b"foreign evidence"
    );
    assert!(!temporary.exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}

#[tokio::test]
async fn exhausted_finalization_budget_aborts_the_worker_and_preserves_partial_evidence() {
    let root = tempfile::tempdir().unwrap();
    let partial = root.path().join("owned.partial.mp4");
    artifacts::write(&partial, b"owned incomplete evidence").unwrap();
    let mut recordings = Recordings::default();
    recordings.jobs.insert(
        "bounded".into(),
        Recording {
            stop: None,
            job: Some(tokio::spawn(std::future::pending())),
            result: None,
            started: Instant::now(),
            finalization_started: None,
            finalization_budget: Duration::from_millis(20),
            finalization_watchdog: None,
            finalization_expired: Default::default(),
            temporary: partial.clone(),
            android: None,
        },
    );
    let result = recordings.stop("bounded").await.unwrap();
    assert_eq!(result["error"]["code"], "TIMEOUT");
    assert_eq!(result["partialPath"], partial.to_str().unwrap());
    assert!(!recordings.active("bounded"));
    assert_eq!(recordings.stop("bounded").await.unwrap(), result);
    assert!(partial.exists());
}

#[tokio::test]
#[cfg(unix)]
async fn slow_android_finalization_remains_owned_and_can_finish_after_stop_returns() {
    let root = tempfile::tempdir().unwrap();
    let android = crate::android::Android {
        adb: "/usr/bin/true".into(),
    }
    .recording("Acme-owned", "slow-copy");
    let (finish, pending) = oneshot::channel();
    let mut recordings = Recordings::default();
    recordings.jobs.insert(
        "slow-copy".into(),
        Recording {
            stop: None,
            job: Some(tokio::spawn(async move {
                pending.await.unwrap();
                Ok(json!({"status":"completed","artifact":{"bytes":4096}}))
            })),
            result: None,
            started: Instant::now(),
            temporary: root.path().join("partial.mp4"),
            android: Some(android),
            finalization_started: None,
            finalization_budget: Duration::from_secs(110),
            finalization_watchdog: None,
            finalization_expired: Default::default(),
        },
    );
    assert_eq!(
        recordings.stop("slow-copy").await.unwrap()["status"],
        "finalizing"
    );
    assert!(recordings.active("slow-copy"));
    assert_eq!(
        recordings.status("slow-copy").await.unwrap()["status"],
        "finalizing"
    );
    finish.send(()).unwrap();
    let result = recordings.stop("slow-copy").await.unwrap();
    assert_eq!(result["status"], "completed");
    assert!(!recordings.active("slow-copy"));
    assert_eq!(recordings.stop("slow-copy").await.unwrap(), result);
}

#[tokio::test]
#[cfg(unix)]
async fn cleanup_failure_retains_operation_failure_and_stable_terminal_result() {
    let android = crate::android::Android {
        adb: "/usr/bin/false".into(),
    }
    .recording("Acme-owned", "failed-cleanup");
    let mut recordings = Recordings::default();
    recordings.jobs.insert(
        "failed-cleanup".into(),
        Recording {
            stop: None,
            job: Some(tokio::spawn(async {
                Err(Error::new("RECORDING_FAILED", "Fixture exit failure")
                    .details(json!({"exitCode":7})))
            })),
            result: None,
            started: Instant::now(),
            temporary: PathBuf::from("unused-fixture"),
            android: Some(android),
            finalization_started: None,
            finalization_budget: Duration::from_secs(110),
            finalization_watchdog: None,
            finalization_expired: Default::default(),
        },
    );
    let result = recordings.stop("failed-cleanup").await.unwrap();
    assert_eq!(result["error"]["code"], "CLEANUP_FAILED");
    assert_eq!(
        result["error"]["details"]["result"]["error"]["details"]["exitCode"],
        7
    );
    assert_eq!(recordings.stop("failed-cleanup").await.unwrap(), result);
    assert_eq!(recordings.status("failed-cleanup").await.unwrap(), result);
    assert!(
        recordings.active("failed-cleanup"),
        "Uncertain cleanup must fence another recording"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn cancelled_stop_resumes_cleanup_without_losing_the_worker_result() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let adb = root.path().join("adb");
    std::fs::write(
        &adb,
        "#!/bin/sh\ncd \"$(dirname \"$0\")\"\ntouch removing\nsleep 0.2\ntouch removed\n",
    )
    .unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    let android = crate::android::Android { adb }.recording("Acme-owned", "cancelled");
    let mut recordings = Recordings::default();
    recordings.jobs.insert(
        "cancelled".into(),
        Recording {
            stop: None,
            job: Some(tokio::spawn(async {
                Err(Error::new("RECORDING_FAILED", "Fixture exit failure"))
            })),
            result: None,
            started: Instant::now(),
            temporary: root.path().join("partial.mp4"),
            android: Some(android),
            finalization_started: None,
            finalization_budget: Duration::from_secs(110),
            finalization_watchdog: None,
            finalization_expired: Default::default(),
        },
    );
    let removing = root.path().join("removing");
    {
        let stop = recordings.stop("cancelled");
        tokio::pin!(stop);
        tokio::select! {
            result = &mut stop => panic!("Cleanup completed before fixture gate: {result:?}"),
            () = async {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
                while !removing.exists() {
                    assert!(tokio::time::Instant::now() < deadline);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            } => {}
        }
    }
    let result = recordings.stop("cancelled").await.unwrap();
    assert_eq!(result["error"]["code"], "RECORDING_FAILED");
    assert!(root.path().join("removed").exists());
    assert_eq!(recordings.stop("cancelled").await.unwrap(), result);
}
#[tokio::test]
async fn failed_worker_has_a_stable_terminal_result() {
    let mut recordings = Recordings::default();
    recordings.jobs.insert(
        "failed".into(),
        Recording {
            stop: None,
            job: Some(tokio::spawn(async { panic!("Fixture worker failure") })),
            result: None,
            started: Instant::now(),
            temporary: PathBuf::from("unused-fixture"),
            android: None,
            finalization_started: None,
            finalization_budget: Duration::from_secs(10),
            finalization_watchdog: None,
            finalization_expired: Default::default(),
        },
    );
    let result = recordings.stop("failed").await.unwrap();
    assert_eq!(result["status"], "failed");
    assert_eq!(result["error"]["code"], "RECORDING_FAILED");
    assert_eq!(recordings.stop("failed").await.unwrap(), result);
    assert_eq!(recordings.status("failed").await.unwrap(), result);
    recordings.forget("failed");
}
#[tokio::test]
async fn releasing_terminal_recordings_preserves_their_evidence() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("evidence.mp4");
    artifacts::write(&path, b"fixture evidence").unwrap();
    let mut recordings = Recordings::default();
    for index in 0..100 {
        let id = index.to_string();
        let evidence = path.clone();
        recordings.jobs.insert(
            id.clone(),
            Recording {
                stop: None,
                job: Some(tokio::spawn(async move {
                    Ok(json!({"status":"complete","artifact":{"path":evidence}}))
                })),
                result: None,
                started: Instant::now(),
                temporary: root.path().join("unused"),
                android: None,
                finalization_started: None,
                finalization_budget: Duration::from_secs(10),
                finalization_watchdog: None,
                finalization_expired: Default::default(),
            },
        );
        let result = recordings.stop(&id).await.unwrap();
        assert_eq!(result["status"], "complete");
        assert_eq!(recordings.stop(&id).await.unwrap(), result);
        recordings.forget(&id);
        assert_eq!(recordings.status(&id).await.unwrap_err().code, "NOT_FOUND");
    }
    assert!(recordings.jobs.is_empty());
    assert_eq!(std::fs::read(path).unwrap(), b"fixture evidence");
}
