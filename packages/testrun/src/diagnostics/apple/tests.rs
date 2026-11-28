use super::*;
#[test]
fn apple_source_selection_rejects_implicit_targets_shell_values_and_transport_fallback() {
    for target in ["booted", "device:-x", "device:ACME;reboot", "android:ACME"] {
        assert!(
            Capture::new(target, "com.example.acme", "usb", true, true, 3, None, None).is_err()
        );
    }
    assert!(
        Capture::new(
            "simulator:ACME",
            "com.example.acme",
            "network",
            true,
            true,
            3,
            None,
            None
        )
        .is_err()
    );
    assert!(
        Capture::new(
            "device:ACME",
            "com.example.acme",
            "usb",
            true,
            true,
            0,
            None,
            None
        )
        .is_err()
    );
}
#[test]
fn apple_physical_syslog_keeps_original_clock_and_exact_pid() {
    assert!(bundle("com.example.acme-beta"));
    assert!(!bundle_token("com.example.acme-beta", "com.example.acme"));
    let value = syslog("Nov 7 10:30:01 Phone Acme[42] <Notice>: background task expired").unwrap();
    assert_eq!(value["pid"], 42);
    assert_eq!(value["process"], "Acme");
    assert!(value["processGeneration"].is_null());
    assert!(syslog("bad private line").is_none());
    assert_eq!(
        file_url("file:///private/App.app/Acme"),
        Some("/private/App.app/Acme".into())
    );
    assert!(file_url("file://other-host/private/App.app").is_none());
}
#[test]
fn apple_report_files_refuse_symlinks_and_uuid_matching_is_exact() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("report.ips");
    std::fs::write(&path, b"{}").unwrap();
    assert_eq!(read_file(&path, 2).unwrap(), b"{}");
    assert!(read_file(&path, 1).is_err());
    #[cfg(unix)]
    {
        let link = temp.path().join("link.ips");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read_file(&link, 2).is_err());
    }
    assert_eq!(
        reports::uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
        Some("AAAAAAAABBBBCCCCDDDDEEEEEEEEEEEE".into())
    );
    assert!(reports::uuid("build-name").is_none());
}
#[tokio::test]
async fn apple_stopped_capture_does_not_start_platform_work() {
    let mut c = Capture::new(
        "device:ACME",
        "com.example.acme",
        "usb",
        true,
        true,
        3,
        None,
        None,
    )
    .unwrap();
    let (_sender, rx) = tokio::sync::watch::channel(Some("stopped".into()));
    c.control(rx, Duration::from_secs(1));
    assert_eq!(c.read().await.err().unwrap().code, "CANCELLED");
}
