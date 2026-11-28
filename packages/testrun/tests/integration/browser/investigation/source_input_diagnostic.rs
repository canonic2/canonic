//! Unchanged Firefox native-input diagnostic with test-only protocol observations.
use super::*;

#[tokio::test]
async fn pinned_source_run_to_native_input_diagnostic() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        let observation = fixture
            .workspace
            .path()
            .join(format!("source-input-{mode}-protocol.jsonl"));
        let output = Command::new(
            fixture
                .cache
                .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
        )
        .arg("--require")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/browser-protocol-observations.cjs"
        ))
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/browser-source-input-baseline.cjs"
        ))
        .arg(fixture.cache.join("playwright-core-1.64.1/package"))
        .arg(mode)
        .current_dir(fixture.workspace.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("PLAYWRIGHT_BROWSERS_PATH", fixture.cache.join("browsers"))
        .env("TESTRUN_CHILD_OBSERVATIONS", &observation)
        .env("TESTRUN_NATIVE_PROTOCOL_ONLY", "1")
        .output()
        .await
        .unwrap();
        for (extension, bytes) in [("json", &output.stdout), ("stderr", &output.stderr)] {
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("source-input-{mode}.{extension}")),
                bytes,
            )
            .unwrap();
        }
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["ownedProcesses"], 0, "{report}");
        let records = std::fs::read_to_string(&observation)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(
            records.len() < 2048,
            "Protocol observer capacity exhausted; this cannot certify a complete transcript"
        );
        assert!(
            !records.iter().any(|record| matches!(
                record["event"].as_str(),
                Some("observer-bounds" | "observer-parse-error")
            )),
            "Protocol decoding must remain complete"
        );
        let commands = records
            .iter()
            .filter(|row| {
                row["lane"] == "driver-to-browser"
                    && row["message"]["method"] == "Page.dispatchMouseEvent"
            })
            .collect::<Vec<_>>();
        assert!(
            !commands.is_empty(),
            "Native mouse commands must actually be observed"
        );
        for command in &commands {
            assert!(
                ["mousemove", "mousedown", "mouseup"]
                    .contains(&command["message"]["inputType"].as_str().unwrap())
            );
            assert!(
                records
                    .iter()
                    .any(|reply| reply["lane"] == "browser-to-driver"
                        && reply["message"]["id"] == command["message"]["id"]),
                "Native command lacks a correlated reply: {command}"
            );
        }
        assert!(
            output.status.success(),
            "SDK diagnostic failed: {report}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(report["stage"], "complete");
        assert_eq!(report["completedClicks"], 7);
        eprintln!(
            "Source/native input SDK firefox/{mode}: seven clicks, exact pause stages, {} native commands with observed replies and zero owned processes",
            commands.len()
        );
    }
}
