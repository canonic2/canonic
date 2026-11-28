//! Encoded transport, bounded decoded evidence and failure without HTTP replay.
use super::*;
use std::io::Write;

#[tokio::test]
async fn private_response_details_preserve_accepted_lf_headers_without_body_capture() {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
        }
        stream.write_all(b"HTTP/1.1 200 Acme LF\nX-Acme: original\nContent-Length: 4\nConnection: close\n\nAcme").await.unwrap();
    });
    let mut contexts = Contexts::default();
    let context = create(&mut contexts, Uuid::new_v4(), &base);
    let id = contexts
        .start(context, fetch("/"), Instant::now() + Duration::from_secs(5))
        .unwrap();
    let Ready::Reply(_) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Expected response")
    };
    let details = contexts.response_read(id, 65536).unwrap();
    assert_eq!(details["wireMetadataAvailable"], true);
    assert_eq!(details["statusText"], "Acme LF");
    assert_eq!(details["headers"][0]["name"], "X-Acme");
    contexts.remove(context).unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn wire_details_follow_redirects_reuse_connections_and_isolate_contexts() {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let counts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = counts.clone();
    let server = tokio::spawn(async move {
        serve_wire_details(listener, observed).await;
    });
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    let a = create(&mut contexts, connection, &base);
    let b = create(&mut contexts, connection, &base);
    let id = contexts
        .start(a, fetch("/hop"), Instant::now() + Duration::from_secs(5))
        .unwrap();
    let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Expected redirect result")
    };
    assert_eq!(reply.metadata["redirects"], 1);
    assert_eq!(
        contexts.response_read(id, 65536).unwrap()["statusText"],
        "Acme /final-A"
    );
    contexts.complete(id, reply.metadata, None).unwrap();
    let mut ids = Vec::new();
    for (context, path) in [(a, "/A"), (b, "/B")] {
        ids.push((
            contexts
                .start(
                    context,
                    fetch(path),
                    Instant::now() + Duration::from_secs(5),
                )
                .unwrap(),
            path,
        ));
    }
    for (id, path) in ids {
        let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
            panic!("Expected context result")
        };
        assert_eq!(reply.metadata["retryCount"], 0);
        assert_eq!(reply.metadata["replayed"], false);
        let details = contexts.response_read(id, 65536).unwrap();
        assert_eq!(details["statusText"], format!("Acme {path}"));
        assert_eq!(details["securityDetails"], Value::Null);
        assert_eq!(
            details["serverAddr"],
            json!({"ipAddress":"127.0.0.1","port":Url::parse(&base).unwrap().port().unwrap()})
        );
        assert_eq!(details["timing"]["secureConnectionStart"], -1.0);
        if path == "/A" {
            assert_eq!(
                details["timing"]["connectStart"], -1.0,
                "The same-origin redirect and next request reuse their socket"
            );
        } else {
            assert!(
                details["timing"]["connectStart"].as_f64().unwrap() >= 0.0,
                "The independent context owns a fresh socket"
            );
        }
        assert_eq!(details["wireMetadataAvailable"], true);
        assert_eq!(details["headers"][0], json!({"name":"X-Peer","value":path}));
        assert_eq!(details["headers"][1]["name"], "X-Middle");
        assert_eq!(
            details["headers"][2],
            json!({"name":"x-peer","value":"second"})
        );
        contexts.complete(id, reply.metadata, None).unwrap();
        contexts.response_remove(id);
    }
    assert_eq!(counts.load(Ordering::SeqCst), 4);
    contexts.close().unwrap();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
}

