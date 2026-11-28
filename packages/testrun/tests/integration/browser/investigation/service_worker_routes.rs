//! Measure context-owned service-worker routing before native implementation.
use super::*;
mod installed;

#[tokio::test]
async fn pinned_service_worker_route_ownership_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let report = reference(&fixture, engine, mode).await;
            assert_eq!(report["ownedProcesses"], 0, "{report}");
            assert_eq!(report["cases"].as_array().unwrap().len(), 2, "{report}");
            if engine == "chrome" {
                for row in report["cases"].as_array().unwrap() {
                    assert_eq!(row["stage"], "complete", "{row}");
                    assert_eq!(row["registration"]["controlled"], true, "{row}");
                    assert_eq!(row["pageEvents"], json!([]), "{row}");
                    assert_eq!(row["first"], "Acme first", "{row}");
                    assert_eq!(row["afterPageClose"], "Acme first", "{row}");
                    for event in row["events"].as_array().unwrap() {
                        assert_eq!(event["worker"], true, "{event}");
                        assert_eq!(event["sameWorker"], true, "{event}");
                        assert!(event["frame"].is_null(), "{event}");
                        assert!(event["frameError"].is_object(), "{event}");
                        assert_eq!(
                            event["frameError"]["message"],
                            "Service Worker requests do not have an associated frame."
                        );
                    }
                    assert_eq!(row["workerClosedAfterContext"], 1, "{row}");
                    assert!(
                        row["handlerResults"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|reply| reply["ok"] == true),
                        "{row}"
                    );
                }
                let owned = &report["cases"][0];
                assert_eq!(owned["replaced"], "Acme second");
                assert_eq!(owned["restored"], "Acme first");
                assert_eq!(owned["removalWaited"], true);
                assert_eq!(owned["held"], json!({"ok":true,"value":"Acme first"}));
                assert_eq!(owned["afterRemoval"], "Acme wire /wire/after-removal");
                assert_eq!(owned["workerClosedBeforeContext"], 0);
                assert_eq!(
                    owned["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|event| json!([event["handler"], event["path"]]))
                        .collect::<Vec<_>>(),
                    vec![
                        json!(["first", "/wire/first"]),
                        json!(["first", "/wire/after-page-close"]),
                        json!(["second", "/wire/replaced"]),
                        json!(["first", "/wire/restored"]),
                        json!(["first", "/wire/held"])
                    ]
                );
                let closing = &report["cases"][1];
                assert_eq!(closing["pending"]["ok"], false);
                assert!(
                    closing["pending"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("Target page, context or browser has been closed")
                );
                assert_eq!(
                    closing["handlerResults"].as_array().unwrap().len(),
                    3,
                    "A retained SDK route reply after context closure completes as a no-op"
                );
                assert_eq!(report["wire"], json!(["/wire/after-removal"]));
            } else {
                for row in report["cases"].as_array().unwrap() {
                    assert_eq!(row["stage"], "no-sdk-worker-owner", "{row}");
                    assert_eq!(
                        row["registration"],
                        json!({"available":true,"controlled":true}),
                        "{row}"
                    );
                    assert_eq!(row["workerCount"], 0);
                    assert_eq!(row["first"], "Acme wire /wire/first");
                    assert_eq!(row["events"], json!([]));
                    assert_eq!(row["pageEvents"], json!([]));
                }
                assert_eq!(report["wire"], json!(["/wire/first", "/wire/first"]));
            }
            eprintln!(
                "Pinned service-worker routes {engine}/{mode}: context ownership and policy/target lifetime reference"
            );
        }
    }
}

async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-service-worker-route-baseline.cjs"),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .output()
    .await
    .unwrap();
    for (extension, bytes) in [("json", &result.stdout), ("stderr", &result.stderr)] {
        std::fs::write(
            fixture.workspace.path().join(format!(
                "worker-route-reference-{engine}-{mode}.{extension}"
            )),
            bytes,
        )
        .unwrap();
    }
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0, "{report}");
    report
}
