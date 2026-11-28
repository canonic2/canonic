use super::*;
use crate::http_fixtures::wire_inputs as wire;
pub(super) async fn request(
    contexts: &mut Contexts,
    client: Reference,
    input: Fetch,
) -> (Value, Value) {
    let id = contexts
        .start(client, input, Instant::now() + Duration::from_secs(5))
        .unwrap();
    let Ready::Reply(reply) = ready(contexts, id).await.unwrap() else {
        panic!("Expected HTTP reply")
    };
    let body = serde_json::from_slice(reply.body.as_ref().unwrap()).unwrap();
    let details = contexts.response_read(id, 65536).unwrap();
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
    (body, details)
}
#[tokio::test]
async fn http_wire_input_present_bodies_replace_stale_content_lengths_including_empty_buffers() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &site.url);
    for method in ["GET", "POST"] {
        for bytes in [vec![0, 255, 128], b"Acme replacement".to_vec(), vec![]] {
            let input = Fetch {
                method: method.into(),
                headers: vec![Header {
                    name: "cOnTeNt-LeNgTh".into(),
                    value: "6".into(),
                }],
                body: Some(Payload::Base64 {
                    data: STANDARD.encode(&bytes),
                }),
                ..fetch("/echo")
            };
            let (body, _) = request(&mut contexts, client, input).await;
            assert_eq!(body["method"], method);
            assert_eq!(body["headers"]["content-length"], bytes.len().to_string());
            assert_eq!(body["bodyBase64"], STANDARD.encode(bytes));
        }
    }
    assert_eq!(site.records.lock().unwrap().len(), 6);
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}

#[tokio::test]
async fn http_wire_input_sdk_verb_shortcuts_reach_the_wire_and_head_has_no_body() {
    // Pinned APIRequestContext get/head/put/patch/delete/post are fetch with
    // that method; HEAD exposes an empty body even if the server sends bytes.
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &site.url);
    for method in ["GET", "HEAD", "PUT", "PATCH", "DELETE", "POST"] {
        let id = contexts
            .start(
                client,
                Fetch {
                    method: method.into(),
                    ..fetch("/verb")
                },
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
            panic!("Expected HTTP reply")
        };
        let recorded = site.records.lock().unwrap().last().unwrap().clone();
        assert_eq!(recorded["method"], method);
        let body = reply.body.clone().unwrap();
        if method == "HEAD" {
            assert!(body.is_empty(), "HEAD body: {body:?}");
        } else {
            let echoed: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(echoed["method"], method);
        }
        contexts.restore(id, reply);
        contexts.discard(id).unwrap();
    }
    assert_eq!(site.records.lock().unwrap().len(), 6);
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}

#[test]
fn http_wire_input_header_validation_uses_node_latin1_and_refuses_higher_unicode() {
    for character in ['ÿ', 'é', '\t'] {
        let input = Fetch {
            headers: vec![Header {
                name: "X-Acme".into(),
                value: character.to_string(),
            }],
            ..fetch("http://localhost/")
        };
        input.validate().unwrap();
    }
    for value in ["💩", "Acme\r\nInjected: yes", "Acme\0", "Acme\u{7f}"] {
        let options = Options {
            headers: Some(vec![Header {
                name: "X-Acme".into(),
                value: value.into(),
            }]),
            ..Default::default()
        };
        assert_eq!(options.validate().unwrap_err().code, "HTTP_HEADER_INVALID");
    }
}
#[test]
fn http_wire_input_urls_normalize_and_validate_percent_encoded_credentials() {
    use base64::Engine as _;
    let url = absolute_url("http://Acme%2B%C3%A9:one%3Atwo+three@example.com/").unwrap();
    assert_eq!(
        request_wire::authorization(&url).unwrap().unwrap(),
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("Acme+é:one:two+three")
        )
    );
    assert_eq!(
        absolute_url("\t https://Acme:known@example.com/Acme path?q=one two#fragment \r\n")
            .unwrap()
            .as_str(),
        "https://Acme:known@example.com/Acme%20path?q=one%20two#fragment"
    );
    for value in [
        "http://Acme:%ff@example.com/",
        "http://Acme:%xy@example.com/",
        "file:///Acme",
        "https:///",
    ] {
        assert!(absolute_url(value).is_err(), "{value}");
    }
}
#[tokio::test]
async fn http_wire_input_latin1_bytes_and_explicit_host_are_exact() {
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &site.url);
    let input = Fetch {
        headers: vec![
            Header {
                name: "X-Latin".into(),
                value: "ÿ".into(),
            },
            Header {
                name: "Host".into(),
                value: "Acme-host.invalid".into(),
            },
        ],
        ..fetch("/echo")
    };
    let (body, _) = request(&mut contexts, client, input).await;
    assert_eq!(body["headers"]["x-latin"], "ÿ");
    assert_eq!(body["headers"]["host"], "Acme-host.invalid");
    assert_eq!(site.records.lock().unwrap().len(), 1);
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}