#[test]
fn response_details_have_independent_bounded_retention_and_explicit_removal() {
    let mut contexts = Contexts::default();
    let first = Uuid::new_v4();
    contexts
        .details
        .insert(first, json!({"private":"Acme secret"}));
    assert_eq!(
        contexts.response_read(first, 8).unwrap_err().code,
        "HTTP_RESPONSE_RESULT_BOUNDS"
    );
    assert_eq!(
        contexts.response_read(first, 65537).unwrap_err().code,
        "HTTP_RESPONSE_RESULT_BOUNDS"
    );
    for _ in 0..127 {
        contexts
            .details
            .insert(Uuid::new_v4(), json!({"private":true}));
    }
    assert_eq!(
        contexts.response_read(first, 65536).unwrap()["private"],
        "Acme secret"
    );
    let last = Uuid::new_v4();
    contexts.details.insert(last, json!({"private":true}));
    assert_eq!(
        contexts.response_read(first, 65536).unwrap_err().code,
        "UNKNOWN_HTTP_RESPONSE_DETAILS"
    );
    assert_eq!(contexts.response_remove(last)["removed"], true);
    assert_eq!(contexts.response_remove(last)["removed"], false);
    assert_eq!(
        contexts.response_read(last, 65536).unwrap_err().code,
        "UNKNOWN_HTTP_RESPONSE_DETAILS"
    );
}

#[tokio::test]
async fn private_response_details_preserve_duplicates_and_url_without_body_retention() {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(stream.read_u8().await.unwrap());
        }
        stream.write_all(b"HTTP/1.1 201 Acme private phrase\r\nX-Acme: one\r\nSet-Cookie: acme=secret; Path=/\r\nx-acme: two\r\nX-Binary: \xff\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
    });
    let mut contexts = Contexts::default();
    let context = create(&mut contexts, Uuid::new_v4(), &url);
    let mut request = fetch("/details?acme=private");
    request.read_body = false;
    let id = contexts
        .start(context, request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        contexts.response_read(id, 65536).unwrap_err().code,
        "UNKNOWN_HTTP_RESPONSE_DETAILS"
    );
    let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Expected response")
    };
    assert!(reply.body.is_none());
    let ordinary = contexts.complete(id, reply.metadata, None).unwrap();
    assert!(!ordinary.to_string().contains("secret"));
    assert!(!ordinary.to_string().contains("acme=private"));
    assert!(!ordinary.to_string().contains("Acme private phrase"));
    contexts.remove(context).unwrap();
    let details = contexts.response_read(id, 65536).unwrap();
    assert_eq!(details["url"], format!("{url}/details?acme=private"));
    assert_eq!(details["status"], 201);
    assert_eq!(details["ok"], true);
    assert_eq!(details["statusTextAvailable"], true);
    assert_eq!(details["statusText"], "Acme private phrase");
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
            .any(|h| h["name"] == "Set-Cookie" && h["value"] == "acme=secret; Path=/")
    );
    assert!(
        headers
            .iter()
            .any(|h| h["name"] == "X-Binary" && h["value"] == "ÿ" && h["valueBase64"] == "/w==")
    );
    assert_eq!(
        headers
            .iter()
            .map(|header| header["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "X-Acme",
            "Set-Cookie",
            "x-acme",
            "X-Binary",
            "Content-Length",
            "Connection"
        ]
    );
    assert_eq!(contexts.response_remove(id)["removed"], true);
    assert!(matches!(
        contexts.ready(id).unwrap(),
        Some(Ready::Complete(_))
    ));
    assert_eq!(
        contexts.response_read(id, 65536).unwrap_err().code,
        "UNKNOWN_HTTP_RESPONSE_DETAILS"
    );
    server.await.unwrap();
}

