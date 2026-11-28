//! Explicit selected-driver acceptance; no browser is launched by these cases.

use super::*;
use std::time::Duration;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}

async fn bundle() -> DiagnosticBundle {
    let node = std::env::var_os("TESTRUN_TEST_NODE")
        .expect("Select Node through TESTRUN_TEST_NODE for the acceptance-browser-driver feature");
    let package =
        std::env::var_os("TESTRUN_TEST_PLAYWRIGHT_CORE").expect("Select the pinned driver package");
    let browsers =
        std::env::var_os("TESTRUN_TEST_BROWSERS").expect("Select a private browser cache");
    DiagnosticBundle::inspect(
        Path::new(&node),
        Path::new(&package),
        Path::new(&browsers),
        deadline(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn unchanged_driver_bootstraps_with_pinned_identity_and_releases_its_process() {
    let bundle = bundle().await;
    let private_browsers = bundle.browsers.clone();
    let mut driver = Driver::start(bundle, deadline()).await.unwrap();
    let pid = driver.owned.child.id().unwrap();
    assert!(crate::process::start_time(pid).is_some());
    assert_eq!(driver.identity().driver_version, "1.64.1");
    assert_eq!(driver.identity().node_version, "v24.12.0");
    assert_eq!(driver.identity().client_language, "rust");
    assert_eq!(driver.identity().node_sha256.len(), 64);
    let bootstrap = driver.connection().bootstrap().unwrap();
    assert_eq!(bootstrap.browser_types.len(), 3);
    for (name, reference) in bootstrap.browser_types {
        let data = driver
            .connection()
            .initializer(&reference)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(data["name"], name);
        assert!(Path::new(data["executablePath"].as_str().unwrap()).starts_with(&private_browsers));
    }
    let result = driver.terminate(deadline()).await.unwrap();
    assert_eq!(driver.owned.child.id(), None);
    assert_eq!(crate::process::start_time(pid), None);
    assert_eq!(driver.terminate(deadline()).await.unwrap(), result);
    assert!(!driver.connection().status().ready);
    assert!(!driver.diagnostics().read_failed);
}

#[tokio::test]
async fn interrupted_termination_keeps_the_same_owned_process_for_retry() {
    let mut driver = Driver::start(bundle().await, deadline()).await.unwrap();
    let pid = driver.owned.child.id().unwrap();
    assert!(matches!(
        driver.terminate(Instant::now()).await,
        Err(Error::CleanupPending)
    ));
    assert!(crate::process::start_time(pid).is_some());
    let mut termination = Box::pin(driver.terminate(deadline()));
    assert!(futures_util::poll!(termination.as_mut()).is_pending());
    drop(termination);
    assert_eq!(driver.owned.child.id(), Some(pid));
    driver.terminate(deadline()).await.unwrap();
    assert_eq!(driver.owned.child.id(), None);
    assert_eq!(crate::process::start_time(pid), None);
}

#[tokio::test]
async fn interrupted_stderr_join_retains_the_reader_until_retry_completes() {
    let mut driver = Driver::start(bundle().await, deadline()).await.unwrap();
    let exit = driver.terminate(deadline()).await.unwrap();
    // Isolate the final cleanup stage with a reader whose completion is under
    // fixture control, after the real owned process has already been reaped.
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    driver.stderr = Some(tokio::spawn(async move {
        let _ = released.await;
    }));
    let reader_id = driver.stderr.as_ref().unwrap().id();
    let mut termination = Box::pin(driver.terminate(deadline()));
    assert!(futures_util::poll!(termination.as_mut()).is_pending());
    drop(termination);
    assert_eq!(driver.stderr.as_ref().unwrap().id(), reader_id);
    assert!(matches!(
        driver.terminate(Instant::now()).await,
        Err(Error::CleanupPending)
    ));
    assert_eq!(driver.stderr.as_ref().unwrap().id(), reader_id);
    release.send(()).unwrap();
    assert_eq!(driver.terminate(deadline()).await.unwrap(), exit);
    assert!(driver.stderr.is_none());
}

#[tokio::test]
async fn killed_driver_reports_loss_and_can_be_reaped_without_restarting() {
    let mut driver = Driver::start(bundle().await, deadline()).await.unwrap();
    let pid = driver.owned.child.id().unwrap();
    let old = driver.connection().bootstrap().unwrap().browser_types["chromium"].clone();
    driver.owned.signal().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while driver.connection().status().loss.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!driver.connection().status().ready);
    assert!(matches!(
        driver.connection().initializer(&old).await,
        Err(connection::Error::Lost { .. })
    ));
    driver.terminate(deadline()).await.unwrap();
    assert_eq!(crate::process::start_time(pid), None);
}

#[tokio::test]
async fn owned_group_cleanup_removes_a_real_descendant_before_reaping_the_leader() {
    let directory = tempfile::tempdir().unwrap();
    let pidfile = directory.path().join("child.pid");
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("sleep 60 & printf '%s' $! > \"$1\"; wait")
        .arg("fixture")
        .arg(&pidfile)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut owned = OwnedChild::spawn(&mut command).unwrap();
    let child = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = tokio::fs::read_to_string(&pidfile).await
                && let Ok(pid) = text.parse::<u32>()
            {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(crate::process::start_time(child).is_some());
    owned.signal().unwrap();
    owned.child.wait().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while crate::process::start_time(child).is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(owned.child.id(), None);
}

#[tokio::test]
async fn diagnostic_selection_refuses_relative_paths_and_wrong_package_before_launch() {
    assert!(matches!(
        DiagnosticBundle::inspect(
            Path::new("node"),
            Path::new("package"),
            Path::new("browsers"),
            deadline()
        )
        .await,
        Err(Error::Paths)
    ));
    let directory = tempfile::tempdir().unwrap();
    tokio::fs::write(
        directory.path().join("package.json"),
        b"{\"name\":\"other\",\"version\":\"1.64.1\"}",
    )
    .await
    .unwrap();
    let node = std::env::var_os("TESTRUN_TEST_NODE").unwrap();
    assert!(matches!(
        DiagnosticBundle::inspect(
            Path::new(&node),
            directory.path(),
            directory.path(),
            deadline()
        )
        .await,
        Err(Error::Package)
    ));
}

#[tokio::test]
async fn driver_lifecycle_witness_refresh_retains_interrupted_query_and_unknown_teardown() {
    for (cancelled, teardown_before_result) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let mut driver = Driver::start(bundle().await, deadline()).await.unwrap();
        driver.observe_descendants().await.unwrap();
        let root = driver.owned.child.id().unwrap();
        let mut tracker = driver.witnesses.clone();
        let (release, released) = tokio::sync::oneshot::channel();
        driver.observation = Some(tokio::spawn(async move {
            released.await.unwrap();
            tracker.observe(root)?;
            Ok(tracker)
        }));
        if cancelled {
            let mut inspecting = Box::pin(driver.observe_before_close(deadline()));
            assert!(futures_util::poll!(inspecting.as_mut()).is_pending());
            drop(inspecting);
        } else {
            assert!(matches!(
                driver.observe_before_close(Instant::now()).await,
                Err(Error::CleanupPending)
            ));
        }
        assert!(
            driver.observation.is_some(),
            "The read-only query must remain owned after interruption"
        );
        assert!(
            !driver.uninspected,
            "A cancelled read before teardown has not lost an ownership window"
        );
        if teardown_before_result {
            driver.note_teardown_inspection(false);
            driver.connection.close().await.unwrap();
        }
        release.send(()).unwrap();
        driver.observe_before_close(deadline()).await.unwrap();
        assert!(driver.observation.is_none());
        assert_eq!(
            driver.uninspected, teardown_before_result,
            "Only teardown through an unobserved window retains unknown ownership"
        );
        driver.note_teardown_inspection(true);
        // No browser was launched. EOF releases this diagnostic driver naturally.
        let outcome = driver.shutdown(deadline()).await;
        if teardown_before_result {
            assert!(matches!(outcome, Err(Error::CleanupPending)));
        } else {
            assert!(
                outcome.is_ok(),
                "A completed refresh before teardown can finish naturally: {outcome:?}"
            );
        }
        assert!(driver.owned.child.id().is_none());
        assert!(driver.exit.is_some());
        assert_eq!(driver.witnesses.remaining().unwrap(), 0);
        assert!(driver.diagnostics().stderr_closed);
    }
}