#[tokio::test]
async fn http_wire_input_configured_credentials_override_url_only_on_challenge_or_always() {
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let base = site.url.replace("http://", "http://Acme:known@");
    for (send, expected_requests) in [("unauthorized", 2), ("always", 1)] {
        let client = serde_json::from_value(
            contexts
                .create(
                    contexts.connection.unwrap_or_else(Uuid::new_v4),
                    Options {
                        base_url: Some(base.clone()),
                        http_credentials: Some(
                            serde_json::from_value(json!({
                                "username":"AcmeConfigured","password":"correct","send":send
                            }))
                            .unwrap(),
                        ),
                        ..Default::default()
                    },
                )
                .unwrap()["httpContext"]
                .clone(),
        )
        .unwrap();
        let before = site.records.lock().unwrap().len();
        let (body, details) = request(&mut contexts, client, fetch("/auth-fallback")).await;
        assert_eq!(
            body["headers"]["authorization"],
            "Basic QWNtZUNvbmZpZ3VyZWQ6Y29ycmVjdA=="
        );
        assert_eq!(details["status"], 200);
        let records = site.records.lock().unwrap();
        assert_eq!(records.len() - before, expected_requests);
        if send == "unauthorized" {
            assert_eq!(
                records[before]["headers"]["authorization"],
                "Basic QWNtZTprbm93bg=="
            );
        }
        drop(records);
        contexts.remove(client).unwrap();
    }
    contexts.close().unwrap();
}

#[tokio::test]
async fn http_wire_input_invalid_values_refuse_without_jobs_or_origin_requests() {
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &site.url);
    for input in [
        Fetch {
            headers: vec![Header {
                name: "X-Acme".into(),
                value: "💩".into(),
            }],
            ..fetch("/echo")
        },
        fetch(&format!(
            "{}/%Acme",
            site.url.replace("http://", "http://Acme:%ff@")
        )),
    ] {
        assert!(
            contexts
                .start(client, input, Instant::now() + Duration::from_secs(5))
                .is_err()
        );
        assert!(contexts.jobs.is_empty());
        assert!(site.records.lock().unwrap().is_empty());
    }
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_wire_input_credentials_and_fragment_do_not_enter_transport_authority_or_target() {
    let site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let base = site.url.replace("http://", "http://Acme:known@");
    let client = create(&mut contexts, Uuid::new_v4(), &base);
    let (body, details) = request(&mut contexts, client, fetch("/echo#Acme")).await;
    assert_eq!(body["url"], "/echo");
    assert_eq!(body["headers"]["authorization"], "Basic QWNtZTprbm93bg==");
    assert_eq!(
        body["headers"]["host"],
        site.url.strip_prefix("http://").unwrap()
    );
    assert_eq!(details["url"], format!("{base}/echo#Acme"));
    let input = Fetch {
        headers: vec![Header {
            name: "Authorization".into(),
            value: "Acme explicit".into(),
        }],
        ..fetch("/echo")
    };
    assert_eq!(
        request(&mut contexts, client, input).await.0["headers"]["authorization"],
        "Acme explicit"
    );
    let (body, details) = request(&mut contexts, client, fetch("/redirect-credentials")).await;
    assert_eq!(
        body["headers"]["authorization"],
        "Basic QWNtZTI6Y2hhbmdlZA=="
    );
    assert_eq!(body["url"], "/after-redirect");
    assert!(
        details["url"]
            .as_str()
            .unwrap()
            .ends_with("/after-redirect#Acme")
    );
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}