#[test]
fn private_response_reads_preserve_values_bounds_and_evidence_identity() {
    let root = tempfile::tempdir().unwrap();
    let format = crate::browser::artifacts::Format {
        prefix: "http",
        extension: "bin",
        mime: "application/octet-stream",
        name: "http-entity-body",
        max_bytes: MAX_BODY,
    };
    let data = br#"{"name":"Acme private","values":[null,true,3]}"#;
    let artifact = format.write(root.path(), data, json!({})).unwrap();
    let read = |format, limit| read_response(&artifact, format, limit);
    for _ in 0..2 {
        assert_eq!(
            read(BodyFormat::Json, 65536).unwrap()["value"],
            json!({"name":"Acme private","values":[null,true,3]})
        );
        assert_eq!(
            read(BodyFormat::Text, 65536).unwrap()["value"],
            std::str::from_utf8(data).unwrap()
        );
        assert_eq!(
            read(BodyFormat::Base64, 65536).unwrap()["value"],
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data)
        );
    }
    assert_eq!(
        read(BodyFormat::Json, 16).unwrap_err().code,
        "HTTP_RESPONSE_RESULT_BOUNDS"
    );
    assert_eq!(
        read(BodyFormat::Text, 65537).unwrap_err().code,
        "HTTP_RESPONSE_RESULT_BOUNDS"
    );
    let binary = format.write(root.path(), b"Acme\xff\0", json!({})).unwrap();
    assert_eq!(
        read_response(&binary, BodyFormat::Text, 65536).unwrap()["value"],
        "Acme\u{fffd}\0"
    );
    assert_eq!(
        read_response(&binary, BodyFormat::Json, 65536)
            .unwrap_err()
            .code,
        "HTTP_RESPONSE_JSON"
    );
    let path = artifact.summary["attachment"]["path"].as_str().unwrap();
    std::fs::write(path, vec![b'x'; data.len()]).unwrap();
    assert_eq!(
        read(BodyFormat::Text, 65536).unwrap_err().code,
        "EVIDENCE_CHANGED"
    );
}

#[test]
fn http_json_value_reads_preserve_special_values_and_refuse_oversize_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let format = crate::browser::artifacts::Format {
        prefix: "http",
        extension: "bin",
        mime: "application/octet-stream",
        name: "http-entity-body",
        max_bytes: MAX_BODY,
    };
    let bytes = format!("[1e400,\"{}uD800\"]", char::from(92));
    let artifact = format
        .write(root.path(), bytes.as_bytes(), json!({}))
        .unwrap();
    let refused = read_response(&artifact, BodyFormat::Json, 65536).unwrap_err();
    assert_eq!(refused.code, "HTTP_RESPONSE_JSON");
    assert_eq!(
        refused.partial.as_ref().unwrap()["evidenceId"],
        json!(artifact.id)
    );
    assert_eq!(refused.partial.as_ref().unwrap()["replayed"], false);
    let value = read_response(&artifact, BodyFormat::JsonValue, 65536).unwrap();
    assert_eq!(value["value"]["nodes"][1]["value"], "positive_infinity");
    assert_eq!(value["value"]["nodes"][2]["units"], json!([0xd800]));
    let exact = serde_json::to_vec(&value).unwrap().len();
    assert_eq!(
        read_response(&artifact, BodyFormat::JsonValue, exact).unwrap(),
        value
    );
    assert_eq!(
        read_response(&artifact, BodyFormat::JsonValue, exact - 1)
            .unwrap_err()
            .code,
        "HTTP_RESPONSE_RESULT_BOUNDS"
    );
    assert_eq!(artifact.read().unwrap(), bytes.as_bytes());
    assert_eq!(
        read_response(&artifact, BodyFormat::JsonValue, 65536).unwrap(),
        value
    );
}

fn encode(coding: &str, bytes: &[u8]) -> Vec<u8> {
    if coding == "deflate" {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    } else {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }
}

