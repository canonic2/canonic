//! Bearer protection through the driver: metadata, refusals before the body,
//! verifier outcomes, operation scopes, the handler's grant, listen streams
//! ending at grant expiry and a slow verifier delaying only its own request.
use super::*;
use crate::http::{
    Operation, Rejection, ResourceOptions, ResourceServer, ScopePolicy, TokenVerifier,
};

const RESOURCE: &str = "http://127.0.0.1/mcp";

/// Accepts a few fixed tokens; `acme-slow` waits until released.
struct Verifier {
    slow_started: AtomicBool,
    released: AtomicBool,
    slow_done: AtomicBool,
}

impl TokenVerifier for Verifier {
    fn verify(
        &self,
        token: crate::http::BearerToken<'_>,
        resource: &str,
    ) -> Result<Grant, Rejection> {
        assert_eq!(resource, RESOURCE);
        let ours = || Grant::new(vec![RESOURCE.to_owned()]).with_subject("user-1");
        match token.as_str() {
            "acme-good" => Ok(ours()
                .with_scopes(["acme:read", "acme:list", "acme:notify"])
                .with_expiry(SystemTime::now() + Duration::from_hours(1))),
            "acme-narrow" => Ok(ours().with_scopes(["acme:read"])),
            "acme-brief" => Ok(ours().with_expiry(SystemTime::now() + Duration::from_millis(300))),
            "acme-foreign" => Ok(Grant::new(vec!["https://other.example.com/mcp".to_owned()])),
            "acme-down" => Err(Rejection::Unavailable),
            "acme-slow" => {
                self.slow_started.store(true, Ordering::SeqCst);
                let deadline = Instant::now() + WAIT;
                while !self.released.load(Ordering::SeqCst) && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(5));
                }
                self.slow_done.store(true, Ordering::SeqCst);
                Ok(ours())
            }
            _ => Err(Rejection::invalid()),
        }
    }
}

/// `acme_echo` needs `acme:write`, listing tools `acme:list`, the product
/// notification `acme:notify`, watching `acme://secret` `acme:secret`, and an
/// unknown method `acme:admin`.
struct Policy;

impl ScopePolicy for Policy {
    fn required(&self, operation: &Operation<'_>) -> Vec<String> {
        let scope = |scope: &str| vec![scope.to_owned()];
        match (operation.method, operation.name) {
            ("tools/call", Some("acme_echo")) => scope("acme:write"),
            ("tools/list", _) => scope("acme:list"),
            ("notifications/acme/changed", _) => scope("acme:notify"),
            ("subscriptions/listen", _)
                if operation.params["notifications"]["resourceSubscriptions"]
                    .as_array()
                    .is_some_and(|uris| uris.contains(&json!("acme://secret"))) =>
            {
                scope("acme:secret")
            }
            ("acme/missing", _) => scope("acme:admin"),
            _ => Vec::new(),
        }
    }
}

fn protected() -> (HttpOptions, Arc<Verifier>) {
    let verifier = Arc::new(Verifier {
        slow_started: AtomicBool::new(false),
        released: AtomicBool::new(false),
        slow_done: AtomicBool::new(false),
    });
    let resource = ResourceServer::new(ResourceOptions::new(
        RESOURCE,
        vec!["https://auth.example.com".to_owned()],
    ))
    .unwrap();
    let http = HttpOptions {
        authorization: Some(
            HttpAuthorization::new(resource, Arc::clone(&verifier) as Arc<dyn TokenVerifier>)
                .with_policy(Arc::new(Policy)),
        ),
        ..http()
    };
    (http, verifier)
}

fn bearer(token: &str) -> String {
    format!("Authorization: Bearer {token}\r\n")
}

