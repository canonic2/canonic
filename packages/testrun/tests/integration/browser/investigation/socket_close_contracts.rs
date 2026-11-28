//! Direct SDK close defaults and arrival-owned close-handler removal.
use super::*;

#[tokio::test]
async fn pinned_websocket_close_contracts_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let report = reference(&fixture, engine, mode).await;
            assert_eq!(report["ownedProcesses"], 0, "{report}");
            let cases = report["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 11, "{report}");
            for row in &cases[..8] {
                let name = row["name"].as_str().unwrap();
                let calls = row["nativeCalls"].as_array().unwrap();
                assert_eq!(calls.len(), 1, "{row}");
                if engine == "webkit"
                    && name.ends_with("-reason")
                    && !name.ends_with("-empty-reason")
                {
                    assert_eq!(row["wireErrors"].as_array().unwrap().len(), 1, "{row}");
                    assert_eq!(
                        row["wireErrors"][0]["code"], "WS_ERR_INVALID_CLOSE_CODE",
                        "{row}"
                    );
                    assert!(
                        row["wireErrors"][0]["message"]
                            .as_str()
                            .unwrap()
                            .contains("16739"),
                        "{row}"
                    );
                } else {
                    assert_eq!(row["wireErrors"], json!([]), "{row}");
                }
                assert_eq!(
                    calls[0]["arguments"][0],
                    json!({"kind":"undefined"}),
                    "{row}"
                );
                assert_eq!(
                    calls[0]["arguments"][1],
                    if name.ends_with("-reason") {
                        json!({"kind":"string","value":if name.ends_with("-empty-reason"){""}else{"Acme reason"}})
                    } else {
                        json!({"kind":"undefined"})
                    },
                    "{row}"
                );
                if !name.ends_with("-reason") {
                    assert!(
                        calls[0]["error"].is_null(),
                        "Omitted reason preserves the SDK's empty native close: {row}"
                    );
                    assert_eq!(
                        row["serverClosed"],
                        json!({"code":1005,"reason":""}),
                        "{row}"
                    );
                } else if engine == "chrome" {
                    assert_eq!(
                        calls[0]["error"], "InvalidAccessError",
                        "Explicit reason with undefined code retains the pinned native refusal: {row}"
                    );
                }
                if calls[0]["error"].is_null() {
                    assert_eq!(row["pageReady"], 3, "{row}");
                    assert_eq!(row["serverReady"], 3, "{row}");
                    assert!(row["serverClosed"].is_object(), "{row}");
                } else {
                    assert_eq!(row["serverReady"], 1, "{row}");
                    assert!(row["serverClosed"].is_null(), "{row}");
                    assert_eq!(
                        row["pageReady"],
                        if name.starts_with("page-") { 3 } else { 1 },
                        "{row}"
                    );
                }
            }
            let removed = &cases[8];
            assert_eq!(removed["name"], "remove-page");
            assert_eq!(removed["heldPage"], 2, "{removed}");
            assert_eq!(
                removed["events"],
                json!([{"event":"server-close","code":3001,"reason":"Acme page"}]),
                "{removed}"
            );
            assert_eq!(
                removed["serverClosed"],
                json!({"code":3001,"reason":"Acme page"})
            );
            assert_eq!(
                removed["page"].as_array().unwrap().last().unwrap(),
                &json!({"type":"close","code":3002,"reason":"Acme final","wasClean":true})
            );
            let removed = &cases[9];
            assert_eq!(removed["name"], "remove-server");
            assert_eq!(
                removed["events"],
                json!([{"event":"page-close","code":3001,"reason":"Acme server"}]),
                "{removed}"
            );
            assert_eq!(removed["pageReady"], 3, "{removed}");
            assert_eq!(removed["serverReady"], 3, "{removed}");
            let pending = &cases[10];
            assert_eq!(pending["name"], "pending-server-removal");
            assert_eq!(pending["heldPage"], 1, "{pending}");
            assert_eq!(pending["afterRemoval"], 1, "{pending}");
            assert_eq!(
                pending["events"],
                json!([{"event":"server-close","code":3001,"reason":"Acme server"},{"event":"resolved"}]),
                "{pending}"
            );
            assert_eq!(
                pending["page"],
                json!([{"type":"text","data":"Acme-open"},{"type":"text","data":"Acme survivor"},{"type":"close","code":3003,"reason":"Acme final","wasClean":true}]),
                "{pending}"
            );
            eprintln!(
                "Pinned close defaults/removal {engine}/{mode}: eleven workflows, original native close arguments and held callback ownership"
            );
        }
    }
}

pub(super) async fn reference(fixture: &Fixture, engine: &str, mode: &str) -> Value {
    let result = Command::new(
        fixture
            .cache
            .join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"),
    )
    .arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-socket-close-baseline.cjs"),
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
                "socket-close-reference-{engine}-{mode}.{extension}"
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
    serde_json::from_slice(&result.stdout).unwrap()
}
