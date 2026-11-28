//! Missing prerequisites, bounded partial reports and untrusted property values.
use super::*;

#[tokio::test]
async fn missing_adb_is_a_structured_prerequisite_with_unverified_device_checks() {
    let root = tempfile::tempdir().unwrap();
    let adapter = Android {
        adb: root.path().join("Acme-missing-adb"),
    };
    let report = adapter.doctor("android:Acme-selected").await.unwrap();
    assert_eq!(report["ready"], false);
    assert_eq!(report["state"], "blocked");
    assert_eq!(
        report["checks"]["adb"]["error"]["code"],
        "PREREQUISITE_MISSING"
    );
    assert!(report["checks"]["adb"]["manualAction"].is_string());
    for key in &CHECKS[1..] {
        assert_eq!(report["checks"][*key]["state"], "unknown", "{report}");
    }
}

#[tokio::test]
async fn exhausted_budget_preserves_completed_checks_and_starts_no_more_platform_work() {
    let root = tempfile::tempdir().unwrap();
    let adapter = Android {
        adb: root.path().join("Acme-never-start"),
    };
    let deadline = Instant::now();
    let mut checks = json!({"adb":{"state":"ready"},"shell":{"state":"ready"}});
    adapter
        .observations("Acme-selected", deadline, &mut checks)
        .await;
    assert_eq!(checks["shell"]["state"], "ready");
    assert_eq!(checks["os"]["apiError"]["code"], "READINESS_PROBE_TIMEOUT");
    assert_eq!(
        checks["os"]["releaseError"]["code"],
        "READINESS_PROBE_TIMEOUT"
    );
    for key in ["boot", "screenshot", "accessibility"] {
        assert_eq!(checks[key]["state"], "unknown");
        assert_eq!(checks[key]["error"]["code"], "READINESS_PROBE_TIMEOUT");
    }
    let expired = adapter
        .diagnose("android:Acme-selected", deadline)
        .await
        .unwrap();
    assert_eq!(
        expired["checks"]["adb"]["error"]["code"],
        "READINESS_PROBE_TIMEOUT"
    );
}

#[test]
fn absent_malformed_and_failed_properties_remain_unknown_without_losing_other_evidence() {
    for value in ["", "unknown", "0", "4294967296", "36x"] {
        let check = os(Ok(value.into()), Ok("16".into()));
        assert_eq!(check["state"], "unknown", "{check}");
        assert_eq!(check["release"], "16");
    }
    let failed = os(
        Err(Error::new("PLATFORM_FAILED", "Acme API refusal")),
        Err(Error::new("TIMEOUT", "Acme release timeout")),
    );
    assert_eq!(failed["apiError"]["code"], "PLATFORM_FAILED");
    assert_eq!(failed["releaseError"]["code"], "TIMEOUT");
    for bytes in [vec![255], vec![b'x'; 257], b"Acme\0invalid".to_vec()] {
        assert_eq!(text(bytes).unwrap_err().code, "PLATFORM_PROTOCOL");
    }
}
