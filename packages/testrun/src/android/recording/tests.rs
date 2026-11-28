//! Real subprocess fixtures for recording failures and cancellation cleanup.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn fixture(mode: &str) -> (tempfile::TempDir, Recording) {
    let directory = tempfile::tempdir().unwrap();
    let adb = directory.path().join("adb");
    let script = r#"#!/bin/sh
set -eu
cd "$(dirname "$0")"
case "$3" in
shell) case "$4" in
  *"exec screenrecord"*)
    echo $$
    printf '\000\000\000\030ftypisom\000\000\002\000isomiso2' > partial.mp4
    touch started
    if [ -f wait ]; then while [ ! -f interrupted ]; do sleep 0.02; done; fi
    exit 7 ;;
  "'kill' '-INT' "*) touch interrupted ;;
  "'rm' '-f' "*)
    touch removing
    if [ -f hang ]; then echo $$ > cleanup.pid; exec sleep 60; fi
    sleep 0.15
    touch removed ;;
  esac ;;
pull) cp partial.mp4 "$5" ;;
esac
"#;
    std::fs::write(&adb, script).unwrap();
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
    if !mode.is_empty() {
        std::fs::write(directory.path().join(mode), "").unwrap();
    }
    let recording = Android { adb }.recording("Acme-owned", "fixture");
    (directory, recording)
}

async fn wait_for(path: &Path) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while !path.exists() {
        assert!(tokio::time::Instant::now() < deadline, "{}", path.display());
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn unsuccessful_exit_preserves_partial_and_fails_natural_and_stopped_recordings() {
    for mode in ["", "wait"] {
        let (directory, recording) = fixture(mode);
        let path = directory.path().join("evidence.mp4");
        let (stop, signal) = oneshot::channel();
        let worker = recording.clone();
        let output = path.clone();
        let job = tokio::spawn(async move { worker.run(&output, 5000, signal).await });
        wait_for(&directory.path().join("started")).await;
        if mode == "wait" {
            stop.send(()).unwrap();
        }
        let error = job.await.unwrap().unwrap_err();
        assert_eq!(error.code, "RECORDING_FAILED");
        assert_eq!(error.details.unwrap()["exitCode"], 7);
        assert_eq!(&std::fs::read(path).unwrap()[4..8], b"ftyp");
        recording.cleanup().await.unwrap();
        assert!(directory.path().join("removed").exists());
    }
}

#[tokio::test]
async fn aborted_worker_retains_child_and_cleanup_waits_for_remote_removal() {
    let (directory, recording) = fixture("wait");
    let path = directory.path().join("evidence.mp4");
    let (_stop, signal) = oneshot::channel();
    let worker = recording.clone();
    let job = tokio::spawn(async move { worker.run(&path, 5000, signal).await });
    wait_for(&directory.path().join("started")).await;
    // The run keeps the child in shared state even when its future is cancelled.
    job.abort();
    assert!(job.await.unwrap_err().is_cancelled());
    recording.cleanup().await.unwrap();
    assert!(directory.path().join("interrupted").exists());
    assert!(directory.path().join("removed").exists());
    assert!(recording.0.lock().await.child.is_none());
    recording.cleanup().await.unwrap();
}

#[tokio::test]
async fn cleanup_timeout_is_reported_and_reaps_its_adb_process() {
    let (directory, recording) = fixture("hang");
    let error = recording.cleanup().await.unwrap_err();
    assert_eq!(error.code, "CLEANUP_FAILED");
    assert_eq!(error.details.unwrap()["failures"][0]["code"], "TIMEOUT");
    let pid: i32 = std::fs::read_to_string(directory.path().join("cleanup.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: signal zero only inspects this fixture's recorded subprocess.
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
    assert!(!directory.path().join("removed").exists());
}
