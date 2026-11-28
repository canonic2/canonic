//! Proxy observations are independent of the origin and do not expose credential bytes.
use super::*;
use crate::http_fixtures::defaults as site;
#[path = "../../../../tests/fixtures/browser_http_proxy.rs"]
pub(super) mod tunnel;

fn prepared(value: Value) -> super::super::proxy::Prepared {
    serde_json::from_value::<Proxy>(value)
        .unwrap()
        .prepare()
        .unwrap()
}
#[test]
fn http_proxy_normalizes_settings_and_keeps_decoded_credentials_private() {
    use base64::Engine as _;
    for (value, expected) in [
        (
            json!(" http://Acme%3Auser:p%40ss@localhost:8080/private?Acme=secret "),
            Some("Acme:user:p@ss"),
        ),
        (
            json!({"server":"localhost:8080/private","username":"Acme%3Auser","password":"p%40ss"}),
            Some("Acme:user:p@ss"),
        ),
        (
            json!({"server":"http://old:secret@localhost:8080","username":"Acme-new"}),
            Some("Acme-new:"),
        ),
        (
            json!({"server":"http://old:secret@localhost:8080","username":"","password":"ignored"}),
            Some("old:secret"),
        ),
        (
            json!({"server":"localhost:8080","password":"ignored"}),
            None,
        ),
        (
            json!({"server":"localhost:8080","username":"Acme-\u{e9}","password":"private\nvalue"}),
            Some("Acme-\u{e9}:private\nvalue"),
        ),
    ] {
        let input: Proxy = serde_json::from_value(value).unwrap();
        let configured = input.prepare().unwrap();
        assert_eq!(configured.proxy.host(), "localhost");
        assert_eq!(configured.proxy.port(), 8080);
        assert!(configured.proxy.username().is_none());
        assert!(configured.proxy.password().is_none());
        let expected = expected.map(|value| {
            format!(
                "Proxy-Authorization: Basic {}\r\n",
                base64::engine::general_purpose::STANDARD.encode(value)
            )
        });
        assert_eq!(
            configured.authorization.as_ref().map(|value| value.bytes()),
            expected.as_deref().map(str::as_bytes)
        );
        let debug = format!(
            "{input:?} {:?} {:?}",
            configured.proxy, configured.authorization
        );
        for secret in ["Acme-user", "Acme-new", "old:", "secret", "private", "p@ss"] {
            assert!(!debug.contains(secret), "{debug}");
        }
    }
}
#[test]
fn http_proxy_omitted_ports_use_the_pinned_scheme_defaults() {
    // Pinned normalization parses the server as a WHATWG URL; HttpsProxyAgent
    // then connects to the scheme default when the URL has no port.
    for (server, port) in [
        ("localhost", 80),
        ("http://localhost", 80),
        ("https://localhost", 443),
        ("http://localhost:80", 80),
    ] {
        let configured = prepared(json!({ "server": server }));
        assert_eq!(configured.proxy.host(), "localhost", "{server}");
        assert_eq!(configured.proxy.port(), port, "{server}");
    }
}
#[test]
fn http_proxy_bypass_matches_pinned_host_boundaries_and_javascript_trim() {
    for (bypass, url, expected) in [
        ("example.com", "http://example.com:8080/a", true),
        (".example.com", "http://sub.example.com/a", true),
        ("example.com", "http://notexample.com/a", false),
        ("EXAMPLE.COM", "http://example.com/a", false),
        ("*.example.com", "http://sub.example.com/a", false),
        ("*", "http://example.com/a", false),
        ("example.com:8080", "http://example.com:8080/a", false),
        (
            " \u{feff}.example.com\u{feff}, other.test ",
            "http://example.com/a",
            true,
        ),
        ("\u{85}example.com", "http://example.com/a", false),
        ("[::1]", "http://[::1]:8080/a", true),
        ("::1", "http://[::1]:8080/a", false),
        (",", "http://example.com./a", true),
        ("", "http://example.com./a", false),
    ] {
        let proxy = prepared(json!({"server":"localhost:8080","bypass":bypass}));
        assert_eq!(
            proxy.bypasses(&Url::parse(url).unwrap()),
            expected,
            "{bypass:?} {url}"
        );
    }
}
#[test]
fn http_proxy_invalid_fields_and_unsupported_protocols_refuse_before_allocation() {
    let mut contexts = Contexts::default();
    for (proxy, code) in [
        (json!({"server":"http://"}), "HTTP_PROXY_INVALID"),
        (
            json!({"server":"localhost:8080","username":"Acme%zz"}),
            "HTTP_PROXY_INVALID",
        ),
        (
            json!({"server":"localhost:8080","password":"x".repeat(4097)}),
            "HTTP_PROXY_INVALID",
        ),
        (
            json!({"server":"socks6://localhost:8080"}),
            "HTTP_PROXY_UNSUPPORTED",
        ),
    ] {
        let options: Options = serde_json::from_value(json!({"proxy":proxy})).unwrap();
        assert_eq!(
            contexts.create(Uuid::new_v4(), options).unwrap_err().code,
            code
        );
        assert!(contexts.items.is_empty());
    }
    assert!(
        serde_json::from_value::<Options>(
            json!({"proxy":{"server":"localhost:8080","unknown":true}})
        )
        .is_err()
    );
}
async fn request(contexts: &mut Contexts, context: Reference, path: &str) -> Value {
    let id = contexts
        .start(
            context,
            Fetch {
                read_body: true,
                ..fetch(path)
            },
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    let Ready::Reply(reply) = ready(contexts, id).await.unwrap() else {
        panic!("Retained response")
    };
    assert_eq!(reply.metadata["status"], 200);
    let value = serde_json::from_slice(reply.body.as_ref().unwrap()).unwrap();
    assert!(!reply.metadata.to_string().contains("Acme-proxy-private"));
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
    value
}
#[tokio::test]
async fn http_proxy_private_basic_reaches_only_connect_and_each_request_owns_its_tunnel() {
    use base64::Engine as _;
    let site = site::Site::start(None).await;
    let port = Url::parse(&site.url).unwrap().port().unwrap();
    let auth = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("Acme-\u{e9}:Acme-proxy-private@pass")
    );
    let proxy = tunnel::Proxy::start(vec![port], Some(auth.clone())).await;
    let mut contexts = Contexts::default();
    let options:Options=serde_json::from_value(json!({"baseUrl":site.url,"proxy":{"server":proxy.url,"username":"Acme-\u{e9}","password":"Acme-proxy-private@pass"}})).unwrap();
    let context = serde_json::from_value(
        contexts.create(Uuid::new_v4(), options).unwrap()["httpContext"].clone(),
    )
    .unwrap();
    for _ in 0..2 {
        let response = request(&mut contexts, context, "/proxy-keepalive").await;
        assert!(response["headers"].get("proxy-authorization").is_none());
        assert_eq!(response["headers"]["host"], format!("127.0.0.1:{port}"));
    }
    let requests = proxy.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        site.records().len(),
        2,
        "Proxy setup must not probe the origin"
    );
    assert!(
        requests
            .iter()
            .all(|request| request["authorization"] == auth)
    );
    for request in &requests {
        assert_eq!(request["headers"]["proxy-connection"], "close");
        assert_eq!(request["headers"]["host"], format!("127.0.0.1:{port}"));
        assert!(request["headers"].get("user-agent").is_none());
    }
    assert!(!contexts.list().to_string().contains("Acme-proxy-private"));
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_proxy_bypass_is_selected_again_for_each_redirect_destination() {
    let other = site::Site::start(None).await;
    let site = site::Site::start(Some(other.url.replace("127.0.0.1", "localhost"))).await;
    let reverse = site::Site::start(Some(other.url.clone())).await;
    let ports = [&site.url, &other.url, &reverse.url]
        .map(|url| Url::parse(url).unwrap().port().unwrap())
        .to_vec();
    let proxy = tunnel::Proxy::start(ports, None).await;
    let mut contexts = Contexts::default();
    for (base, expected_target) in [
        (
            site.url.clone(),
            site.url.strip_prefix("http://").unwrap().to_owned(),
        ),
        (
            reverse.url.replace("127.0.0.1", "localhost"),
            other.url.strip_prefix("http://").unwrap().to_owned(),
        ),
    ] {
        let bypass = "localhost";
        let options: Options = serde_json::from_value(
            json!({"baseUrl":base,"proxy":{"server":proxy.url,"bypass":bypass,"username":"Acme-proxy-private","password":"Acme-private-pass"}}),
        )
        .unwrap();
        let context = serde_json::from_value(
            contexts
                .create(contexts.connection.unwrap_or_else(Uuid::new_v4), options)
                .unwrap()["httpContext"]
                .clone(),
        )
        .unwrap();
        let before = proxy.requests().len();
        let response = request(&mut contexts, context, "/cross").await;
        assert!(response["headers"].get("proxy-authorization").is_none());
        let requests = proxy.requests();
        assert_eq!(requests.len(), before + 1);
        assert_eq!(requests.last().unwrap()["target"], expected_target);
        assert!(
            requests.last().unwrap()["authorization"]
                .as_str()
                .is_some_and(|value| value.starts_with("Basic "))
        );
        contexts.remove(context).unwrap();
    }
    contexts.close().unwrap();
}
