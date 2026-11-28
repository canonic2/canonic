//! Closed native caller capacity remains a separate contract from scoped errors.
use super::*;

pub(super) async fn reference(fixture: &Fixture, mode: &str) -> Value {
    reference_kind(fixture, mode, false).await
}

pub(super) async fn page_reference(fixture: &Fixture, mode: &str) -> Value {
    reference_kind(fixture, mode, true).await
}

async fn reference_kind(fixture: &Fixture, mode: &str, page_only: bool) -> Value {
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(if page_only {
            "tests/fixtures/browser-page-close-capacity-baseline.cjs"
        } else {
            "tests/fixtures/browser-closed-call-capacity-baseline.cjs"
        }),
    )
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
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
                "{}-reference-{mode}.{extension}",
                if page_only {
                    "page-close"
                } else {
                    "closed-call"
                }
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
    assert_eq!(report["wire"], 0, "{report}");
    assert_eq!(report["sibling"], 42, "{report}");
    assert_eq!(report["cycles"], 65);
    assert_eq!(report["closed"].as_array().unwrap().len(), 65);
    for row in report["closed"].as_array().unwrap() {
        assert_eq!(row["pending"]["ok"], false, "{row}");
        assert!(
            row["pending"]["message"]
                .as_str()
                .unwrap()
                .contains("Target page, context or browser has been closed"),
            "{row}"
        );
        assert_eq!(
            row["workerCloseEvents"],
            if page_only { 0 } else { 1 },
            "{row}"
        );
        if page_only {
            assert_eq!(row["pageCloseEvents"], 1, "{row}");
            assert_eq!(row["sibling"], 42, "{row}");
            assert_eq!(row["worker"], 42, "{row}");
        }
        assert_eq!(row["lateReply"]["ok"], true, "{row}");
    }
    report
}

#[tokio::test]
async fn pinned_closed_call_capacity_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        reference(&fixture, mode).await;
    }
}

#[tokio::test]
async fn pinned_page_close_capacity_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        page_reference(&fixture, mode).await;
    }
}
