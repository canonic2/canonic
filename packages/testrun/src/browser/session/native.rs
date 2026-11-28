//! Explicit owned browser acceptance; no external URLs, profiles or devices.

use super::*;
use crate::browser::contract::{ContextOptions, Engine, Mode};

pub(super) async fn bundle() -> DiagnosticBundle {
    DiagnosticBundle::inspect(
        Path::new(&std::env::var_os("TESTRUN_TEST_NODE").expect("Select Node")),
        Path::new(&std::env::var_os("TESTRUN_TEST_PLAYWRIGHT_CORE").expect("Select the driver")),
        Path::new(&std::env::var_os("TESTRUN_TEST_BROWSERS").expect("Select owned browsers")),
        Instant::now() + Duration::from_secs(30),
    )
    .await
    .unwrap()
}
async fn settled(session: &Session, id: Uuid) -> Operation {
    let outcome = session
        .wait(id, Instant::now() + Duration::from_secs(60))
        .await
        .unwrap();
    assert!(outcome.settled, "Unsettled operation: {}", session.status());
    outcome
}
fn reference(value: &Value, field: &str) -> Reference {
    serde_json::from_value(value[field].clone()).unwrap()
}

#[tokio::test]
async fn firefox_shutdown_lifecycle_diagnostic() {
    let directory = tempfile::Builder::new()
        .prefix("testrun-step4-firefox-shutdown-")
        .tempdir()
        .unwrap()
        .keep();
    let mut failures = Vec::new();
    for (index, mode) in [Mode::Headless, Mode::Headed, Mode::Headless, Mode::Headed]
        .into_iter()
        .enumerate()
    {
        let private = directory.join(format!("cell-{index}"));
        crate::artifacts::directory(&private).unwrap();
        let selected = bundle().await;
        let session = Session::launch(
            selected,
            Uuid::new_v4(),
            Uuid::new_v4(),
            LaunchOptions {
                media_fixtures: Default::default(),
                proxy: None,
                engine: Engine::Firefox,
                mode,
                context: ContextOptions::default(),
            },
            &private,
        )
        .unwrap();
        let scenario = tokio::spawn(exercise(session.clone(), Engine::Firefox, mode)).await;
        let id = session.disconnect(30000).unwrap();
        let original = settled(&session, id).await;
        let original_status = session.status();
        let logs = session
            .0
            .host
            .driver
            .lock()
            .await
            .as_ref()
            .unwrap()
            .diagnostics();
        crate::artifacts::write(
            &private.join("driver-stderr.txt"),
            &logs.stderr_tail.into_iter().collect::<Vec<_>>(),
        )
        .unwrap();
        crate::artifacts::write(
            &private.join("original.json"),
            &serde_json::to_vec_pretty(&json!({"operation":original,"status":original_status,"stderrReadFailed":logs.read_failed,"omittedStderrBytes":logs.omitted_bytes})).unwrap(),
        )
        .unwrap();
        if original.outcome != Outcome::Succeeded {
            failures.push(index);
            let retry = settled(&session, session.disconnect(30000).unwrap()).await;
            crate::artifacts::write(
                &private.join("retry.json"),
                &serde_json::to_vec_pretty(&json!({"operation":retry,"status":session.status()}))
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(retry.outcome, Outcome::Succeeded, "{directory:?}");
        }
        scenario.expect("Owned lifecycle scenario panicked");
        assert_eq!(session.status()["phase"], "closed");
    }
    eprintln!("Private Firefox shutdown evidence: {}", directory.display());
    assert!(
        failures.is_empty(),
        "Original shutdown failures {failures:?}; {directory:?}"
    );
}

#[tokio::test]
async fn firefox_and_webkit_own_contexts_pages_handles_and_orderly_shutdown_in_both_modes() {
    for engine in [Engine::Firefox, Engine::Webkit] {
        for mode in [Mode::Headless, Mode::Headed] {
            let directory = tempfile::tempdir().unwrap();
            let private = directory.path().join("state");
            crate::artifacts::directory(&private).unwrap();
            let session = Session::launch(
                bundle().await,
                Uuid::new_v4(),
                Uuid::new_v4(),
                LaunchOptions {
                    media_fixtures: Default::default(),
                    proxy: None,
                    engine,
                    mode,
                    context: ContextOptions::default(),
                },
                &private,
            )
            .unwrap();
            // A fixture assertion must not end the runtime before owned browser
            // cleanup. Join captures a panic while this task retains the owner.
            let scenario = tokio::spawn(exercise(session.clone(), engine, mode)).await;
            let id = session.disconnect(30000).unwrap();
            let cleanup = settled(&session, id).await;
            assert_eq!(cleanup.outcome, Outcome::Succeeded, "{}", session.status());
            assert_eq!(session.status()["phase"], "closed");
            assert!(session.status()["resources"].as_array().unwrap().is_empty());
            let pid = scenario.expect("Native lifecycle scenario panicked");
            assert_eq!(crate::process::start_time(pid), None);
            let id = session.disconnect(30000).unwrap();
            assert_eq!(settled(&session, id).await.outcome, Outcome::Succeeded);
        }
    }
}

async fn exercise(session: Session, engine: Engine, mode: Mode) -> u32 {
    let startup = settled(&session, session.startup()).await;
    assert_eq!(
        startup.outcome,
        Outcome::Succeeded,
        "{engine:?}/{mode:?}: {}",
        session.status()
    );
    let initial = startup.result.unwrap();
    let initial_context = reference(&initial, "context");
    let initial_page = reference(&initial, "page");
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::ContextCreate {
                options: ContextOptions::default(),
            },
            30000,
        )
        .unwrap();
    let context = reference(&settled(&session, id).await.result.unwrap(), "context");
    assert_ne!(context, initial_context);
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::PageCreate {
                context,
                select: true,
            },
            30000,
        )
        .unwrap();
    let page = reference(&settled(&session, id).await.result.unwrap(), "page");
    assert_ne!(page, initial_page);
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::HandleCreate {
                page,
                expression: "({label:'Acme'})".into(),
            },
            30000,
        )
        .unwrap();
    let outcome = settled(&session, id).await;
    assert_eq!(outcome.outcome, Outcome::Succeeded);
    let handle = reference(&outcome.result.unwrap(), "handle");
    let id = session
        .submit(Uuid::new_v4(), Request::HandleDispose { handle }, 30000)
        .unwrap();
    assert_eq!(settled(&session, id).await.outcome, Outcome::Succeeded);
    let id = session
        .submit(Uuid::new_v4(), Request::ContextClose { context }, 30000)
        .unwrap();
    assert_eq!(settled(&session, id).await.outcome, Outcome::Succeeded);
    assert!(session.status()["selectedPage"].is_null());
    let id = session
        .submit(Uuid::new_v4(), Request::PageSelect { page }, 30000)
        .unwrap();
    assert_eq!(
        settled(&session, id).await.code.as_deref(),
        Some("STALE_REFERENCE")
    );
    // The first context/page remains owned and selectable, never an
    // automatic fallback after the selected second context closes.
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::PageSelect { page: initial_page },
            30000,
        )
        .unwrap();
    assert_eq!(settled(&session, id).await.outcome, Outcome::Succeeded);
    let pid = session
        .0
        .host
        .driver
        .lock()
        .await
        .as_ref()
        .unwrap()
        .pid()
        .unwrap();
    assert!(
        session
            .0
            .host
            .driver
            .lock()
            .await
            .as_ref()
            .unwrap()
            .witnessed_processes()
            > 1,
        "No browser descendant was witnessed"
    );
    pid
}
