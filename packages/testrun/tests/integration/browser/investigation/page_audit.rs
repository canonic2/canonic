//! Installed `page_audit` through the CLI and MCP: Lighthouse navigation and
//! snapshot audits in Chrome headless and headed, private JSON/HTML reports,
//! option bounds, and the Firefox/WebKit refusal.
use super::*;

const AUDIT: &str = "<!doctype html><html lang=en><head><title>Acme audit</title></head><body><h1>Acme audit</h1><img width=24 height=24 src='data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw=='><button></button></body></html>";

/// Serves the audit page, with failing image-alt and button-name audits, at `/audit`.
async fn server() -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let http = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let origin = format!("http://{}", http.local_addr().unwrap());
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = http.accept().await.unwrap();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buffer = [0; 1024];
                while !head.windows(4).any(|part| part == b"\r\n\r\n") {
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    head.extend_from_slice(&buffer[..read]);
                }
                let response = if head.starts_with(b"GET /audit ") {
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{AUDIT}",
                        AUDIT.len()
                    )
                } else {
                    "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        .to_owned()
                };
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    (origin, task)
}

/// Waits up to 120 s for a submitted audit, which can outlast one status wait.
async fn audited(mcp: &mut Mcp, target: &str, submitted: &Value) -> Value {
    for _ in 0..4 {
        let status = mcp.tool("browser_status", json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000})).await;
        if status["operation"]["settled"] == true {
            assert_eq!(status["operation"]["outcome"], "succeeded", "{status}");
            return status["operation"]["result"].clone();
        }
    }
    panic!("Audit did not settle within 120 s: {submitted}");
}

/// Checks one audit result and its two private reports, then removes them.
async fn reports(mcp: &mut Mcp, target: &str, result: &Value) {
    let scores = result["scores"].as_array().unwrap();
    for category in ["accessibility", "agentic-browsing"] {
        assert!(
            scores.iter().any(|score| score["id"] == category),
            "{category}: {result}"
        );
    }
    assert!(result["audits"]["failed"].as_u64().unwrap() > 0, "{result}");
    let reports = result["reports"].as_array().unwrap();
    assert_eq!(reports.len(), 2, "{result}");
    assert_eq!(reports[0]["format"], "lighthouse-result-json", "{result}");
    assert_eq!(reports[1]["format"], "lighthouse-report-html", "{result}");
    for report in reports {
        assert_eq!(report["privacy"], "private-audit-report", "{report}");
    }
    let lhr: Value = serde_json::from_slice(
        &std::fs::read(reports[0]["attachment"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert!(lhr["lighthouseVersion"].is_string(), "{lhr}");
    assert!(lhr["audits"]["image-alt"]["score"].as_f64().unwrap() < 1.0);
    let html = std::fs::read_to_string(reports[1]["attachment"]["path"].as_str().unwrap()).unwrap();
    assert!(html.contains("lighthouseVersion") || html.contains("LIGHTHOUSE_JSON"));
    for report in reports {
        operation(
            mcp,
            target,
            json!({"operation":"developer_evidence_remove","evidenceId":report["evidenceId"]}),
        )
        .await;
    }
}

async fn chrome(fixture: &Fixture, mcp: &mut Mcp, target: &str, context: &Value, origin: &str) {
    let page = operation(
        mcp,
        target,
        json!({"operation":"page_create","context":context,"select":true}),
    )
    .await["page"]
        .clone();
    operation(mcp,target,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"goto","url":format!("{origin}/audit"),"waitUntil":"load"}})).await;
    // CLI: navigation audit with failing node details.
    let request = json!({"operation":"page_audit","page":page,"options":{"mode":"navigation","device":"desktop","includeNodes":true}}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &request])
        .await;
    let navigation = audited(mcp, target, &submitted).await;
    reports(mcp, target, &navigation).await;
    // MCP: snapshot audit of the current document on the mobile profile.
    let submitted = mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"page_audit","page":page,"options":{"mode":"snapshot","device":"mobile","limit":5}}})).await;
    let snapshot = audited(mcp, target, &submitted).await;
    reports(mcp, target, &snapshot).await;
    let status = mcp.detailed_browser_status(target).await;
    assert!(
        status["browser"]["developers"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "An audit releases its own frontend: {status}"
    );
    collectors::refused(
        mcp,
        target,
        json!({"operation":"page_audit","page":page,"options":{"limit":0}}),
        "INVALID_ARGUMENT",
        false,
    )
    .await;
    operation(mcp, target, json!({"operation":"page_close","page":page})).await;
}

#[tokio::test]
async fn installed_step_four_page_audit_checkpoint() {
    let fixture = installed_fixture();
    let (origin, server) = server().await;
    let mut mcp = Mcp::start(&fixture).await;
    let cells = [
        ("chrome", "headless"),
        ("chrome", "headed"),
        ("firefox", "headless"),
        ("webkit", "headless"),
    ];
    for (engine, mode) in cells {
        let connected = fixture
            .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
            .await;
        let target = connected["targetId"].as_str().unwrap().to_owned();
        let ready = fixture.ready_target(&target).await;
        let context = ready["browser"]["selectedContext"].clone();
        let result = std::panic::AssertUnwindSafe(async {
            if engine == "chrome" {
                chrome(&fixture, &mut mcp, &target, &context, &origin).await;
            } else {
                let page = &ready["browser"]["selectedPage"];
                collectors::refused(
                    &mut mcp,
                    &target,
                    json!({"operation":"page_audit","page":page}),
                    "NATIVE_DEVELOPER_UNSUPPORTED",
                    true,
                )
                .await;
            }
        })
        .catch_unwind()
        .await;
        let closed = performance_lifecycle::disconnect_observed(
            &fixture, &mut mcp, &target, engine, mode, &context,
        )
        .await;
        if let Err(panic) = result {
            mcp.stop().await;
            server.abort();
            std::panic::resume_unwind(panic);
        }
        assert_eq!(
            closed["result"]["structuredContent"]["ok"], true,
            "{closed}"
        );
        eprintln!("Page audit {engine} {mode} passed");
    }
    mcp.stop().await;
    server.abort();
}
