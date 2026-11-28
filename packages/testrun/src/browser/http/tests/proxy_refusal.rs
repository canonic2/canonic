//! Refusing CONNECT proxies never own an origin request or destination TLS.
use super::*;
#[path = "../../../../tests/fixtures/browser_http_proxy_refusal.rs"]
mod endpoint;

#[tokio::test]
async fn http_proxy_refusal_head_body_remains_protocol_failure_without_replay() {
    let proxy = endpoint::Refusal::start(vec![endpoint::response(502, "", "Acme")]).await;
    let mut contexts = Contexts::default();
    let context = client(&mut contexts, &proxy, "https");
    let mut request = fetch(&proxy.url("https", "/Acme"));
    request.method = "HEAD".into();
    request.read_body = true;
    request.max_retries = 2;
    let id = contexts
        .start(context, request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    let error = match ready(&mut contexts, id).await {
        Err(error) => error,
        _ => panic!("Pinned SDK refuses body bytes on HEAD"),
    };
    assert_eq!(error.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
    let partial = error.partial.unwrap();
    assert_eq!(partial["failureKind"], "io");
    assert_eq!(partial["connectionReset"], false);
    assert_eq!(partial["replayed"], false);
    assert_eq!(partial["retryCount"], 0);
    assert_eq!(proxy.settled(1).await[0]["trailingBytes"], 0);
}

#[tokio::test]
async fn http_proxy_refusal_chunked_and_head_use_response_parser_semantics() {
    for (method, body) in [("GET", "Acme"), ("HEAD", "")] {
        let bytes = if method == "HEAD" {
            b"HTTP/1.1 502 Acme refusal\r\nContent-Length: 4\r\nConnection: close\r\n\r\n".to_vec()
        } else {
            b"HTTP/1.1 502 Acme refusal\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nAcme\r\n0\r\n\r\n".to_vec()
        };
        let proxy = endpoint::Refusal::start(vec![bytes]).await;
        let mut contexts = Contexts::default();
        let context = client(&mut contexts, &proxy, "https");
        let mut request = fetch(&proxy.url("https", "/Acme"));
        request.method = method.into();
        request.read_body = true;
        let id = contexts
            .start(context, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
            panic!("Retained proxy response")
        };
        assert_eq!(reply.metadata["status"], 502);
        assert_eq!(reply.body.as_deref(), Some(body.as_bytes()));
        let details = contexts.response_read(id, 65536).unwrap();
        assert_eq!(details["serverAddr"], Value::Null);
        assert_eq!(details["securityDetails"], Value::Null);
        for field in [
            "domainLookupStart",
            "domainLookupEnd",
            "connectStart",
            "secureConnectionStart",
            "connectEnd",
            "requestStart",
        ] {
            assert_eq!(details["timing"][field], -1.0, "Synthetic refusal {field}");
        }
        assert_eq!(
            contexts.response_read(id, 65536).unwrap()["statusText"],
            "Acme refusal"
        );
        contexts.restore(id, reply);
        contexts.discard(id).unwrap();
        assert_eq!(proxy.settled(1).await[0]["trailingBytes"], 0);
    }
}

#[tokio::test]
async fn http_proxy_refusal_truncated_body_retains_failure_without_replay() {
    let mut bytes = endpoint::response(502, "", "Acme");
    bytes.pop();
    let proxy = endpoint::Refusal::start(vec![bytes]).await;
    let mut contexts = Contexts::default();
    let context = client(&mut contexts, &proxy, "https");
    let mut request = fetch(&proxy.url("https", "/Acme"));
    request.read_body = true;
    request.max_retries = 2;
    let id = contexts
        .start(context, request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    let error = match ready(&mut contexts, id).await {
        Err(error) => error,
        _ => panic!("Incomplete refusal body must not become complete evidence"),
    };
    assert_eq!(error.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
    let partial = error.partial.unwrap();
    assert_eq!(partial["failureKind"], "response_body");
    assert_eq!(partial["response"]["status"], 502);
    assert_eq!(partial["connectionReset"], false);
    assert_eq!(partial["replayed"], false);
    assert_eq!(proxy.settled(1).await[0]["trailingBytes"], 0);
}

#[tokio::test]
async fn http_proxy_malformed_connect_replies_fail_once_without_origin_dispatch_or_retry() {
    // Pinned HttpsProxyAgent: early end, an empty status line and a colon-less
    // header reject the CONNECT; only ECONNRESET is retried, so none replays.
    for bytes in [
        b"".to_vec(),
        b"\r\n\r\n".to_vec(),
        b"HTTP/1.1 200 Connection established\r\nAcme invalid header\r\n\r\n".to_vec(),
    ] {
        let proxy = endpoint::Refusal::start(vec![bytes.clone()]).await;
        let mut contexts = Contexts::default();
        let context = client(&mut contexts, &proxy, "http");
        let mut request = fetch(&proxy.url("http", "/Acme"));
        request.max_retries = 2;
        let id = contexts
            .start(context, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        let error = match ready(&mut contexts, id).await {
            Err(error) => error,
            Ok(_) => panic!("Malformed CONNECT reply {bytes:?} must not produce a response"),
        };
        let partial = error.partial.unwrap_or_default();
        assert_ne!(partial["replayed"], true, "{bytes:?}: {partial}");
        assert!(partial.get("response").is_none(), "{bytes:?}: {partial}");
        // No origin request may enter a tunnel the pinned agent rejected.
        assert_eq!(proxy.settled(1).await[0]["trailingBytes"], 0, "{bytes:?}");
        contexts.close().unwrap();
    }
    let closed = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let mut contexts = Contexts::default();
    let options: Options =
        serde_json::from_value(json!({"proxy":{"server":format!("http://127.0.0.1:{port}")}}))
            .unwrap();
    let context = serde_json::from_value(
        contexts.create(Uuid::new_v4(), options).unwrap()["httpContext"].clone(),
    )
    .unwrap();
    let mut request = fetch("http://127.0.0.1:9/Acme");
    request.max_retries = 2;
    let id = contexts
        .start(context, request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    let error = match ready(&mut contexts, id).await {
        Err(error) => error,
        Ok(_) => panic!("A refused proxy connection must not produce a response"),
    };
    let partial = error.partial.unwrap_or_default();
    assert_ne!(partial["replayed"], true, "{partial}");
    contexts.close().unwrap();
}

fn client(contexts: &mut Contexts, proxy: &endpoint::Refusal, scheme: &str) -> Reference {
    let options:Options=serde_json::from_value(json!({
        "baseUrl":proxy.url(scheme,"/Acme"),
        "proxy":{"server":proxy.proxy,"username":"Acme proxy","password":"proxy example"},
        "headers":[{"name":"Authorization","value":"Bearer Acme origin secret"},{"name":"X-Acme","value":"origin secret"},{"name":"Cookie","value":"Acme=private"}]
    })).unwrap();
    serde_json::from_value(contexts.create(Uuid::new_v4(), options).unwrap()["httpContext"].clone())
        .unwrap()
}

#[tokio::test]
async fn http_proxy_refusal_preserves_explicit_status_and_redirect_policy_without_retries() {
    for (status, headers, redirects, fail, next) in [
        (301, "Location: /Acme-next\r\n", Some(0), false, None),
        (503, "", None, true, None),
        (302, "Location: /Acme-next\r\n", None, false, Some(403)),
    ] {
        let mut script = vec![endpoint::response(status, headers, "Acme")];
        if let Some(next) = next {
            script.push(endpoint::response(next, "", "Acme"));
        }
        let proxy = endpoint::Refusal::start(script).await;
        let mut contexts = Contexts::default();
        let context = client(&mut contexts, &proxy, "http");
        let mut request = fetch(&proxy.url("http", "/Acme"));
        request.max_redirects = redirects;
        request.fail_on_status_code = Some(fail);
        request.max_retries = 2;
        request.read_body = true;
        let id = contexts
            .start(context, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        let result = ready(&mut contexts, id).await;
        if fail {
            let error = match result {
                Err(error) => error,
                _ => panic!("Explicit status refusal"),
            };
            assert_eq!(error.code, "HTTP_STATUS");
            let partial = error.partial.unwrap();
            assert_eq!(partial["status"], 503);
            assert_eq!(partial["retryCount"], 0);
            assert_eq!(partial["replayed"], false);
        } else {
            let Ready::Reply(reply) = result.unwrap() else {
                panic!("Owned refusal response")
            };
            assert_eq!(reply.metadata["status"], next.unwrap_or(status));
            assert_eq!(reply.metadata["retryCount"], 0);
            assert_eq!(reply.metadata["replayed"], false);
            assert_eq!(
                Url::parse(reply.metadata["url"].as_str().unwrap())
                    .unwrap()
                    .path(),
                if next.is_some() {
                    "/Acme-next"
                } else {
                    "/Acme"
                }
            );
            assert_eq!(reply.body.as_deref(), Some(b"Acme".as_slice()));
            contexts.restore(id, reply);
            contexts.discard(id).unwrap();
        }
        for row in proxy.settled(if next.is_some() { 2 } else { 1 }).await {
            assert_eq!(row["trailingBytes"], 0);
        }
    }
}
#[tokio::test]
async fn http_proxy_refusal_preserves_non200_responses_without_origin_bytes_or_tls() {
    for (scheme, status, body, headers) in [
        (
            "http",
            407,
            "Acme",
            "Proxy-Authenticate: Basic realm=\"Acme\"\r\n",
        ),
        ("https", 502, "Acme", ""),
        ("https", 201, "Acme", ""),
        ("http", 204, "", ""),
    ] {
        let proxy = endpoint::Refusal::start(vec![endpoint::response(status, headers, body)]).await;
        let mut contexts = Contexts::default();
        let context = client(&mut contexts, &proxy, scheme);
        let mut request = fetch(&proxy.url(scheme, "/Acme"));
        request.read_body = true;
        if status == 502 {
            request.method = "POST".into();
            request.body = Some(Payload::Text {
                text: "Acme private body".into(),
            });
        }
        let id = contexts
            .start(context, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        let Ready::Reply(reply) = ready(&mut contexts, id)
            .await
            .expect("A complete CONNECT refusal is an HTTP response")
        else {
            panic!("Owned response")
        };
        assert_eq!(reply.metadata["status"], status);
        assert_eq!(
            contexts.response_read(id, 65536).unwrap()["statusText"],
            "Acme refusal"
        );
        assert_eq!(reply.body.as_deref(), Some(body.as_bytes()));
        contexts.restore(id, reply);
        contexts.discard(id).unwrap();
        let rows = proxy.settled(1).await;
        assert_eq!(rows[0]["trailingBytes"], 0);
        assert_eq!(rows[0]["headers"].as_object().unwrap().len(), 3);
        assert_eq!(rows[0]["headers"]["proxy-connection"], "close");
        assert_eq!(rows[0]["headers"]["host"], "127.0.0.1:PORT");
    }
}
