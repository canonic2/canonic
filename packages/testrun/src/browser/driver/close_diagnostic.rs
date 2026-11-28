//! Instrumented selected-host diagnostic: child exit and stdio closure are separate witnesses.
use super::*;
use crate::browser::{
    contract::{ContextOptions, Engine, LaunchOptions, Mode},
    objects::ObjectRef,
};
use futures_util::FutureExt;
use serde_json::json;
use std::time::Duration;

async fn channel(
    connection: &Connection,
    owner: &ObjectRef,
    method: &str,
    params: Value,
    field: &str,
    kind: &str,
) -> ObjectRef {
    let reply = connection
        .retained(
            owner,
            method,
            params,
            Instant::now() + Duration::from_secs(30),
            false,
        )
        .await
        .unwrap()
        .unwrap();
    connection
        .resolve(reply[field]["guid"].as_str().unwrap(), kind)
        .await
        .unwrap()
}

#[tokio::test]
async fn firefox_headed_child_exit_and_stdio_close_diagnostic() {
    let cache = PathBuf::from(
        std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select the private browser cache"),
    );
    let root = tempfile::Builder::new()
        .prefix("tr-child-close-")
        .tempdir()
        .unwrap()
        .keep();
    let mut failures = vec![];
    for attempt in 0..3 {
        let directory = root.join(attempt.to_string());
        crate::artifacts::directory(&directory).unwrap();
        let mut bundle = DiagnosticBundle::inspect(
            &cache.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
            &cache.join("playwright-core-1.64.1/package"),
            &cache.join("browsers"),
            Instant::now() + Duration::from_secs(30),
        )
        .await
        .unwrap();
        bundle.child_observations = Some((
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/browser-child-observations.cjs"),
            directory.join("child-events.jsonl"),
        ));
        let mut driver = Driver::start(bundle, Instant::now() + Duration::from_secs(30))
            .await
            .unwrap();
        let mut deadline = None;
        let exercise = std::panic::AssertUnwindSafe(async {
            let browser_type =
                driver.connection().bootstrap().unwrap().browser_types["firefox"].clone();
            let browser = channel(
                driver.connection(),
                &browser_type,
                "launch",
                LaunchOptions {
                    media_fixtures: Default::default(),
                    engine: Engine::Firefox,
                    mode: Mode::Headed,
                    proxy: None,
                    context: Default::default(),
                }
                .wire(),
                "browser",
                "Browser",
            )
            .await;
            let context = channel(
                driver.connection(),
                &browser,
                "newContext",
                ContextOptions::default().wire(),
                "context",
                "BrowserContext",
            )
            .await;
            let _page = channel(
                driver.connection(),
                &context,
                "newPage",
                json!({}),
                "page",
                "Page",
            )
            .await;
            driver.observe_descendants().await.unwrap();
            tokio::time::sleep(Duration::from_secs(4)).await;
            let original_deadline = Instant::now() + Duration::from_secs(30);
            deadline = Some(original_deadline);
            let browser_close = tokio::time::timeout_at(
                Instant::now() + Duration::from_secs(5),
                driver.connection().retained(
                    &browser,
                    "close",
                    json!({}),
                    original_deadline,
                    false,
                ),
            )
            .await;
            format!("{browser_close:?}")
        })
        .catch_unwind()
        .await;
        let original = driver
            .shutdown(deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(30)))
            .await;
        let report = json!({"browserClose":exercise.as_ref().ok(),"originalSucceeded":original.is_ok(),"shutdown":driver.shutdown_report()});
        std::fs::write(
            directory.join("original.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        if original.is_err() {
            failures.push(attempt);
            // Keep the same owner. This does not change the original deadline or result.
            loop {
                let retry = driver
                    .shutdown(Instant::now() + Duration::from_secs(10))
                    .await;
                std::fs::write(
                    directory.join("cleanup-retry.json"),
                    serde_json::to_vec_pretty(
                        &json!({"succeeded":retry.is_ok(),"shutdown":driver.shutdown_report()}),
                    )
                    .unwrap(),
                )
                .unwrap();
                if retry.is_ok() {
                    break;
                }
                eprintln!(
                    "Cleanup pending; preserving the same owned driver {}; evidence {}",
                    driver.pid().unwrap_or(0),
                    root.display()
                );
            }
        }
        assert!(driver.pid().is_none());
        if let Err(panic) = exercise {
            std::panic::resume_unwind(panic);
        }
    }
    eprintln!(
        "Instrumented Firefox child/stdio evidence: {}",
        root.display()
    );
    assert!(
        failures.is_empty(),
        "Original shutdown failures {failures:?}; evidence {}",
        root.display()
    );
}
