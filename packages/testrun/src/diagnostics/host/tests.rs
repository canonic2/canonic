use super::*;
#[test]
fn diagnostic_services_host_selection_and_native_units_are_explicit() {
    let options = Options {
        target_id: "process:Acme:42".into(),
        service_id: "api".into(),
        host_id: "Acme".into(),
        process_id: 42,
        expected_generation: None,
    };
    Capture::new(options.clone()).unwrap();
    let mut invalid = options;
    invalid.process_id = 0;
    assert!(Capture::new(invalid).is_err());
    let (parent, metrics, native) =
        ps_snapshot(42, "42 1 512 2.5 /Acme Application/worker").unwrap();
    assert_eq!(parent, Some(1));
    assert_eq!(metrics[0]["value"], 524288.0);
    assert_eq!(metrics[0]["unit"], "bytes");
    assert_eq!(metrics[1]["unit"], "percent");
    assert_eq!(native["environmentRead"], false);
    assert!(ps_snapshot(43, "42 1 512 2.5 worker").is_err());
    assert!(ps_snapshot(42, "42 1 512 NaN worker").is_err());
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[tokio::test]
async fn diagnostic_services_owned_child_snapshot_fences_birth_and_never_signals_the_borrowed_source()
 {
    let mut child = tokio::process::Command::new("/bin/sleep")
        .arg("20")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    let mut options = Options {
        target_id: format!("process:Acme:{pid}"),
        service_id: "api".into(),
        host_id: "Acme".into(),
        process_id: pid,
        expected_generation: None,
    };
    let raw = Capture::new(options.clone())
        .unwrap()
        .read(Duration::from_secs(3))
        .await
        .unwrap();
    let packet: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(packet["pid"], pid);
    assert_eq!(packet["ownership"], "borrowed");
    assert!(packet["metrics"].as_array().unwrap().len() >= 2);
    let index = crate::diagnostics::analysis::decode::decode("host-process-v1", &raw).unwrap();
    assert!(index.rows.iter().any(|r| r.kind == "metric"));
    options.expected_generation = packet["processGeneration"].as_str().map(str::to_owned);
    Capture::new(options.clone())
        .unwrap()
        .read(Duration::from_secs(3))
        .await
        .unwrap();
    options.expected_generation = Some("different-birth".into());
    assert_eq!(
        Capture::new(options.clone())
            .unwrap()
            .read(Duration::from_secs(3))
            .await
            .unwrap_err()
            .code,
        "PROCESS_GENERATION_CHANGED"
    );
    assert!(child.try_wait().unwrap().is_none());
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    assert_eq!(
        Capture::new(options)
            .unwrap()
            .read(Duration::from_secs(3))
            .await
            .unwrap_err()
            .code,
        "PROCESS_UNAVAILABLE"
    );
}
#[cfg(unix)]
#[tokio::test]
async fn diagnostic_services_box_reader_uses_only_exact_log_argv_and_rejects_changed_operation() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("box-peer");
    let script = "#!/bin/sh\n[ \"$1\" = --project ] || exit 2\n[ \"$2\" = '/private/tmp/Acme project;literal' ] || exit 2\n[ \"$3\" = logs ] || exit 2\n[ \"$4\" = 'acme#feature' ] || exit 2\n[ \"$5\" = --operation ] || exit 2\n[ \"$6\" = node-1 ] || exit 2\n[ \"$7\" = --stream ] || exit 2\n[ \"$8\" = stderr ] || exit 2\n[ \"$9\" = --json ] || exit 2\nprintf '%s' '{\"workspace\":\"acme#feature\",\"operation\":\"node-1\",\"stream\":\"stderr\",\"tail_only\":true,\"text\":\"Acme fixture failure\"}'\n";
    std::fs::write(&program, script).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let o = BoxOptions {
        target_id: "service:Acme:api".into(),
        service_id: "api".into(),
        project: "/private/tmp/Acme project;literal".into(),
        workspace: "acme#feature".into(),
        operation_id: "node-1".into(),
        stream: "stderr".into(),
    };
    let bytes = o.read_with(&program, Duration::from_secs(3)).await.unwrap();
    let index = crate::diagnostics::analysis::decode::decode("box-log-v1", &bytes).unwrap();
    assert_eq!(index.rows[0].data["tail_only"], true);
    std::fs::write(&program,"#!/bin/sh\nprintf '%s' '{\"workspace\":\"acme#feature\",\"operation\":\"other\",\"stream\":\"stderr\"}'\n").unwrap();
    assert_eq!(
        o.read_with(&program, Duration::from_secs(3))
            .await
            .unwrap_err()
            .code,
        "SCOPE_MISMATCH"
    );
}
