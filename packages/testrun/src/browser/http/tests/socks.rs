//! SOCKS HTTP defaults, private handshake credentials and bounded transport ownership.
use super::*;
use crate::browser::socks_fixture as endpoint;

#[tokio::test]
async fn browser_socks_fixture_correlates_payloads_and_refuses_unselected_destinations() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let origin = endpoint::Site::start(endpoint::Mode::BrowserForward, false).await;
    let proxy = origin.fork_proxy().await;
    let url = origin.url("http", "acme.invalid", "/Acme-page");
    let mut contexts = Contexts::default();
    let client = context(
        &mut contexts,
        json!({"server":proxy.server("socks5","")}),
        &url,
    );
    let mut request = fetch(&url);
    request.read_body = true;
    let id = contexts
        .start(client, request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Selected fixture response")
    };
    assert_eq!(reply.metadata["status"], 200);
    assert!(
        std::str::from_utf8(reply.body.as_ref().unwrap())
            .unwrap()
            .contains("Acme SOCKS")
    );
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
    let facts = proxy.settled(1).await;
    assert_eq!(facts["connections"][0]["applicationData"], true);
    let authority = proxy.proxy.strip_prefix("http://").unwrap();
    let mut wrong = tokio::net::TcpStream::connect(authority).await.unwrap();
    wrong.write_all(b"GET ").await.unwrap();
    assert_eq!(wrong.read(&mut [0; 1]).await.unwrap(), 0);
    let mut unselected = tokio::net::TcpStream::connect(authority).await.unwrap();
    unselected.write_all(&[5, 1, 0]).await.unwrap();
    let mut methods = [0; 2];
    unselected.read_exact(&mut methods).await.unwrap();
    assert_eq!(methods, [5, 0]);
    let mut request = vec![5, 1, 0, 3, 12];
    request.extend_from_slice(b"acme.invalid");
    request.extend_from_slice(&0u16.to_be_bytes());
    unselected.write_all(&request).await.unwrap();
    let mut refusal = [0; 10];
    unselected.read_exact(&mut refusal).await.unwrap();
    assert_eq!(refusal, [5, 2, 0, 1, 0, 0, 0, 0, 0, 0]);
    let facts = proxy.settled(1).await;
    assert_eq!(proxy.protocol_errors(), vec![71]);
    assert_eq!(proxy.protocol_prefixes(), vec![b"GET ".to_vec()]);
    assert_eq!(proxy.refused(), 1);
    assert_eq!(facts["origin"].as_array().unwrap().len(), 1);
    assert!(
        origin.settled(0).await["connections"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

fn context(contexts: &mut Contexts, proxy: Value, url: &str) -> Reference {
    let options:Options=serde_json::from_value(json!({"baseUrl":url,"proxy":proxy,"headers":[{"name":"Authorization","value":"Bearer Acme origin"},{"name":"X-Acme","value":"origin"}]})).unwrap();
    let owner = contexts.connection.unwrap_or_else(Uuid::new_v4);
    serde_json::from_value(contexts.create(owner, options).unwrap()["httpContext"].clone()).unwrap()
}

#[tokio::test]
async fn http_socks_wire_aliases_uri_auth_and_ignored_object_credentials_match_pinned_defaults() {
    for (scheme, userinfo, force_auth, object_auth) in [
        ("socks5", "", false, false),
        ("socks5h", "", false, false),
        ("socks", "", false, false),
        ("socks4", "", false, false),
        ("socks4a", "", false, false),
        ("socks5", "Acme%3A%C3%A9:p%40ss@", false, false),
        ("socks4", "Acme%3Auser:ignored@", false, false),
        ("socks5", "", false, true),
        ("socks5", "", true, false),
    ] {
        let site = endpoint::Site::start(
            if force_auth {
                endpoint::Mode::ForceAuth
            } else {
                endpoint::Mode::Forward
            },
            false,
        )
        .await;
        let url = site.url("http", "localhost", "/echo");
        let mut proxy = json!({"server":site.server(scheme,userinfo)});
        if object_auth {
            proxy["username"] = json!("Acme ignored");
            proxy["password"] = json!("ignored example");
        }
        let mut contexts = Contexts::default();
        let context = context(&mut contexts, proxy, &url);
        let mut request = fetch(&url);
        request.read_body = true;
        let id = contexts
            .start(context, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
            panic!("Owned SOCKS response")
        };
        assert_eq!(reply.metadata["status"], 200);
        assert_eq!(reply.body.as_deref(), Some(b"Acme".as_slice()));
        assert_eq!(
            contexts.response_read(id, 65536).unwrap()["wireMetadataAvailable"],
            true
        );
        contexts.restore(id, reply);
        contexts.discard(id).unwrap();
        let facts = site.settled(1).await;
        let connection = &facts["connections"][0];
        let version = if scheme.starts_with("socks4") { 4 } else { 5 };
        assert_eq!(connection["version"], version);
        assert_eq!(
            connection["destination"],
            json!({"type":"domain","host":"localhost","port":"PORT"})
        );
        if version == 4 {
            assert_eq!(
                connection["userId"],
                if userinfo.is_empty() { "" } else { "Acme:user" }
            );
        } else {
            assert_eq!(
                connection["methods"],
                if userinfo.is_empty() {
                    json!([0])
                } else {
                    json!([0, 2])
                }
            );
            assert_eq!(
                connection["authentication"],
                if force_auth {
                    json!({"userId":"","password":""})
                } else if userinfo.is_empty() {
                    Value::Null
                } else {
                    json!({"userId":"Acme:é","password":"p@ss"})
                }
            );
        }
        assert_eq!(
            facts["origin"],
            json!([{"path":"/echo","host":"localhost:PORT","authorization":"Bearer Acme origin","proxyAuthorization":"","custom":"origin"}])
        );
    }
}

#[tokio::test]
async fn http_socks_address_framing_preserves_ipv4_ipv6_and_socks4_domain_fallback() {
    for (scheme, host, destination) in [
        (
            "socks5",
            "127.0.0.1",
            json!({"type":"ipv4","addressHex":"7f000001","port":"PORT"}),
        ),
        (
            "socks5",
            "[::1]",
            json!({"type":"ipv6","addressHex":"00000000000000000000000000000001","port":"PORT"}),
        ),
        (
            "socks4",
            "127.0.0.1",
            json!({"type":"ipv4","addressHex":"7f000001","port":"PORT"}),
        ),
        (
            "socks4",
            "[::1]",
            json!({"type":"domain","host":"::1","port":"PORT"}),
        ),
    ] {
        let site = endpoint::Site::start(endpoint::Mode::Forward, false).await;
        let url = site.url("http", host, "/echo");
        let mut contexts = Contexts::default();
        let context = context(
            &mut contexts,
            json!({"server":site.server(scheme,"")}),
            &url,
        );
        let mut request = fetch(&url);
        request.read_body = true;
        let id = contexts
            .start(context, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
            panic!("SOCKS address response")
        };
        assert_eq!(reply.metadata["status"], 200);
        contexts.restore(id, reply);
        contexts.discard(id).unwrap();
        let facts = site.settled(1).await;
        assert_eq!(facts["connections"][0]["destination"], destination);
        assert_eq!(facts["origin"][0]["host"], format!("{host}:PORT"));
    }
}

#[tokio::test]
async fn http_socks_bypass_is_reselected_on_both_redirect_directions() {
    for (host, path, second) in [
        ("localhost", "/forward", "127.0.0.1"),
        ("127.0.0.1", "/reverse", "localhost"),
    ] {
        let site = endpoint::Site::start(endpoint::Mode::Forward, false).await;
        let url = site.url("http", host, path);
        let mut contexts = Contexts::default();
        let context = context(
            &mut contexts,
            json!({"server":site.server("socks5",""),"bypass":" 127.0.0.1 "}),
            &url,
        );
        let mut request = fetch(&url);
        request.read_body = true;
        let id = contexts
            .start(context, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
            panic!("SOCKS redirect response")
        };
        assert_eq!(reply.metadata["status"], 200);
        assert_eq!(reply.metadata["redirects"], 1);
        assert_eq!(reply.body.as_deref(), Some(b"Acme".as_slice()));
        contexts.restore(id, reply);
        contexts.discard(id).unwrap();
        let facts = site.settled(1).await;
        assert_eq!(facts["connections"].as_array().unwrap().len(), 1);
        assert_eq!(
            facts["origin"],
            json!([
                {"path":path,"host":format!("{host}:PORT"),"authorization":"Bearer Acme origin","proxyAuthorization":"","custom":"origin"},
                {"path":"/echo","host":format!("{second}:PORT"),"authorization":"","proxyAuthorization":"","custom":"origin"}
            ])
        );
    }
}

#[tokio::test]
async fn http_socks_handshake_deadline_and_cancellation_prevent_late_origin_dispatch() {
    let site = endpoint::Site::start(endpoint::Mode::StallGreeting, false).await;
    let url = site.url("http", "localhost", "/echo");
    let mut contexts = Contexts::default();
    let client = context(
        &mut contexts,
        json!({"server":site.server("socks5","")}),
        &url,
    );
    let mut request = fetch(&url);
    request.timeout_ms = Some(80);
    request.max_retries = 2;
    let started = Instant::now();
    let id = contexts
        .start(client, request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    let error = match ready(&mut contexts, id).await {
        Err(error) => error,
        _ => panic!("Held SOCKS greeting must time out"),
    };
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(error.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
    let partial = error.partial.unwrap();
    assert_eq!(partial["failureKind"], "timeout");
    assert_eq!(partial["retryCount"], 0);
    assert!(
        site.settled(1).await["origin"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for scheme in ["socks4", "socks5"] {
        let site = endpoint::Site::start(endpoint::Mode::HoldReply, false).await;
        let url = site.url("http", "localhost", "/echo");
        let client = context(
            &mut contexts,
            json!({"server":site.server(scheme,"Acme:example@")}),
            &url,
        );
        let mut request = fetch(&url);
        request.method = "POST".into();
        request.body = Some(Payload::Text {
            text: "Acme private body".into(),
        });
        let id = contexts
            .start(client, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        site.handshake().await;
        let cancelled = Instant::now();
        contexts.cancel_request(id);
        let error = match ready(&mut contexts, id).await {
            Err(error) => error,
            _ => panic!("Recognized cancellation must prevent origin dispatch"),
        };
        assert_eq!(error.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
        assert!(
            cancelled.elapsed() < Duration::from_secs(2),
            "Cancellation must settle while the peer still withholds its reply"
        );
        site.grant();
        assert_eq!(error.partial.unwrap()["replayed"], false);
        assert!(
            site.settled(1).await["origin"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(site.prefixes().is_empty());
    }
}

#[tokio::test]
async fn http_socks_authentication_and_connect_refusals_never_reach_origin_or_retry() {
    for (scheme, mode) in [
        ("socks5", endpoint::Mode::RefuseAuth),
        ("socks4", endpoint::Mode::RefuseConnect),
        ("socks5", endpoint::Mode::RefuseConnect),
    ] {
        let site = endpoint::Site::start(mode, false).await;
        let url = site.url("http", "localhost", "/echo");
        let mut contexts = Contexts::default();
        let client = context(
            &mut contexts,
            json!({"server":site.server(scheme,"Acme:example@")}),
            &url,
        );
        let mut request = fetch(&url);
        request.max_retries = 2;
        let id = contexts
            .start(client, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        let error = match ready(&mut contexts, id).await {
            Err(error) => error,
            _ => panic!("SOCKS rejection must fail"),
        };
        assert_eq!(error.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
        let partial = error.partial.unwrap();
        assert_eq!(partial["connectionReset"], false);
        assert_eq!(partial["retryCount"], 0);
        assert_eq!(partial["replayed"], false);
        assert!(
            site.settled(1).await["origin"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(site.prefixes().is_empty());
    }
}

#[tokio::test]
async fn http_socks_destination_tls_keeps_omission_and_explicit_false_distinct() {
    for (default, requested, success) in [
        (None, None, false),
        (Some(true), Some(false), false),
        (Some(false), Some(true), true),
        (Some(true), None, true),
    ] {
        let site = endpoint::Site::start(endpoint::Mode::Forward, true).await;
        let url = site.url("https", "localhost", "/echo");
        let mut contexts = Contexts::default();
        let options: Options = serde_json::from_value(
            json!({"proxy":{"server":site.server("socks5","")},"ignoreHttpsErrors":default}),
        )
        .unwrap();
        let client: Reference = serde_json::from_value(
            contexts.create(Uuid::new_v4(), options).unwrap()["httpContext"].clone(),
        )
        .unwrap();
        let mut request = fetch(&url);
        request.read_body = true;
        request.ignore_https_errors = requested;
        let id = contexts
            .start(client, request, Instant::now() + Duration::from_secs(5))
            .unwrap();
        match ready(&mut contexts, id).await {
            Ok(Ready::Reply(reply)) => {
                assert!(success);
                assert_eq!(reply.metadata["status"], 200);
                assert_eq!(reply.body.as_deref(), Some(b"Acme".as_slice()));
                contexts.restore(id, reply);
                contexts.discard(id).unwrap();
            }
            Err(error) => {
                assert!(!success);
                assert_eq!(error.partial.unwrap()["failureKind"], "tls");
            }
            _ => panic!("Unsettled TLS response"),
        }
        let facts = site.settled(1).await;
        assert_eq!(
            facts["origin"].as_array().unwrap().len(),
            usize::from(success)
        );
        assert_eq!(
            site.prefixes(),
            vec![22],
            "Every SOCKS HTTPS destination must start TLS"
        );
    }
}

#[test]
fn http_socks_preparation_keeps_remote_resolution_and_credentials_private() {
    for (scheme, protocol) in [
        ("socks", "Socks5h"),
        ("socks5", "Socks5h"),
        ("socks5h", "Socks5h"),
        ("socks4", "Socks4A"),
        ("socks4a", "Socks4A"),
    ] {
        let input:Proxy=serde_json::from_value(json!({"server":format!("{scheme}://Acme%3Auser:p%40ss@localhost:1081/ignored"),"username":"Acme ignored","password":"ignored example"})).unwrap();
        let prepared = input
            .prepare()
            .expect("Pinned SOCKS HTTP settings must be supported");
        assert_eq!(format!("{:?}", prepared.proxy.protocol()), protocol);
        assert!(
            !prepared.proxy.resolve_target(),
            "The pinned helper delegates destination names"
        );
        assert!(prepared.proxy.username().is_none());
        assert!(prepared.proxy.password().is_none());
        let private = format!("{input:?} {:?} {:?}", prepared.proxy, prepared.socks);
        for secret in ["Acme", "p@ss", "ignored"] {
            assert!(!private.contains(secret));
        }
    }
}

#[test]
fn http_socks_preparation_bounds_decoded_utf8_credentials_and_uses_default_port_for_zero() {
    for scheme in ["socks4", "socks5"] {
        for port in ["", ":0", ":1080"] {
            let input: Proxy =
                serde_json::from_value(json!({"server":format!("{scheme}://localhost{port}")}))
                    .unwrap();
            assert_eq!(input.prepare().unwrap().proxy.port(), 1080);
        }
        for (field, bytes) in [("a".repeat(255), 255), ("é".repeat(128), 256)] {
            for user in [true, false] {
                let credentials = if user {
                    format!("{field}:example")
                } else {
                    format!("Acme:{field}")
                };
                let input: Proxy = serde_json::from_value(
                    json!({"server":format!("{scheme}://{credentials}@localhost:1080")}),
                )
                .unwrap();
                if bytes == 255 {
                    assert!(input.prepare().is_ok());
                } else {
                    assert_eq!(input.prepare().err().unwrap().code, "HTTP_PROXY_INVALID");
                }
            }
        }
    }
    let input: Proxy =
        serde_json::from_value(json!({"server":"socks4://Acme%00user:example@localhost:1080"}))
            .unwrap();
    assert_eq!(input.prepare().err().unwrap().code, "HTTP_PROXY_INVALID");
}