#[test]
fn protected_requests_are_verified_before_the_body_and_carry_the_grant() {
    let (http, _) = protected();
    let server = Server::start(http, None);
    let metadata = server.exchange(
        b"GET /.well-known/oauth-protected-resource/mcp HTTP/1.1\r\nHost: localhost\r\n\r\n",
    );
    assert_eq!(metadata.status, 200);
    assert_eq!(metadata.json()["resource"], RESOURCE);
    assert_eq!(metadata.header("access-control-allow-origin"), Some("*"));

    // A missing token is refused instead of `100 Continue`: the body is never read.
    let mut stream = server.connect();
    stream
        .write_all(
            b"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
              Accept: application/json, text/event-stream\r\nExpect: 100-continue\r\n\
              Content-Length: 100\r\n\r\n",
        )
        .unwrap();
    let mut reader = BufReader::new(stream);
    assert_eq!(line(&mut reader), "HTTP/1.1 401 Unauthorized");

    let list = |extra: &str| server.exchange(&post("tools/list", &json!(1), json!({}), extra));
    let missing = list("");
    assert_eq!(missing.status, 401);
    assert_eq!(
        missing.header("www-authenticate"),
        Some(
            "Bearer resource_metadata=\"http://127.0.0.1/.well-known/oauth-protected-resource/mcp\""
        )
    );
    assert_eq!(missing.body, Vec::<u8>::new());
    for (token, status) in [("acme-bad", 401), ("acme-foreign", 401), ("acme-down", 503)] {
        let refused = list(&bearer(token));
        assert_eq!(refused.status, status, "{token}");
        assert_eq!(refused.body, Vec::<u8>::new());
    }
    assert!(
        list(&bearer("acme-bad"))
            .header("www-authenticate")
            .unwrap()
            .starts_with("Bearer error=\"invalid_token\"")
    );
    assert_eq!(list(&bearer("acme-good")).status, 200);
    // The session answers `tools/list` itself; the scope refusal replaces that reply.
    let narrow = list(&bearer("acme-narrow"));
    assert_eq!(narrow.status, 403);
    assert_eq!(narrow.body, Vec::<u8>::new());
    assert!(
        narrow
            .header("www-authenticate")
            .unwrap()
            .contains("scope=\"acme:list\"")
    );

    let good = bearer("acme-good");
    let whoami = server.exchange(&call(&json!(2), "acme_whoami", &json!({}), &good));
    assert_eq!(whoami.json()["result"]["content"][0]["text"], "user-1");
    let echo = server.exchange(&call(
        &json!(3),
        "acme_echo",
        &json!({"text":"hi"}),
        &format!("Mcp-Param-Text: hi\r\n{good}"),
    ));
    assert_eq!(echo.status, 403);
    assert_eq!(
        echo.header("www-authenticate"),
        Some(
            "Bearer error=\"insufficient_scope\", scope=\"acme:write\", resource_metadata=\"http://127.0.0.1/.well-known/oauth-protected-resource/mcp\""
        )
    );
    // The session's own refusal (step 12) comes before the operation scopes.
    let missing_method = server.exchange(&post("acme/missing", &json!(4), json!({}), &good));
    assert_eq!(missing_method.status, 404);
    assert_eq!(
        missing_method.json()["error"]["code"],
        rpc::METHOD_NOT_FOUND
    );

    let notification = |extra: &str| {
        let body = br#"{"jsonrpc":"2.0","method":"notifications/acme/changed","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#;
        let mut request = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\n{extra}Content-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        request.extend_from_slice(body);
        server.exchange(&request)
    };
    assert_eq!(notification("").status, 401);
    // The policy applies to notifications too: refused before `202`, never routed.
    let refused = notification(&bearer("acme-narrow"));
    assert_eq!(refused.status, 403);
    assert!(
        refused
            .header("www-authenticate")
            .unwrap()
            .contains("scope=\"acme:notify\"")
    );
    assert_eq!(notification(&good).status, 202);
    assert_eq!(server.next_event(), Ok("notified"));
}

#[test]
fn a_listen_refused_for_its_scopes_releases_its_stream() {
    let (http, _) = protected();
    let server = Server::start(
        http,
        Some(SubscriptionOptions {
            max_streams: 1,
            ..SubscriptionOptions::default()
        }),
    );
    let listen = |id: &str, filter: Value| {
        let mut stream = server.connect();
        stream
            .write_all(&post(
                "subscriptions/listen",
                &json!(id),
                json!({"notifications": filter}),
                &bearer("acme-good"),
            ))
            .unwrap();
        BufReader::new(stream)
    };
    let mut refused = listen(
        "secret",
        json!({"resourceSubscriptions": ["acme://secret"]}),
    );
    let reply = Reply::read(&mut refused);
    assert_eq!(reply.status, 403);
    assert_eq!(reply.header("content-type"), None);
    assert_eq!(reply.body, Vec::<u8>::new());
    assert!(
        reply
            .header("www-authenticate")
            .unwrap()
            .contains("scope=\"acme:secret\"")
    );
    // The only stream slot was released: a permitted listen is acknowledged.
    let mut allowed = listen("tools", json!({"toolsListChanged": true}));
    assert_eq!(Reply::read(&mut allowed).status, 200);
    let acknowledged = event(&mut allowed).unwrap();
    assert_eq!(
        acknowledged["method"],
        "notifications/subscriptions/acknowledged"
    );
}

#[test]
fn listen_streams_end_with_their_result_when_the_grant_expires() {
    let (http, _) = protected();
    let server = Server::start(http, Some(SubscriptionOptions::default()));
    let mut stream = server.connect();
    stream
        .write_all(&post(
            "subscriptions/listen",
            &json!("brief"),
            json!({"notifications":{}}),
            &bearer("acme-brief"),
        ))
        .unwrap();
    let mut reader = BufReader::new(stream);
    assert_eq!(Reply::read(&mut reader).status, 200);
    assert!(event(&mut reader).is_some());
    let ended = event(&mut reader).unwrap();
    assert_eq!(ended["id"], "brief");
    assert_eq!(ended["result"]["resultType"], "complete");
}

#[test]
fn a_slow_verifier_delays_only_its_own_request() {
    let (http, verifier) = protected();
    let server = Server::start(http, None);
    let mut slow = server.connect();
    slow.write_all(&call(
        &json!("slow"),
        "acme_whoami",
        &json!({}),
        &bearer("acme-slow"),
    ))
    .unwrap();
    let deadline = Instant::now() + WAIT;
    while !verifier.slow_started.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "the slow verification never started"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let fast = server.exchange(&call(
        &json!("fast"),
        "acme_whoami",
        &json!({}),
        &bearer("acme-good"),
    ));
    assert_eq!(fast.json()["result"]["content"][0]["text"], "user-1");
    // The fast request finished while the slow verification was still waiting.
    assert!(!verifier.slow_done.load(Ordering::SeqCst));
    verifier.released.store(true, Ordering::SeqCst);
    let slow = Reply::read(&mut BufReader::new(slow));
    assert_eq!(slow.json()["id"], "slow");
}
