//! Measure pinned worker context defaults before changing native propagation.
use super::*;

pub(super) async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-service-worker-defaults-baseline.cjs"
    ))
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
    for (extension, bytes) in [("json", &output.stdout), ("stderr", &output.stderr)] {
        std::fs::write(
            fixture.workspace.path().join(format!(
                "worker-defaults-reference-{engine}-{mode}.{extension}"
            )),
            bytes,
        )
        .unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ownedProcesses"], 0, "{report}");
    for row in report["cases"].as_array().unwrap() {
        assert_eq!(row["registration"], true, "{row}");
        if engine == "chrome" {
            assert_eq!(row["stage"], "complete", "{row}");
            for key in ["initial", "changed"] {
                assert_eq!(row[key]["status"], 200, "{row}");
            }
            if row["name"] == "headers" {
                for (key, value) in [
                    ("initial", json!("initial")),
                    ("changed", json!("changed")),
                    ("cleared", Value::Null),
                ] {
                    let body: Value =
                        serde_json::from_str(row[key]["text"].as_str().unwrap()).unwrap();
                    assert_eq!(body["context"], value, "{row}");
                    assert!(body["page"].is_null(), "{row}");
                }
            } else {
                for key in ["initial", "changed"] {
                    let body: Value =
                        serde_json::from_str(row[key]["text"].as_str().unwrap()).unwrap();
                    assert_eq!(body["authorized"], true, "{row}");
                    assert!(body["page"].is_null(), "{row}");
                }
                assert_eq!(row["cleared"]["status"], 401, "{row}");
                assert_eq!(row["cleared"]["text"], "Acme unauthorized", "{row}");
            }
        } else {
            assert_eq!(row["stage"], "no-sdk-worker-owner", "{row}");
        }
    }
    report
}

#[tokio::test]
async fn pinned_service_worker_defaults_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            reference(&fixture, engine, mode).await;
            eprintln!(
                "Pinned worker defaults {engine}/{mode}: headers and credential lifecycle reference"
            );
        }
    }
}
