//! Measure actual native body availability before designing owned Request inputs.
use super::*;

#[tokio::test]
async fn pinned_native_http_owned_request_body_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        let output = Command::new(
            fixture
                .cache
                .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
        )
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/browser-owned-request-input-probe.cjs"
        ))
        .arg(fixture.cache.join("playwright-core-1.64.1/package"))
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
                fixture
                    .workspace
                    .path()
                    .join(format!("owned-request-body-sdk-{mode}.{suffix}")),
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
        assert!(value.get("failure").is_none(), "{value}");
        assert_eq!(value["ownedProcesses"], 0, "{value}");
        assert_eq!(value["cases"].as_array().unwrap().len(), 9, "{value}");
        eprintln!("Owned Request native body {mode}: {value}");
    }
}
