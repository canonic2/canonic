//! Exact SDK forwarding, single-handler replacement and independent close owners.
use super::*;
mod installed;

#[tokio::test]
async fn pinned_websocket_sdk_reference_checkpoint() {
    let fixture = installed_fixture();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let report = reference(&fixture, engine, mode).await;
            assert_eq!(report["ownedProcesses"], 0, "{report}");
            let cases = report["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 6, "{report}");
            for row in cases {
                let name = row["name"].as_str().unwrap();
                if name == "server-only" {
                    assert_eq!(row["readyAfterDiscardedSend"], 1, "{row}");
                    assert_eq!(row["server"], json!([]), "{row}");
                    assert_eq!(
                        row["events"],
                        json!([{"event":"server-close","code":3002,"reason":"Acme server"}]),
                        "{row}"
                    );
                    assert_eq!(
                        row["page"],
                        json!([{"type":"text","data":"Acme-open"},{"type":"text","data":"Acme survivor"},{"type":"close","code":3003,"reason":"Acme final","wasClean":true}]),
                        "{row}"
                    );
                    assert_eq!(
                        row["serverClosed"],
                        json!({"code":3002,"reason":"Acme server"}),
                        "{row}"
                    );
                    assert_eq!(row["connected"], true, "{row}");
                    continue;
                }
                if name == "close" {
                    assert_eq!(row["heldServerOpen"], true, "{row}");
                    assert_eq!(row["heldPageClosing"], true, "{row}");
                    assert_eq!(
                        row["events"],
                        json!([{"event":"page-close","code":3001,"reason":"Acme page"},{"event":"server-close","code":3002,"reason":"Acme server"},{"event":"page-close","code":3003,"reason":"Acme final"}]),
                        "{row}"
                    );
                    assert_eq!(
                        row["serverClosed"],
                        json!({"code":3002,"reason":"Acme server"}),
                        "{row}"
                    );
                    assert_eq!(
                        row["page"].as_array().unwrap().last().unwrap(),
                        &json!({"type":"close","code":3003,"reason":"Acme final","wasClean":true}),
                        "{row}"
                    );
                    continue;
                }
                assert_eq!(row["connected"], name != "mock", "{row}");
                let mut page = vec![];
                let mut server = vec![];
                if name != "mock" {
                    page.extend([
                        json!({"type":"text","data":"Acme-open"}),
                        json!({"type":"text","data":"Acme-first"}),
                        json!({"type":"binary","data":[0,17,255]}),
                    ]);
                    server.extend([
                        json!({"isBinary":false,"data":"Acme-first"}),
                        json!({"isBinary":true,"data":[0,17,255]}),
                    ]);
                }
                if name == "replace" {
                    page.push(json!({"type":"text","data":"Acme-default-restored"}));
                    server.push(json!({"isBinary":false,"data":"Acme-default-restored"}));
                    let events = row["events"].as_array().unwrap();
                    assert_eq!(events.len(), 5, "{row}");
                    assert_eq!(
                        events
                            .iter()
                            .filter(|event| **event == "page-message")
                            .count(),
                        2,
                        "{row}"
                    );
                    assert_eq!(
                        events
                            .iter()
                            .filter(|event| **event == "server-message")
                            .count(),
                        3,
                        "{row}"
                    );
                } else {
                    assert_eq!(row["events"], json!([]), "{row}");
                }
                let code = if name == "close-default" { 3004 } else { 3001 };
                let reason = if name == "close-default" {
                    "Acme route"
                } else {
                    "Acme page"
                };
                page.push(json!({"type":"close","code":code,"reason":reason,"wasClean":true}));
                assert_eq!(row["page"], json!(page), "{row}");
                assert_eq!(row["server"], json!(server), "{row}");
                assert_eq!(
                    row["serverClosed"],
                    if name == "mock" {
                        Value::Null
                    } else {
                        json!({"code":code,"reason":reason})
                    },
                    "{row}"
                );
            }
            eprintln!(
                "Pinned socket SDK {engine}/{mode}: relay/mock defaults, replaced/removed message handlers and independent close owners"
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
            .join("tests/fixtures/browser-socket-sdk-baseline.cjs"),
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
            fixture
                .workspace
                .path()
                .join(format!("socket-sdk-{engine}-{mode}.{extension}")),
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
