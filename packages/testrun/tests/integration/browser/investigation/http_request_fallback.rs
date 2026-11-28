//! Pinned Request getters/API copies retain routing intent after source closure.
use super::*;
mod installed;
use super::wire_fixture as wire;
async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let output = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/browser-http-request-fallback-baseline.cjs"
    ))
    .arg(fixture.cache.join("playwright-core-1.64.1/package"))
    .arg(engine)
    .arg(mode)
    .arg(fixture.cache.join("browsers"))
    .current_dir(fixture.workspace.path())
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("PW_LANG_NAME", "javascript")
    .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
    .output()
    .await
    .unwrap();
    for (suffix, bytes) in [("json", &output.stdout), ("stderr", &output.stderr)] {
        std::fs::write(
            fixture.workspace.path().join(format!(
                "http-request-fallback-sdk-{engine}-{mode}.{suffix}"
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
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ownedProcesses"], 0);
    assert!(value.get("failure").is_none(), "{value}");
    assert_eq!(value["cases"].as_object().unwrap().len(), 4);
    for kind in ["fallback", "continue", "empty", "layered"] {
        let case = &value["cases"][kind];
        assert_eq!(case["copy"]["method"], case["source"]["method"]);
        assert_eq!(case["copy"]["bodyBase64"], case["source"]["bodyBase64"]);
    }
    value
}
#[tokio::test]
async fn pinned_http_request_fallback_reference_checkpoint() {
    let fixture = installed_fixture();
    for engine in ["chrome", "firefox", "webkit"] {
        for mode in ["headless", "headed"] {
            reference(&fixture, engine, mode).await;
            eprintln!(
                "Request routing intent {engine}/{mode}: four source-closure cases, ownedProcesses=0"
            );
        }
    }
}
