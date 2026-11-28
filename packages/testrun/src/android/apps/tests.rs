//! Owned adb boundary fixtures for inventory cancellation and helper cleanup.
use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

fn fixture(phase: &str) -> (tempfile::TempDir, Android) {
    let directory = tempfile::tempdir().unwrap();
    let adb = directory.path().join("adb");
    fs::write(
        &adb,
        include_str!("../../../tests/fixtures/android_apps.sh"),
    )
    .unwrap();
    fs::set_permissions(&adb, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&adb, directory.path().join("app_process")).unwrap();
    symlink(&adb, directory.path().join("cat")).unwrap();
    fs::write(directory.path().join("phase"), phase).unwrap();
    (directory, Android { adb })
}

async fn until(mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "Inventory fixture did not reach its condition"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn cleaned(root: &Path) {
    until(|| root.join("cleaned").exists()).await;
    let remote = fs::read_to_string(root.join("remote")).unwrap();
    assert!(!Path::new(remote.trim()).exists());
}

#[tokio::test]
async fn cancelled_inventory_retains_provisioning_and_helper_until_cleanup() {
    for phase in ["push", "helper"] {
        let (directory, adapter) = fixture(phase);
        let root = directory.path();
        let task = tokio::spawn(async move { adapter.apps("owned-fixture").await });
        until(|| root.join(phase).exists()).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        // The I/O owner outlives the caller; let its current stage complete.
        fs::write(root.join("release"), "").unwrap();
        cleaned(root).await;
    }
}

#[tokio::test]
async fn failed_inventory_removes_partial_transfers_and_helper_files() {
    for phase in ["push_failed", "helper_failed", "success"] {
        let (directory, adapter) = fixture(phase);
        let result = adapter.apps("owned-fixture").await.unwrap();
        cleaned(directory.path()).await;
        assert_eq!(result["apps"][0]["appId"], "com.example.acme");
        assert_eq!(result.get("namesUnavailable").is_some(), phase != "success");
    }
}

#[tokio::test]
async fn inventory_cleanup_failure_preserves_the_cause_without_fallback() {
    let (directory, adapter) = fixture("cleanup_failed");
    let error = adapter.apps("owned-fixture").await.unwrap_err();
    assert_eq!(error.code, "CLEANUP_FAILED");
    let details = error.details.unwrap();
    assert_eq!(details["cause"]["code"], "PLATFORM_FAILED");
    assert_eq!(details["inventoryError"]["code"], "PLATFORM_FAILED");
    let remote = fs::read_to_string(directory.path().join("remote")).unwrap();
    // The trap may already have removed it; refusal to verify removal remains
    // a cleanup failure regardless of artifact presence.
    assert!(
        !Path::new(remote.trim()).exists(),
        "The trap removed the artifact even though verification failed"
    );
}

#[tokio::test]
async fn truncated_inventory_provisioning_cleans_without_a_host_cleanup_call() {
    let (directory, adapter) = fixture("success");
    let script = inventory_script("/data/local/tmp/testrun-apps-fixture.jar");
    let result = crate::process::run_input(
        &adapter.adb,
        &["-s", "owned-fixture", "shell", "-T", &script],
        b"truncated",
        Duration::from_secs(5),
        4096,
    )
    .await;
    assert!(result.is_err(), "A partial helper must never execute");
    let remote = fs::read_to_string(directory.path().join("remote")).unwrap();
    assert!(!Path::new(remote.trim()).exists());
    assert!(!directory.path().join("helper").exists());
    assert!(
        !directory.path().join("cleaned").exists(),
        "Only the remote trap performed cleanup"
    );
}
