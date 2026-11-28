//! Focused public-client checkpoint; independent of the broad investigation journey.
use super::*;

pub(super) async fn respond(stream: &mut tokio::net::TcpStream, header: &[u8]) {
    use std::io::Write as _;
    let request = String::from_utf8_lossy(header);
    let coding = request
        .split_whitespace()
        .nth(1)
        .unwrap()
        .split_once('?')
        .unwrap()
        .1;
    if coding == "empty" {
        stream
            .write_all(
                b"HTTP/1.1 204 \r\nX-Acme: empty\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        return;
    }
    if coding == "keepalive" {
        for sequence in 1.. {
            stream.write_all(format!("HTTP/1.1 200 Acme keepalive {sequence}\r\nX-Acme: one\r\nX-Sequence: {sequence}\r\nx-acme: two\r\nContent-Length: 4\r\nConnection: keep-alive\r\n\r\nAcme").as_bytes()).await.unwrap();
            let mut next = Vec::new();
            while !next.ends_with(b"\r\n\r\n") {
                match stream.read_u8().await {
                    Ok(byte) => next.push(byte),
                    Err(_) => return,
                }
            }
            if next.starts_with(b"GET /http-encoded?empty ") {
                stream.write_all(b"HTTP/1.1 204 \r\nX-Acme: empty\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                return;
            }
            assert!(next.starts_with(b"GET /http-encoded?keepalive "));
        }
        unreachable!();
    }
    let (encoding, bytes) = match coding {
        "gzip" | "deflate" | "json" => {
            let payload: &[u8] = if coding == "json" {
                br#"{"name":"Acme private","values":[1,true,null]}"#
            } else {
                b"hello world"
            };
            let bytes = if coding != "deflate" {
                let mut writer =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                writer.write_all(payload).unwrap();
                writer.finish().unwrap()
            } else {
                let mut writer =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                writer.write_all(payload).unwrap();
                writer.finish().unwrap()
            };
            (if coding == "json" { "gzip" } else { coding }, bytes)
        }
        "br" => ("br", b"\x0b\x05\x80hello world\x03".to_vec()),
        "invalid" => ("gzip", vec![255; 32]),
        _ => panic!("Unexpected owned encoding fixture: {coding}"),
    };
    stream.write_all(format!("HTTP/1.1 200 Acme custom phrase\r\nContent-Encoding: {encoding}\r\nX-Acme: one\r\nSet-Cookie: acme=private; Path=/\r\nx-acme: two\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).as_bytes()).await.unwrap();
    let _ = stream.write_all(&bytes).await;
}

async fn decoded_responses(fixture: &Fixture, mcp: &mut Mcp, target: &str, site: &str) -> Value {
    let context = operation(
        mcp,
        target,
        json!({"operation":"http_context_create","options":{"baseUrl":site}}),
    )
    .await["httpContext"]
        .clone();
    for coding in ["gzip", "deflate", "br"] {
        let request = json!({"operation":"http_fetch","httpContext":context,"request":{"url":format!("/http-encoded?{coding}"),"readBody":true,"maxBytes":11}}).to_string();
        let submitted = fixture
            .cli(&["browser", "send", "--target", target, &request])
            .await;
        let response = completed(mcp, target, submitted).await;
        assert_eq!(response["contentEncoding"], coding);
        assert_eq!(response["bodyDecoded"], true);
        assert_eq!(response["bodyBytes"], 11);
        assert!(response["encodedBodyBytes"].as_u64().unwrap() > 11);
        let evidence = &response["body"];
        assert_eq!(response["ok"], true);
        let text=operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":evidence["evidenceId"],"format":"text"})).await;
        assert_eq!(text["value"], "hello world");
        assert_eq!(text["private"], true);
        let encoded=operation(mcp,target,json!({"operation":"http_evidence_read","evidenceId":evidence["evidenceId"],"format":"base64"})).await;
        assert_eq!(encoded["value"], "aGVsbG8gd29ybGQ=");
        let bytes = std::fs::read(evidence["attachment"]["path"].as_str().unwrap()).unwrap();
        assert_eq!(bytes, b"hello world");
        operation(
            mcp,
            target,
            json!({"operation":"http_evidence_remove","evidenceId":evidence["evidenceId"]}),
        )
        .await;
        let details = operation(
            mcp,
            target,
            json!({"operation":"http_response_read","requestId":response["requestId"]}),
        )
        .await;
        assert_eq!(details["url"], format!("{site}/http-encoded?{coding}"));
        operation(
            mcp,
            target,
            json!({"operation":"http_response_remove","requestId":response["requestId"]}),
        )
        .await;
    }
    for (read_body, code) in [(false, None), (true, Some("HTTP_BODY_DECODE"))] {
        let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_fetch","httpContext":context,"request":{"url":"/http-encoded?invalid","readBody":read_body,"maxRetries":2}}})).await;
        let status=mcp.tool("browser_status",json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000})).await;
        assert_eq!(status["operation"]["settled"], true, "{status}");
        assert_eq!(status["operation"]["code"].as_str(), code, "{status}");
        assert_eq!(status["operation"]["result"]["status"], 200, "{status}");
        assert_eq!(status["operation"]["result"]["retryCount"], 0, "{status}");
        if !read_body {
            let request_id = &status["operation"]["result"]["requestId"];
            let details = operation(
                mcp,
                target,
                json!({"operation":"http_response_read","requestId":request_id}),
            )
            .await;
            assert_eq!(details["status"], 200);
            operation(
                mcp,
                target,
                json!({"operation":"http_response_remove","requestId":request_id}),
            )
            .await;
        }
    }
    let response=operation(mcp,target,json!({"operation":"http_fetch","httpContext":context,"request":{"url":"/http-encoded?json","readBody":true}})).await;
    let evidence = &response["body"];
    assert!(!response.to_string().contains("acme=private"));
    assert!(!response.to_string().contains("Acme custom phrase"));
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":context}),
    )
    .await;
    let request_id = &response["requestId"];
    let read_details = json!({"operation":"http_response_read","requestId":request_id}).to_string();
    let fresh_started = std::time::Instant::now();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &read_details])
        .await;
    let details = completed(mcp, target, submitted).await;
    let fresh_ms = fresh_started.elapsed().as_secs_f64() * 1000.0;
    let retained_started = std::time::Instant::now();
    let retained = operation(
        mcp,
        target,
        json!({"operation":"http_response_read","requestId":request_id}),
    )
    .await;
    let retained_ms = retained_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(retained, details);
    assert_eq!(details["url"], format!("{site}/http-encoded?json"));
    assert_eq!(details["statusTextAvailable"], true);
    assert_eq!(details["statusText"], "Acme custom phrase");
    assert_eq!(details["wireMetadataAvailable"], true);
    assert_eq!(details["headerNames"], "original");
    assert_eq!(details["headerOrder"], "wire");
    let headers = details["headers"].as_array().unwrap();
    assert_eq!(
        headers
            .iter()
            .filter(|h| h["name"].as_str().unwrap().eq_ignore_ascii_case("x-acme"))
            .map(|h| h["value"].clone())
            .collect::<Vec<_>>(),
        vec![json!("one"), json!("two")]
    );
    assert!(
        headers
            .iter()
            .any(|h| h["name"] == "Set-Cookie" && h["value"] == "acme=private; Path=/")
    );
    assert_eq!(
        headers
            .iter()
            .map(|header| header["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "Content-Encoding",
            "X-Acme",
            "Set-Cookie",
            "x-acme",
            "Content-Length",
            "Connection"
        ]
    );
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_response_read","requestId":request_id,"maxBytes":8}})).await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["code"], "HTTP_RESPONSE_RESULT_BOUNDS");
    let removal_started = std::time::Instant::now();
    assert_eq!(
        operation(
            mcp,
            target,
            json!({"operation":"http_response_remove","requestId":request_id})
        )
        .await["removed"],
        true
    );
    let removal_ms = removal_started.elapsed().as_secs_f64() * 1000.0;
    let submitted=mcp.tool("browser_send",json!({"targetId":target,"request":{"operation":"http_response_read","requestId":request_id}})).await;
    let status = mcp
        .tool(
            "browser_status",
            json!({"targetId":target,"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await;
    assert_eq!(status["operation"]["code"], "UNKNOWN_HTTP_RESPONSE_DETAILS");
    let read=json!({"operation":"http_evidence_read","evidenceId":evidence["evidenceId"],"format":"json"}).to_string();
    let submitted = fixture
        .cli(&["browser", "send", "--target", target, &read])
        .await;
    assert_eq!(
        completed(mcp, target, submitted).await["value"],
        json!({"name":"Acme private","values":[1,true,null]})
    );
    operation(
        mcp,
        target,
        json!({"operation":"http_evidence_remove","evidenceId":evidence["evidenceId"]}),
    )
    .await;
    json!({"freshCliReadMs":fresh_ms,"retainedMcpReadMs":retained_ms,"retainedMcpRemovalMs":removal_ms})
}

async fn pooled_wire(fixture: &Fixture, mcp: &mut Mcp, target: &str, site: &str) -> Value {
    let context = operation(
        mcp,
        target,
        json!({"operation":"http_context_create","options":{"baseUrl":site}}),
    )
    .await["httpContext"]
        .clone();
    let request = json!({"operation":"http_fetch","httpContext":context,"request":{"url":"/http-encoded?keepalive","readBody":true,"maxBytes":4}});
    let mut timings = Vec::new();
    for sequence in 1..=2 {
        let started = std::time::Instant::now();
        let response = if sequence == 1 {
            let submitted = fixture
                .cli(&["browser", "send", "--target", target, &request.to_string()])
                .await;
            completed(mcp, target, submitted).await
        } else {
            operation(mcp, target, request.clone()).await
        };
        timings.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(response["retryCount"], 0);
        assert_eq!(response["replayed"], false);
        let details = operation(
            mcp,
            target,
            json!({"operation":"http_response_read","requestId":response["requestId"]}),
        )
        .await;
        assert_eq!(details["wireMetadataAvailable"], true);
        assert_eq!(details["statusText"], format!("Acme keepalive {sequence}"));
        assert_eq!(
            details["headers"][0],
            json!({"name":"X-Acme","value":"one"})
        );
        assert_eq!(
            details["headers"][1],
            json!({"name":"X-Sequence","value":sequence.to_string()})
        );
        assert_eq!(
            details["headers"][2],
            json!({"name":"x-acme","value":"two"})
        );
        operation(
            mcp,
            target,
            json!({"operation":"http_evidence_remove","evidenceId":response["body"]["evidenceId"]}),
        )
        .await;
        assert_eq!(
            operation(
                mcp,
                target,
                json!({"operation":"http_response_read","requestId":response["requestId"]})
            )
            .await,
            details
        );
        operation(
            mcp,
            target,
            json!({"operation":"http_response_remove","requestId":response["requestId"]}),
        )
        .await;
    }
    let response=operation(mcp,target,json!({"operation":"http_fetch","httpContext":context,"request":{"url":"/http-encoded?empty","readBody":false}})).await;
    let details = operation(
        mcp,
        target,
        json!({"operation":"http_response_read","requestId":response["requestId"]}),
    )
    .await;
    assert_eq!(details["status"], 204);
    assert_eq!(details["statusTextAvailable"], true);
    assert_eq!(details["statusText"], "");
    assert_eq!(details["wireMetadataAvailable"], true);
    operation(
        mcp,
        target,
        json!({"operation":"http_response_remove","requestId":response["requestId"]}),
    )
    .await;
    operation(
        mcp,
        target,
        json!({"operation":"http_context_remove","httpContext":context}),
    )
    .await;
    json!({"freshCliFetchAndPublicationMs":timings[0],"retainedMcpPooledFetchAndPublicationMs":timings[1]})
}

#[tokio::test]
async fn installed_step_four_http_response_checkpoint() {
    let fixture = installed_fixture();
    let site = Site::start().await;
    let mut mcp = Mcp::start(&fixture).await;
    let mut cleanup_failures = Vec::new();
    for mode in ["headless", "headed"] {
        for engine in ["chrome", "firefox", "webkit"] {
            let connected = fixture
                .cli(&["browser", "connect", "--engine", engine, "--mode", mode])
                .await;
            let target = connected["targetId"].as_str().unwrap();
            fixture.ready().await;
            let result = std::panic::AssertUnwindSafe(async {
                let reads = decoded_responses(&fixture, &mut mcp, target, &site.url).await;
                let fetches = pooled_wire(&fixture, &mut mcp, target, &site.url).await;
                json!({"reads":reads,"fetches":fetches})
            })
            .catch_unwind()
            .await;
            if let Ok(latency) = &result {
                eprintln!("HTTP response latency {engine} {mode}: {latency}");
            }
            let disconnected = mcp
                .request(
                    "tools/call",
                    json!({"name":"disconnect","arguments":{"targetId":target}}),
                )
                .await;
            std::fs::write(
                fixture
                    .workspace
                    .path()
                    .join(format!("http-wire-disconnect-{engine}-{mode}.json")),
                serde_json::to_vec_pretty(&disconnected).unwrap(),
            )
            .unwrap();
            if disconnected["result"]["structuredContent"]["ok"] != true {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                cleanup_failures.push(disconnected);
            }
            if let Err(panic) = result {
                fixture.cli(&["disconnect"]).await;
                mcp.tool("disconnect", json!({})).await;
                mcp.stop().await;
                std::panic::resume_unwind(panic);
            }
        }
    }
    fixture.cli(&["disconnect"]).await;
    mcp.tool("disconnect", json!({})).await;
    mcp.stop().await;
    assert!(
        cleanup_failures.is_empty(),
        "HTTP disconnects: {cleanup_failures:?}"
    );
}
