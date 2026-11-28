//! Disposal changes public lifetime without hiding actual workers or deleting evidence.
use super::*;
use crate::http_fixtures::concurrency as wire;

struct Browser;
impl browser::BrowserCookies for Browser {
    fn partition_identity_available(&self) -> bool {
        true
    }
    fn read(&self, _: Instant) -> Result<Value, Error> {
        Ok(json!([]))
    }
    fn write(&self, _: Value, _: Instant) -> Result<(), Error> {
        Ok(())
    }
}
fn associated(contexts: &mut Contexts, connection: Uuid, base: &str) -> (Reference, Reference) {
    let browser = Reference {
        connection_id: connection,
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let result = contexts
        .create_with_browser(
            connection,
            Options {
                browser_context: Some(browser),
                base_url: Some(base.into()),
                ..Default::default()
            },
            Some(Arc::new(Browser)),
        )
        .unwrap();
    (
        serde_json::from_value(result["httpContext"].clone()).unwrap(),
        browser,
    )
}
fn reason(value: &str) -> DisposeOptions {
    DisposeOptions {
        reason: Some(value.into()),
    }
}
fn capture(contexts: &mut Contexts, id: Uuid, reply: Reply, directory: &Path) -> Value {
    let artifact = materialize(directory, &reply).unwrap();
    contexts.complete(id, reply.metadata, artifact).unwrap()
}
#[test]
fn http_disposal_reason_privacy_repeated_disposal_and_actual_filter_ownership() {
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), "http://127.0.0.1:9");
    let owner = contexts.get(client).unwrap();
    let lease = owner.filter_lease().unwrap();
    let first = contexts
        .dispose(client, reason("Acme-private-first"))
        .unwrap();
    assert_eq!(first["repeated"], false);
    assert_eq!(first["active"], true);
    assert!(lease.cancelled.load(Ordering::SeqCst));
    assert_eq!(
        contexts.remove(client).unwrap_err().code,
        "HTTP_CONTEXT_BUSY"
    );
    assert_eq!(
        contexts.get(client).err().unwrap().code,
        "HTTP_CONTEXT_DISPOSED"
    );
    assert_eq!(
        contexts.cookie_filter_lease(client).err().unwrap().code,
        "HTTP_CONTEXT_DISPOSED"
    );
    assert_eq!(
        contexts.clear_cookies(client).unwrap_err().code,
        "HTTP_CONTEXT_DISPOSED"
    );
    assert_eq!(
        contexts.cookies_read(client, &[]).unwrap_err().code,
        "HTTP_CONTEXT_DISPOSED"
    );
    assert!(!contexts.list().to_string().contains("Acme-private"));
    assert!(
        !contexts
            .context_read(client, false, 65536)
            .unwrap()
            .to_string()
            .contains("Acme-private")
    );
    let second = contexts
        .dispose(client, reason("Acme-private-second"))
        .unwrap();
    assert_eq!(second["repeated"], true);
    let detail = contexts.context_read(client, true, 65536).unwrap();
    assert_eq!(detail["initialReason"], "Acme-private-first");
    assert_eq!(detail["fetchReason"], "Acme-private-second");
    assert_eq!(
        contexts.context_read(client, true, 1).unwrap_err().code,
        "HTTP_RESPONSE_RESULT_BOUNDS"
    );
    assert_eq!(
        contexts
            .dispose(client, reason(&"x".repeat(65537)))
            .unwrap_err()
            .code,
        "HTTP_DISPOSE_REASON_BOUNDS"
    );
    assert_eq!(contexts.context_read(client, true, 65536).unwrap(), detail);
    let mut foreign = client;
    foreign.connection_id = Uuid::new_v4();
    assert_eq!(
        contexts
            .dispose(foreign, DisposeOptions::default())
            .unwrap_err()
            .code,
        "FOREIGN_REFERENCE"
    );
    contexts.dispose(client, DisposeOptions::default()).unwrap();
    let detail = contexts.context_read(client, true, 65536).unwrap();
    assert_eq!(detail["initialReason"], "Acme-private-first");
    assert!(detail["fetchReason"].is_null());
    assert_eq!(contexts.close().unwrap_err().code, "CLEANUP_PENDING");
    drop(lease);
    assert_eq!(contexts.remove(client).unwrap()["removed"], true);
    assert_eq!(
        contexts
            .dispose(client, DisposeOptions::default())
            .unwrap_err()
            .code,
        "STALE_REFERENCE"
    );
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_disposal_isolated_cancels_only_its_owned_requests_without_replay() {
    let mut site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    let client = create(&mut contexts, connection, &site.url);
    let sibling = create(&mut contexts, connection, &site.url);
    let deadline = Instant::now() + Duration::from_secs(5);
    let a = contexts.start(client, fetch("/A"), deadline).unwrap();
    let watch = contexts.disposal_watch(a).unwrap();
    let b = contexts.start(client, fetch("/B"), deadline).unwrap();
    let c = contexts.start(sibling, fetch("/C"), deadline).unwrap();
    let mut held = site.held(3).await;
    assert_eq!(
        contexts.dispose(client, reason("Acme")).unwrap()["inflightPolicy"],
        "cancel-owned"
    );
    assert!(contexts.disposed_request(a));
    assert!(contexts.disposed_request(b));
    assert!(!contexts.disposed_request(c));
    assert_eq!(
        contexts
            .start(client, fetch("/never"), deadline)
            .unwrap_err()
            .code,
        "HTTP_CONTEXT_DISPOSED"
    );
    assert_eq!(
        ready(&mut contexts, a).await.err().unwrap().code,
        "HTTP_REQUEST_OUTCOME_UNKNOWN"
    );
    assert_eq!(
        ready(&mut contexts, b).await.err().unwrap().code,
        "HTTP_REQUEST_OUTCOME_UNKNOWN"
    );
    assert!(contexts.ready(c).unwrap().is_none());
    assert_eq!(
        contexts.lookup(client).unwrap().lanes.lock().unwrap().len(),
        0
    );
    held.remove("/C").unwrap().send(()).unwrap();
    let Ready::Reply(reply) = ready(&mut contexts, c).await.unwrap() else {
        panic!("Sibling reply")
    };
    assert_eq!(reply.body.as_deref(), Some(b"/C".as_slice()));
    contexts.complete(c, reply.metadata, None).unwrap();
    assert_eq!(site.count.load(Ordering::SeqCst), 3);
    contexts.remove(client).unwrap();
    contexts.remove(sibling).unwrap();
    contexts.close().unwrap();
    assert!(
        watch.load(Ordering::Acquire),
        "Disposal survives job and client retirement"
    );
}
#[tokio::test]
async fn http_disposal_associated_preserves_dispatched_metadata_and_explicit_evidence() {
    let mut site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let (client, browser) = associated(&mut contexts, Uuid::new_v4(), &site.url);
    let deadline = Instant::now() + Duration::from_secs(5);
    let a = contexts.start(client, fetch("/A"), deadline).unwrap();
    let mut held = site.held(1).await;
    assert_eq!(
        contexts.dispose(client, reason("Acme")).unwrap()["inflightPolicy"],
        "continue-dispatched"
    );
    assert!(!contexts.disposed_request(a));
    assert!(contexts.ready(a).unwrap().is_none());
    assert_eq!(
        contexts
            .start(client, fetch("/never"), deadline)
            .unwrap_err()
            .code,
        "HTTP_CONTEXT_DISPOSED"
    );
    held.remove("/A").unwrap().send(()).unwrap();
    let Ready::Reply(reply) = ready(&mut contexts, a).await.unwrap() else {
        panic!("Associated metadata")
    };
    assert_eq!(reply.metadata["status"], 201);
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    crate::artifacts::directory(&directory).unwrap();
    let response = capture(&mut contexts, a, reply, &directory);
    let details = contexts.response_read(a, 65536).unwrap();
    assert_eq!(details["status"], 201);
    assert_eq!(
        contexts
            .response_body_read(a, BodyFormat::Text, 65536)
            .unwrap_err()
            .code,
        "HTTP_RESPONSE_DISPOSED"
    );
    assert_eq!(
        contexts.response_dispose(a).unwrap_err().code,
        "HTTP_CONTEXT_DISPOSED"
    );
    let evidence = serde_json::from_value(response["body"]["evidenceId"].clone()).unwrap();
    assert_eq!(
        read_response(
            &contexts.evidence_get(evidence).unwrap(),
            BodyFormat::Text,
            65536
        )
        .unwrap()["value"],
        "/A"
    );
    assert_eq!(contexts.response_read(a, 65536).unwrap(), details);
    assert_eq!(site.count.load(Ordering::SeqCst), 1);
    assert_eq!(
        contexts.lookup(client).unwrap().lanes.lock().unwrap().len(),
        1
    );
    contexts.context_defaults_closed(browser);
    assert_eq!(
        contexts.lookup(client).unwrap().lanes.lock().unwrap().len(),
        0
    );
    contexts.remove(client).unwrap();
    assert_eq!(contexts.response_read(a, 65536).unwrap(), details);
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_disposal_response_body_is_independent_from_metadata_and_evidence() {
    let mut site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &site.url);
    let id = contexts
        .start(client, fetch("/A"), Instant::now() + Duration::from_secs(5))
        .unwrap();
    site.held(1).await.remove("/A").unwrap().send(()).unwrap();
    let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Reply")
    };
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    crate::artifacts::directory(&directory).unwrap();
    let response = capture(&mut contexts, id, reply, &directory);
    let before = contexts.response_read(id, 65536).unwrap();
    assert_eq!(
        contexts
            .response_body_read(id, BodyFormat::Text, 65536)
            .unwrap()["value"],
        "/A"
    );
    assert_eq!(contexts.response_dispose(id).unwrap()["disposed"], true);
    assert_eq!(contexts.response_dispose(id).unwrap()["disposed"], true);
    assert_eq!(
        contexts
            .response_body_read(id, BodyFormat::Base64, 65536)
            .unwrap_err()
            .code,
        "HTTP_RESPONSE_DISPOSED"
    );
    assert_eq!(contexts.response_read(id, 65536).unwrap(), before);
    let evidence = serde_json::from_value(response["body"]["evidenceId"].clone()).unwrap();
    assert_eq!(
        read_response(
            &contexts.evidence_get(evidence).unwrap(),
            BodyFormat::Text,
            65536
        )
        .unwrap()["value"],
        "/A"
    );
    contexts.remove(client).unwrap();
    assert_eq!(
        contexts
            .response_body_read(id, BodyFormat::Text, 65536)
            .unwrap_err()
            .code,
        "HTTP_RESPONSE_DISPOSED"
    );
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_disposal_parent_closure_cancels_associated_work_and_keeps_cleanup_pending() {
    let mut site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let (client, browser) = associated(&mut contexts, Uuid::new_v4(), &site.url);
    let id = contexts
        .start(client, fetch("/A"), Instant::now() + Duration::from_secs(5))
        .unwrap();
    let _held = site.held(1).await;
    contexts.context_defaults_closed(browser);
    assert!(contexts.disposed_request(id));
    assert_eq!(contexts.close().unwrap_err().code, "CLEANUP_PENDING");
    assert_eq!(
        ready(&mut contexts, id).await.err().unwrap().code,
        "HTTP_REQUEST_OUTCOME_UNKNOWN"
    );
    assert_eq!(
        contexts.context_read(client, false, 65536).unwrap()["disposed"],
        true
    );
    assert_eq!(
        contexts.lookup(client).unwrap().lanes.lock().unwrap().len(),
        0
    );
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_disposal_closes_idle_keepalive_before_retained_response_finalization() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (closed, observed) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            assert!(header.len() < 16384);
            header.push(stream.read_u8().await.unwrap());
        }
        stream
            .write_all(b"HTTP/1.1 200 Acme\r\nContent-Length: 4\r\n\r\nAcme")
            .await
            .unwrap();
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        closed.send(()).unwrap();
    });
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &url);
    let id = contexts
        .start(
            client,
            fetch("/idle"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Retained response")
    };
    assert!(contexts.has_pending(id));
    assert_eq!(contexts.list()["contexts"][0]["activeRequests"], 0);
    contexts.dispose(client, reason("Acme")).unwrap();
    tokio::time::timeout(Duration::from_secs(2), observed)
        .await
        .unwrap()
        .unwrap();
    server.await.unwrap();
    assert!(
        contexts.has_pending(id),
        "Socket close must not erase retained response ownership"
    );
    assert_eq!(contexts.close().unwrap_err().code, "CLEANUP_PENDING");
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    crate::artifacts::directory(&directory).unwrap();
    let response = capture(&mut contexts, id, reply, &directory);
    let evidence = serde_json::from_value(response["body"]["evidenceId"].clone()).unwrap();
    assert_eq!(
        read_response(
            &contexts.evidence_get(evidence).unwrap(),
            BodyFormat::Text,
            65536
        )
        .unwrap()["value"],
        "Acme"
    );
    assert_eq!(
        contexts
            .response_body_read(id, BodyFormat::Text, 65536)
            .unwrap_err()
            .code,
        "HTTP_RESPONSE_DISPOSED"
    );
    contexts.remove(client).unwrap();
    contexts.close().unwrap();
}
