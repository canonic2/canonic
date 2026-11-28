//! Simultaneous requests never share a mutable observer or cancellation owner.
use super::*;
use crate::http_fixtures::concurrency as wire;

#[tokio::test]
async fn http_concurrency_keeps_response_heads_cookies_and_active_ownership_separate() {
    let mut site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let context = create(&mut contexts, Uuid::new_v4(), &site.url);
    let a = contexts
        .start(
            context,
            fetch("/A"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let b = contexts
        .start(
            context,
            fetch("/B"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let mut held = site.held(2).await;
    assert_eq!(contexts.list()["contexts"][0]["activeRequests"], 2);
    assert_eq!(
        contexts.cookie_filter_lease(context).err().unwrap().code,
        "HTTP_CONTEXT_BUSY"
    );
    held.remove("/B").unwrap().send(()).unwrap();
    let Ready::Reply(b_reply) = ready(&mut contexts, b).await.unwrap() else {
        panic!("Reply")
    };
    assert_eq!(b_reply.metadata["status"], 202);
    assert_eq!(b_reply.body.as_deref(), Some(b"/B".as_slice()));
    assert_eq!(
        contexts.response_read(b, 65536).unwrap()["wireMetadataAvailable"],
        true
    );
    assert!(
        contexts.response_read(b, 65536).unwrap()["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|header| header["name"] == "X-Acme" && header["value"] == "/B")
    );
    contexts.complete(b, b_reply.metadata, None).unwrap();
    assert_eq!(contexts.list()["contexts"][0]["activeRequests"], 1);
    assert_eq!(
        contexts.remove(context).unwrap_err().code,
        "HTTP_CONTEXT_BUSY"
    );
    held.remove("/A").unwrap().send(()).unwrap();
    let Ready::Reply(a_reply) = ready(&mut contexts, a).await.unwrap() else {
        panic!("Reply")
    };
    assert_eq!(a_reply.metadata["status"], 201);
    assert_eq!(a_reply.body.as_deref(), Some(b"/A".as_slice()));
    assert_eq!(
        contexts.response_read(a, 65536).unwrap()["wireMetadataAvailable"],
        true
    );
    assert!(
        contexts.response_read(a, 65536).unwrap()["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|header| header["name"] == "X-Acme" && header["value"] == "/A")
    );
    contexts.complete(a, a_reply.metadata, None).unwrap();
    assert_eq!(
        contexts.cookies_read(context, &[]).unwrap()["cookies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cookie| cookie["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["AcmeB", "AcmeA"]
    );
    assert_eq!(site.count.load(Ordering::SeqCst), 2);
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_concurrency_cancel_releases_one_worker_without_changing_its_sibling() {
    let mut site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let context = create(&mut contexts, Uuid::new_v4(), &site.url);
    let a = contexts
        .start(
            context,
            fetch("/A"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let b = contexts
        .start(
            context,
            fetch("/B"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let mut held = site.held(2).await;
    contexts.cancel_request(a);
    let error = tokio::time::timeout(Duration::from_secs(2), ready(&mut contexts, a))
        .await
        .unwrap()
        .err()
        .unwrap();
    assert_eq!(error.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
    assert_eq!(
        error.partial.as_ref().unwrap()["requestMayHaveBeenSent"],
        true
    );
    assert_eq!(contexts.list()["contexts"][0]["activeRequests"], 1);
    assert!(contexts.ready(b).unwrap().is_none());
    held.remove("/B").unwrap().send(()).unwrap();
    let Ready::Reply(reply) = ready(&mut contexts, b).await.unwrap() else {
        panic!("Reply")
    };
    assert_eq!(reply.metadata["status"], 202);
    assert_eq!(reply.metadata["replayed"], false);
    contexts.complete(b, reply.metadata, None).unwrap();
    let _ = held.remove("/A").unwrap().send(());
    assert_eq!(site.count.load(Ordering::SeqCst), 2);
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_concurrency_late_cancel_cannot_close_a_reused_transport_lane() {
    let mut site = wire::Site::start().await;
    let mut contexts = Contexts::default();
    let context = create(&mut contexts, Uuid::new_v4(), &site.url);
    let a = contexts
        .start(
            context,
            fetch("/A"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    site.held(1).await.remove("/A").unwrap().send(()).unwrap();
    let Ready::Reply(first) = ready(&mut contexts, a).await.unwrap() else {
        panic!("Reply")
    };
    let c = contexts
        .start(
            context,
            fetch("/C"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let mut held = site.held(1).await;
    contexts.cancel_request(a);
    assert!(contexts.ready(c).unwrap().is_none());
    held.remove("/C").unwrap().send(()).unwrap();
    let Ready::Reply(last) = ready(&mut contexts, c).await.unwrap() else {
        panic!("Reply")
    };
    assert_eq!(last.metadata["status"], 203);
    assert_eq!(last.body.as_deref(), Some(b"/C".as_slice()));
    contexts.complete(a, first.metadata, None).unwrap();
    contexts.complete(c, last.metadata, None).unwrap();
    assert_eq!(
        contexts.get(context).unwrap().lanes.lock().unwrap().len(),
        1
    );
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
}
