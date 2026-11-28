//! Wire assertions measured independently against the pinned request client.
use super::wire_inputs::request;
use super::*;
use crate::http_fixtures::wire_inputs as wire;
use base64::{Engine as _, engine::general_purpose::STANDARD};
fn header(name: &str, value: &str) -> Header {
    Header {
        name: name.into(),
        value: value.into(),
    }
}
#[tokio::test]
async fn http_json_string_raw_body_defaults_preserve_empty_headers_and_absent_bodies() {
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &site.url);
    for payload in [
        Payload::Text {
            text: "Acme".into(),
        },
        Payload::Text {
            text: String::new(),
        },
        Payload::Base64 {
            data: "AP+A".into(),
        },
        Payload::Base64 {
            data: String::new(),
        },
    ] {
        let expected = payload.bytes().unwrap();
        let (body, _) = request(
            &mut contexts,
            client,
            Fetch {
                method: "POST".into(),
                body: Some(payload),
                ..fetch("/echo")
            },
        )
        .await;
        assert_eq!(
            body["headers"]["content-type"], "application/octet-stream",
            "{body}"
        );
        assert_eq!(
            STANDARD
                .decode(body["bodyBase64"].as_str().unwrap())
                .unwrap(),
            expected
        );
        assert_eq!(
            body["headers"]["content-length"],
            expected.len().to_string()
        );
    }
    let (body, _) = request(
        &mut contexts,
        client,
        Fetch {
            method: "POST".into(),
            headers: vec![header("Content-Type", "")],
            body: Some(Payload::Text {
                text: "Acme".into(),
            }),
            ..fetch("/echo")
        },
    )
    .await;
    assert_eq!(body["headers"]["content-type"], "");
    let (body, _) = request(
        &mut contexts,
        client,
        Fetch {
            method: "POST".into(),
            body: None,
            ..fetch("/echo")
        },
    )
    .await;
    assert!(body["headers"].get("content-type").is_none(), "{body}");
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_json_string_caller_header_precedence_and_context_defaults_match_pinned_bytes() {
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let owner = Uuid::new_v4();
    let plain = create(&mut contexts, owner, &site.url);
    let defaults = contexts
        .create(
            owner,
            Options {
                base_url: Some(site.url.clone()),
                headers: Some(vec![header("Content-Type", "application/json")]),
                ..Default::default()
            },
        )
        .unwrap();
    let defaults: Reference = serde_json::from_value(defaults["httpContext"].clone()).unwrap();
    let input = "Acme invalid\nline";
    let quoted = b"\"Acme invalid\\nline\"".as_slice();
    for (client, headers, expected, mime) in [
        (
            plain,
            vec![header("Content-Type", "application/json")],
            quoted,
            "application/json",
        ),
        (
            plain,
            vec![header("Content-Type", "text/plain")],
            input.as_bytes(),
            "text/plain",
        ),
        (
            plain,
            vec![header("Content-Type", "Application/JSON")],
            input.as_bytes(),
            "Application/JSON",
        ),
        (
            plain,
            vec![header("Content-Type", "application/json; charset=utf-8")],
            input.as_bytes(),
            "application/json; charset=utf-8",
        ),
        (
            plain,
            vec![
                header("Content-Type", "application/json"),
                header("content-type", "text/plain"),
            ],
            quoted,
            "text/plain",
        ),
        (
            plain,
            vec![
                header("Content-Type", "text/plain"),
                header("content-type", "application/json"),
            ],
            input.as_bytes(),
            "application/json",
        ),
        (defaults, vec![], input.as_bytes(), "application/json"),
    ] {
        let (body, _) = request(
            &mut contexts,
            client,
            Fetch {
                method: "POST".into(),
                headers,
                body: Some(Payload::Text { text: input.into() }),
                ..fetch("/echo")
            },
        )
        .await;
        assert_eq!(
            STANDARD
                .decode(body["bodyBase64"].as_str().unwrap())
                .unwrap(),
            expected,
            "{body}"
        );
        assert_eq!(body["headers"]["content-type"], mime, "{body}");
        assert_eq!(
            body["headers"]["content-length"],
            expected.len().to_string()
        );
    }
    contexts.remove(plain).unwrap();
    contexts.remove(defaults).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_json_string_valid_text_keeps_whitespace_duplicates_overflow_surrogates_and_depth() {
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &site.url);
    let escaped = format!("\"{}u{}\"", '\\', "d800");
    for text in [
        " { \"Acme\": 1, \"Acme\": 2 } \n".to_owned(),
        "1e9999".into(),
        escaped,
        format!("{}0{}", "[".repeat(512), "]".repeat(512)),
        "true".into(),
        "null".into(),
    ] {
        let (body, _) = request(
            &mut contexts,
            client,
            Fetch {
                method: "POST".into(),
                headers: vec![header("Content-Type", "application/json")],
                body: Some(Payload::Text { text: text.clone() }),
                ..fetch("/echo")
            },
        )
        .await;
        assert_eq!(
            STANDARD
                .decode(body["bodyBase64"].as_str().unwrap())
                .unwrap(),
            text.as_bytes(),
            "{body}"
        );
    }
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_json_string_empty_text_quotes_but_binary_data_bypasses_string_processing() {
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &site.url);
    for (payload, expected) in [
        (
            Payload::Text {
                text: String::new(),
            },
            b"\"\"".to_vec(),
        ),
        (
            Payload::Base64 {
                data: String::new(),
            },
            vec![],
        ),
        (
            Payload::Base64 {
                data: "AP+A".into(),
            },
            vec![0, 255, 128],
        ),
    ] {
        let (body, _) = request(
            &mut contexts,
            client,
            Fetch {
                method: "POST".into(),
                headers: vec![
                    header("Content-Type", "application/json"),
                    header("Content-Length", "6"),
                ],
                body: Some(payload),
                ..fetch("/echo")
            },
        )
        .await;
        assert_eq!(
            STANDARD
                .decode(body["bodyBase64"].as_str().unwrap())
                .unwrap(),
            expected,
            "{body}"
        );
        assert_eq!(
            body["headers"]["content-length"],
            expected.len().to_string()
        );
    }
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_json_string_escaped_expansion_refuses_before_http_allocation_or_origin_io() {
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &site.url);
    let input = Fetch {
        method: "POST".into(),
        headers: vec![header("Content-Type", "application/json")],
        body: Some(Payload::Text {
            text: "\0".repeat(MAX_BODY / 6 + 1),
        }),
        ..fetch("/echo")
    };
    assert_eq!(
        contexts
            .start(client, input, Instant::now() + Duration::from_secs(5))
            .unwrap_err()
            .code,
        "HTTP_REQUEST_BODY_BOUNDS"
    );
    assert!(contexts.jobs.is_empty());
    assert!(site.records.lock().unwrap().is_empty());
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}
