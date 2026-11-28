//! Pinned SDK ownership and fallback contracts before native HAR integration.
use super::*;

pub(super) async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-service-worker-har-baseline.cjs"
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
            fixture
                .workspace
                .path()
                .join(format!("worker-har-reference-{engine}-{mode}.{extension}")),
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
    assert_eq!(report["cases"].as_array().unwrap().len(), 2);
    for row in report["cases"].as_array().unwrap() {
        assert_eq!(row["registration"], true, "{row}");
        assert_eq!(row["pageEvents"], json!([]), "{row}");
        if engine == "chrome" {
            assert_eq!(row["stage"], "complete", "{row}");
            assert_eq!(row["workerCount"], 1, "{row}");
            assert_eq!(
                row["hit"],
                json!({"ok":true,"value":"Acme archive"}),
                "{row}"
            );
            assert_eq!(
                row["afterRemoval"],
                json!({"ok":true,"value":"Acme wire /wire/removed"}),
                "{row}"
            );
        } else {
            assert_eq!(row["stage"], "no-sdk-worker-owner", "{row}");
            assert_eq!(row["workerCount"], 0, "{row}");
        }
    }
    if engine == "chrome" {
        let automatic = &report["cases"][0];
        assert_eq!(automatic["miss"]["ok"], false, "{automatic}");
        assert_eq!(automatic["hitAgain"], automatic["hit"]);
        assert_eq!(
            automatic["fallback"],
            json!({"ok":true,"value":"Acme wire /wire/fallback"})
        );
        assert_eq!(automatic["filtered"]["ok"], true);
        assert_eq!(
            report["cases"][1]["miss"],
            json!({"ok":true,"value":"Acme oldest"})
        );
        assert_eq!(
            report["cases"][1]["events"],
            json!([
                {"handler":"newest","path":"/wire/hit","worker":true},
                {"handler":"newest","path":"/wire/miss","worker":true},
                {"handler":"oldest","path":"/wire/miss","worker":true}
            ])
        );
        assert_eq!(
            report["wire"],
            json!(["/wire/fallback", "/wire/removed", "/wire/removed"])
        );
    } else {
        assert_eq!(report["wire"], json!([]));
    }
    report
}

#[tokio::test]
async fn pinned_service_worker_har_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            reference(&fixture, engine, mode).await;
            eprintln!(
                "Pinned worker HAR {engine}/{mode}: context replay and ordered fallback reference"
            );
        }
    }
}
