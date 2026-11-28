//! Instrumented session lifecycle; original shutdown results survive cleanup retries.
use super::*;
use futures_util::FutureExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn webkit_headed_socks_failure_protocol_diagnostic() {
    let cache = std::path::PathBuf::from(
        std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select the private browser cache"),
    );
    let root = tempfile::Builder::new()
        .prefix("tr-socks-protocol-")
        .tempdir_in("/private/tmp")
        .unwrap()
        .keep();
    let mut failures = vec![];
    for attempt in 0..3 {
        let origin = crate::browser::socks_fixture::Site::start(
            crate::browser::socks_fixture::Mode::BrowserForward,
            false,
        )
        .await;
        let directory = root.join(attempt.to_string());
        crate::artifacts::directory(&directory).unwrap();
        // Match the paired installed probe's endpoint history before comparing
        // its driver path with the previously passing cold session probes.
        let sdk = tokio::process::Command::new(
            cache.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
        )
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/browser-socks-negative-probe.cjs"),
        )
        .arg(cache.join("playwright-core-1.64.1/package"))
        .arg(origin.server("socks4", ""))
        .arg(origin.url("http", "127.0.0.1", "/Acme-page"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("PLAYWRIGHT_BROWSERS_PATH", cache.join("browsers"))
        .output()
        .await
        .unwrap();
        std::fs::write(directory.join("sdk-warmup.json"),serde_json::to_vec_pretty(&json!({"success":sdk.status.success(),"stdout":String::from_utf8_lossy(&sdk.stdout),"stderr":String::from_utf8_lossy(&sdk.stderr),"facts":origin.facts(),"protocolPrefixes":origin.protocol_prefixes()})).unwrap()).unwrap();
        assert!(
            sdk.status.success(),
            "{}",
            String::from_utf8_lossy(&sdk.stderr)
        );
        let reference: Value = serde_json::from_slice(&sdk.stdout).unwrap();
        assert_eq!(reference["ownedProcesses"], 0, "{reference}");
        assert!(
            reference["navigation"]["message"]
                .as_str()
                .unwrap()
                .contains("The network connection was lost"),
            "{reference}"
        );
        let bundle = DiagnosticBundle::inspect(
            &cache.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
            &cache.join("playwright-core-1.64.1/package"),
            &cache.join("browsers"),
            Instant::now() + Duration::from_secs(30),
        )
        .await
        .unwrap()
        .observe_children(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/browser-protocol-observations.cjs"),
            directory.join("protocol.jsonl"),
        );
        let session=Session::launch(bundle,Uuid::new_v4(),Uuid::new_v4(),LaunchOptions{media_fixtures:Default::default(),engine:Engine::Webkit,mode:Mode::Headed,
            proxy:Some(serde_json::from_value(json!({"server":format!("{}/ignored?Acme=1",origin.server("socks4","AcmeIgnored:example@"))})).unwrap()),
            context:serde_json::from_value(json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"baseURL":origin.url("http","127.0.0.1",""),"extraHTTPHeaders":[{"name":"X-Acme","value":"browser"}]})).unwrap()},&directory).unwrap();
        let exercise=std::panic::AssertUnwindSafe(async {
            let startup=session.wait(session.startup(),Instant::now()+Duration::from_secs(90)).await.unwrap();
            assert_eq!(startup.outcome,Outcome::Succeeded,"{startup:?}");
            let request=serde_json::from_value(json!({"operation":"page_navigate","page":startup.result.as_ref().unwrap()["page"],"navigation":{"kind":"goto","url":origin.url("http","127.0.0.1","/Acme-page"),"waitUntil":"load"}})).unwrap();
            let navigation=session.wait(session.submit(Uuid::new_v4(),request,30000).unwrap(),Instant::now()+Duration::from_secs(40)).await.unwrap();
            let connection=session.0.host.connection().unwrap().status();
            std::fs::write(directory.join("navigation.json"),serde_json::to_vec_pretty(&json!({"operation":navigation,"status":session.status(),"privateFailures":session.private_failures(),"connection":format!("{connection:?}"),"facts":origin.facts(),"protocolPrefixes":origin.protocol_prefixes()})).unwrap()).unwrap();
            assert_eq!(navigation.outcome,Outcome::Failed,"{navigation:?}");
            assert_eq!(navigation.code.as_deref(),Some("BROWSER_METHOD_FAILED"),"{navigation:?}");
        }).catch_unwind().await;
        let original = session
            .wait(
                session.disconnect(30000).unwrap(),
                Instant::now() + Duration::from_secs(40),
            )
            .await
            .unwrap();
        std::fs::write(
            directory.join("original-close.json"),
            serde_json::to_vec_pretty(&json!({"operation":original,"status":session.status()}))
                .unwrap(),
        )
        .unwrap();
        if exercise.is_err() {
            failures.push(format!("navigation {attempt}"));
        }
        if original.outcome != Outcome::Succeeded {
            failures.push(format!("cleanup {attempt}"));
            loop {
                let retry = session
                    .wait(
                        session.disconnect(30000).unwrap(),
                        Instant::now() + Duration::from_secs(40),
                    )
                    .await
                    .unwrap();
                std::fs::write(
                    directory.join("cleanup-retry.json"),
                    serde_json::to_vec_pretty(
                        &json!({"operation":retry,"status":session.status()}),
                    )
                    .unwrap(),
                )
                .unwrap();
                if retry.outcome == Outcome::Succeeded {
                    break;
                }
                eprintln!(
                    "Retaining diagnostic cleanup owner: {}",
                    directory.display()
                );
            }
        }
        origin
            .settled(origin.facts()["connections"].as_array().unwrap().len())
            .await;
    }
    eprintln!("WebKit SOCKS protocol diagnostic: {}", root.display());
    assert!(
        failures.is_empty(),
        "Original failures: {failures:?}; evidence {}",
        root.display()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn firefox_headed_session_child_exit_and_stdio_close_diagnostic() {
    let cache = std::path::PathBuf::from(
        std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select the private browser cache"),
    );
    let root = tempfile::Builder::new()
        .prefix("tr-session-close-")
        .tempdir_in("/private/tmp")
        .unwrap()
        .keep();
    let mut failures = vec![];
    for attempt in 0..3 {
        let directory = root.join(attempt.to_string());
        crate::artifacts::directory(&directory).unwrap();
        let bundle = DiagnosticBundle::inspect(
            &cache.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
            &cache.join("playwright-core-1.64.1/package"),
            &cache.join("browsers"),
            Instant::now() + Duration::from_secs(30),
        )
        .await
        .unwrap()
        .observe_children(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/browser-child-observations.cjs"),
            directory.join("child-events.jsonl"),
        );
        let session = Session::launch(
            bundle,
            Uuid::new_v4(),
            Uuid::new_v4(),
            LaunchOptions {
                media_fixtures: Default::default(),
                engine: Engine::Firefox,
                mode: Mode::Headed,
                proxy: None,
                context: Default::default(),
            },
            &directory,
        )
        .unwrap();
        let exercise = std::panic::AssertUnwindSafe(async {
            let startup = session
                .wait(session.startup(), Instant::now() + Duration::from_secs(90))
                .await
                .unwrap();
            assert_eq!(startup.outcome, Outcome::Succeeded, "{startup:?}");
            tokio::time::sleep(Duration::from_secs(4)).await;
        })
        .catch_unwind()
        .await;
        let original = session
            .wait(
                session.disconnect(30000).unwrap(),
                Instant::now() + Duration::from_secs(40),
            )
            .await
            .unwrap();
        std::fs::write(
            directory.join("original.json"),
            serde_json::to_vec_pretty(&json!({"operation":original,"status":session.status()}))
                .unwrap(),
        )
        .unwrap();
        if original.outcome != Outcome::Succeeded {
            failures.push(attempt);
            loop {
                let retry = session
                    .wait(
                        session.disconnect(30000).unwrap(),
                        Instant::now() + Duration::from_secs(40),
                    )
                    .await
                    .unwrap();
                std::fs::write(
                    directory.join("cleanup-retry.json"),
                    serde_json::to_vec_pretty(
                        &json!({"operation":retry,"status":session.status()}),
                    )
                    .unwrap(),
                )
                .unwrap();
                if retry.outcome == Outcome::Succeeded {
                    break;
                }
                eprintln!(
                    "Retaining session cleanup owner; evidence {}",
                    directory.display()
                );
            }
        }
        if let Err(panic) = exercise {
            std::panic::resume_unwind(panic);
        }
    }
    eprintln!(
        "Instrumented session child/stdio evidence: {}",
        root.display()
    );
    assert!(
        failures.is_empty(),
        "Original shutdown failures {failures:?}; evidence {}",
        root.display()
    );
}