#[test]
fn response_codings_preserve_binary_bytes_and_enforce_decoded_limits() {
    let bytes = b"Acme\0private\xff\xfe";
    for coding in ["gzip", "x-gzip", "GZIP", "deflate"] {
        let encoded = encode(coding, bytes);
        let decoded = super::super::response::decode(encoded.clone(), coding, bytes.len()).unwrap();
        assert_eq!(decoded.bytes, bytes);
        assert!(decoded.decoded);
        assert_eq!(
            super::super::response::decode(encoded, coding, bytes.len() - 1).err(),
            Some("HTTP_BODY_BOUNDS")
        );
        assert_eq!(
            super::super::response::decode(Vec::new(), coding, 1)
                .unwrap()
                .bytes,
            b""
        );
    }
    // Brotli uncompressed meta-block containing eleven bytes.
    let brotli = b"\x0b\x05\x80hello world\x03";
    let decoded = super::super::response::decode(brotli.to_vec(), "br", 11).unwrap();
    assert_eq!(decoded.bytes, b"hello world");
    assert!(decoded.decoded);
    assert_eq!(
        super::super::response::decode(brotli.to_vec(), "br", 10).err(),
        Some("HTTP_BODY_BOUNDS")
    );
    for coding in ["", "identity", "Acme-private-encoding", "gzip, br"] {
        let value = super::super::response::decode(bytes.to_vec(), coding, bytes.len()).unwrap();
        assert_eq!(value.bytes, bytes);
        assert!(!value.decoded);
    }
}

#[test]
fn gzip_members_and_sync_flush_truncation_match_the_pinned_request_reader() {
    let mut deflate = encode("deflate", b"Acme private");
    deflate.pop();
    assert_eq!(
        super::super::response::decode(deflate, "deflate", 32).err(),
        Some("HTTP_BODY_DECODE")
    );
    let first = encode("gzip", b"Acme ");
    let mut members = first.clone();
    members.extend(encode("gzip", b"private"));
    assert_eq!(
        super::super::response::decode(members, "gzip", 12)
            .unwrap()
            .bytes,
        b"Acme private"
    );
    let mut truncated = first.clone();
    truncated.truncate(first.len() - 8);
    assert_eq!(
        super::super::response::decode(truncated, "gzip", 12)
            .unwrap()
            .bytes,
        b"Acme "
    );
    let mut corrupt = first;
    let end = corrupt.len();
    corrupt[end - 8] ^= 255;
    assert_eq!(
        super::super::response::decode(corrupt, "gzip", 12).err(),
        Some("HTTP_BODY_DECODE")
    );
    for coding in ["gzip", "deflate", "br"] {
        assert_eq!(
            super::super::response::decode(vec![255; 32], coding, 32).err(),
            Some("HTTP_BODY_DECODE")
        );
    }
}

async fn response_site(
    coding: &str,
    bytes: Vec<u8>,
    expected: Vec<&str>,
) -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let coding = coding.to_owned();
    let expected: Vec<String> = expected.into_iter().map(str::to_owned).collect();
    let task = tokio::spawn(async move {
        for accepted_encoding in expected {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(stream.read_u8().await.unwrap());
                assert!(header.len() < 16384);
            }
            let header = String::from_utf8(header).unwrap();
            assert!(
                header.lines().any(|line| line
                    .split_once(':')
                    .is_some_and(|(name, value)| name.eq_ignore_ascii_case("accept-encoding")
                        && value.trim() == accepted_encoding)),
                "{header}"
            );
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Encoding: {coding}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).as_bytes()).await.unwrap();
            let _ = stream.write_all(&bytes).await;
        }
    });
    (url, task)
}

