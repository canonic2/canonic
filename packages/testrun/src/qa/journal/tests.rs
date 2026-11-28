//! Durable outcome recovery with real files and controlled owner termination.
use super::*;
#[cfg(unix)]
use std::time::Duration;

#[test]
fn interrupted_dispatch_stays_unknown_and_completed_reports_survive_reopening() {
    let root = tempfile::tempdir().unwrap();
    let archive = Archive::create(root.path()).unwrap();
    archive
        .record(
            "route",
            &json!({"commandId":"route","status":"outcome_unknown","steps":[]}),
        )
        .unwrap();
    archive
        .record(
            "route:0",
            &json!({"commandId":"route:0","state":"dispatched"}),
        )
        .unwrap();
    assert_eq!(
        Archive::at(root.path()).status("route").unwrap()["actionOutcomes"][0]["state"],
        "outcome_unknown"
    );
    archive.record("route:0", &json!({"commandId":"route:0","state":"completed","observed":false,"result":{"completion":"reports_sent","deliveryAcknowledged":false}})).unwrap();
    let recovered = Archive::at(root.path()).status("route").unwrap();
    assert_eq!(recovered["status"], "outcome_unknown");
    assert_eq!(recovered["steps"], json!([]));
    assert!(recovered.get("observation").is_none());
    assert_eq!(recovered["actionOutcomes"][0]["state"], "completed");
    assert_eq!(recovered["actionOutcomes"][0]["observed"], false);
    assert_eq!(
        recovered["actionOutcomes"][0]["result"]["deliveryAcknowledged"],
        false
    );
    // Final publication supplies the complete batch and image reference together.
    let completed = json!({"commandId":"route","status":"completed",
        "steps":[{"index":0,"ok":true,"observation":{"artifactId":"image"}}],
        "observation":{"artifactId":"image"}});
    archive.record("route", &completed).unwrap();
    let recovered = Archive::at(root.path()).status("route").unwrap();
    assert_eq!(recovered["status"], "completed");
    assert_eq!(recovered["steps"], completed["steps"]);
    assert_eq!(recovered["observation"], completed["observation"]);
    assert_eq!(recovered["actionOutcomes"][0]["state"], "completed");
    assert_eq!(archive.status("missing").unwrap_err().code, "NOT_FOUND");
    archive
        .record("../../escape", &json!({"state":"completed"}))
        .unwrap();
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    archive
        .record(
            "route:0",
            &json!({"commandId":"route:0","status":"failed","steps":[]}),
        )
        .unwrap();
    // Batch IDs and derived action IDs have separate storage namespaces.
    assert_eq!(archive.status("route:0").unwrap()["state"], "completed");
    archive.record("corrupt", &json!(false)).unwrap();
    assert_eq!(archive.status("corrupt").unwrap_err().code, "JOURNAL_IO");
}

#[cfg(unix)]
#[test]
fn process_fixture() {
    let Some(directory) = std::env::var_os("TESTRUN_TEST_OUTCOME_OWNER") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let archive = Archive::create(&directory).unwrap();
    let _listener = std::os::unix::net::UnixListener::bind(directory.join("control.sock")).unwrap();
    archive
        .record(
            "route",
            &json!({"commandId":"route","status":"outcome_unknown","steps":[]}),
        )
        .unwrap();
    archive
        .record(
            "route:0",
            &json!({"commandId":"route:0","state":"dispatched"}),
        )
        .unwrap();
    if std::env::var_os("TESTRUN_TEST_OUTCOME_COMPLETED").is_some() {
        archive.record("route:0", &json!({"commandId":"route:0","state":"completed","observed":false,"result":{"completion":"reports_sent"}})).unwrap();
    }
    fs::write(directory.join("ready"), b"ready").unwrap();
    loop {
        std::thread::park();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn killed_owner_leaves_queryable_outcomes_without_restoring_control() {
    for completed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("owned");
        crate::artifacts::directory(&directory).unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "qa::journal::tests::process_fixture"])
            .env("TESTRUN_TEST_OUTCOME_OWNER", &directory);
        if completed {
            command.env("TESTRUN_TEST_OUTCOME_COMPLETED", "1");
        }
        let mut child = command.spawn().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !directory.join("ready").exists() && std::time::Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() {
                panic!("Fixture exited before readiness");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let ready = directory.join("ready").exists();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(ready, "Owned fixture did not become ready");
        let input = crate::cli::Mobile {
            target: None,
            command: crate::cli::Action::Status {
                page: crate::cli::PageArgs::default(),
                action_id: None,
                wait_ms: None,
                include_details: false,
                endpoint: Some(directory.join("control.sock")),
                command_id: Some("route".into()),
            },
        };
        let status = crate::interactive::execute(&input).await.unwrap().unwrap();
        assert_eq!(status["data"]["status"], "outcome_unknown");
        assert_eq!(status["data"]["steps"], json!([]));
        assert!(status["data"].get("observation").is_none());
        assert_eq!(
            status["data"]["actionOutcomes"][0]["state"],
            if completed {
                "completed"
            } else {
                "outcome_unknown"
            }
        );
        assert_eq!(status["data"]["journalSource"], "archive");
    }
}
