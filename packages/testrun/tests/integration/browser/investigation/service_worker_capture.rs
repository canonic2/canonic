//! Pinned context/page HAR ownership and worker replay/default coexistence.
use super::*;
use base64::Engine;

pub(super) fn body(entry: &Value) -> String {
    let text = entry["body"].as_str().expect("Captured body");
    if entry["encoding"] == "base64" {
        String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(text)
                .unwrap(),
        )
        .unwrap()
    } else {
        text.to_owned()
    }
}

pub(super) async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-service-worker-capture-baseline.cjs"
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
                "worker-capture-reference-{engine}-{mode}.{extension}"
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
    assert_eq!(report["cases"].as_array().unwrap().len(), 4);
    for row in report["cases"].as_array().unwrap() {
        assert_eq!(row["registration"], true, "{row}");
        if engine != "chrome" {
            assert_eq!(row["stage"], "no-sdk-worker-owner", "{row}");
            continue;
        }
        assert_eq!(row["stage"], "complete", "{row}");
        let entries = row["entries"].as_array().unwrap();
        if row["name"] == "page-excluded" {
            assert!(entries.is_empty(), "{row}");
            continue;
        }
        assert_eq!(
            entries.len(),
            if row["name"] == "coexistence" { 2 } else { 1 },
            "{row}"
        );
        for entry in entries {
            assert!(entry["pageref"].is_null(), "{entry}");
            assert!(!entry["workerRef"].as_str().unwrap().is_empty(), "{entry}");
            assert_eq!(entry["status"], 200, "{entry}");
            assert_eq!(entry["contextHeader"], "Acme capture", "{entry}");
        }
        if row["name"] == "coexistence" {
            assert_eq!(row["replayed"], "Acme archive");
            assert_eq!(row["authenticated"], "Acme authenticated");
            assert_eq!(entries[0]["workerRef"], entries[1]["workerRef"]);
            assert_eq!(body(&entries[0]), "Acme archive");
            assert_eq!(body(&entries[1]), "Acme authenticated");
        } else {
            assert_eq!(row["fetched"], "Acme wire /wire/captured");
            assert_eq!(body(&entries[0]), "Acme wire /wire/captured");
        }
    }
    if engine == "chrome" {
        assert_eq!(
            report["wire"],
            json!([
                {"path":"/wire/captured","context":"Acme capture","authorizationPresent":false,"authorized":false},
                {"path":"/wire/captured","context":"Acme capture","authorizationPresent":false,"authorized":false},
                {"path":"/wire/captured","context":"Acme capture","authorizationPresent":false,"authorized":false},
                {"path":"/wire/auth-first","context":"Acme capture","authorizationPresent":false,"authorized":false},
                {"path":"/wire/auth-first","context":"Acme capture","authorizationPresent":true,"authorized":true},
            ])
        );
    } else {
        assert_eq!(report["wire"], json!([]));
    }
    report
}

#[tokio::test]
async fn pinned_service_worker_capture_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            reference(&fixture, engine, mode).await;
            eprintln!(
                "Pinned worker capture {engine}/{mode}: source ownership, page exclusion and replay/default coexistence"
            );
        }
    }
}