#[tokio::test]
async fn compressed_http_responses_finalize_decoded_evidence_and_keep_encoding_defaults() {
    let directory = tempfile::tempdir().unwrap();
    for coding in ["gzip", "deflate", "br"] {
        let bytes = b"hello world";
        let encoded = if coding == "br" {
            b"\x0b\x05\x80hello world\x03".to_vec()
        } else {
            encode(coding, bytes)
        };
        let length = encoded.len();
        let (url, server) = response_site(
            coding,
            encoded,
            vec!["gzip,deflate,br", "identity", "gzip,deflate,br"],
        )
        .await;
        let mut contexts = Contexts::default();
        let context = create(&mut contexts, Uuid::new_v4(), &url);
        for accepted in [None, Some("identity"), None] {
            let mut request = fetch("/");
            request.max_bytes = bytes.len();
            if let Some(value) = accepted {
                request.headers.push(Header {
                    name: "Accept-Encoding".into(),
                    value: value.into(),
                });
            }
            let id = contexts
                .start(context, request, Instant::now() + Duration::from_secs(5))
                .unwrap();
            let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
                panic!("Expected response bytes")
            };
            assert_eq!(reply.metadata["bodyDecoded"], true);
            assert_eq!(reply.metadata["bodyBytes"], bytes.len());
            assert_eq!(reply.metadata["encodedBodyBytes"], length);
            assert_eq!(reply.metadata["contentEncoding"], coding);
            assert!(!reply.metadata.to_string().contains("hello world"));
            let artifact = materialize(directory.path(), &reply).unwrap().unwrap();
            assert_eq!(
                std::fs::read(artifact.summary["attachment"]["path"].as_str().unwrap()).unwrap(),
                bytes
            );
            contexts
                .complete(id, reply.metadata, Some(artifact))
                .unwrap();
            assert!(matches!(
                contexts.ready(id).unwrap(),
                Some(Ready::Complete(_))
            ));
        }
        contexts.remove(context).unwrap();
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn decoded_body_failures_keep_response_metadata_and_do_not_retry() {
    for (coding, encoded, limit, expected) in [
        (
            "gzip",
            encode("gzip", &vec![b'x'; 4096]),
            32,
            "HTTP_BODY_BOUNDS",
        ),
        ("gzip", vec![255; 32], 64, "HTTP_BODY_DECODE"),
        ("deflate", vec![255; 32], 64, "HTTP_BODY_DECODE"),
        ("br", vec![255; 32], 64, "HTTP_BODY_DECODE"),
        (
            "identity",
            vec![b'x'; MAX_BODY + 1],
            MAX_BODY,
            "HTTP_BODY_BOUNDS",
        ),
    ] {
        let (url, server) = response_site(coding, encoded, vec!["gzip,deflate,br"]).await;
        let mut contexts = Contexts::default();
        let context = create(&mut contexts, Uuid::new_v4(), &url);
        let mut request = fetch("/");
        request.max_bytes = limit;
        request.max_retries = 2;
        let id = contexts
            .start(context, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        let error = ready(&mut contexts, id)
            .await
            .err()
            .expect("Response must refuse");
        assert_eq!(error.code, expected);
        let partial = error.partial.unwrap();
        assert_eq!(partial["status"], 200);
        assert_eq!(partial["bodyComplete"], false);
        assert_eq!(partial["replayed"], false);
        assert_eq!(partial["retryCount"], 0);
        contexts.remove(context).unwrap();
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
    }
}

async fn serve_wire_details(
    listener: tokio::net::TcpListener,
    observed: Arc<std::sync::atomic::AtomicUsize>,
) {
    let mut connections = tokio::task::JoinSet::new();
    for _ in 0..2 {
        let (mut stream, _) = listener.accept().await.unwrap();
        let count = observed.clone();
        connections.spawn(async move {
                loop {
                    let mut header=Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        match stream.read_u8().await { Ok(byte)=>header.push(byte),Err(_)=>return }
                    }
                    count.fetch_add(1,Ordering::SeqCst);
                    let header=String::from_utf8(header).unwrap();
                    let path=header.split_whitespace().nth(1).unwrap();
                    if path=="/hop" {
                        stream.write_all(b"HTTP/1.1 302 Acme hop\r\nLocation: /final-A\r\nX-Redirect: private old\r\nContent-Length: 0\r\n\r\n").await.unwrap();
                    } else {
                        stream.write_all(format!("HTTP/1.1 200 Acme {path}\r\nX-Peer: {path}\r\nX-Middle: between\r\nx-peer: second\r\nContent-Length: 4\r\nConnection: keep-alive\r\n\r\nAcme").as_bytes()).await.unwrap();
                    }
                }
            });
    }
    while let Some(result) = connections.join_next().await {
        result.unwrap();
    }
}
