//! HTTP cleanup retains actual caller publication after blocking worker retirement.
use super::*;
use crate::http_fixtures::concurrency as wire;
#[tokio::test]
async fn http_disposal_context_close_acknowledgement_precedes_lifecycle_event() {
    let (session, mut peer, _directory, context, _page) = harness().await;
    session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .context_options
        .insert(context, ContextOptions::default());
    let created = session.submit(Uuid::new_v4(), serde_json::from_value(json!({
        "operation":"http_context_create",
        "options":{"browserContext":context,"allowUnknownCookiePartitions":true,"userAgent":"Acme HTTP agent"}
    })).unwrap(), 5000).unwrap();
    let created = complete(&session, created).await;
    assert_eq!(created.outcome, Outcome::Succeeded, "{created:?}");
    let client = created.result.unwrap()["httpContext"].clone();
    let closing = session
        .submit(Uuid::new_v4(), Request::ContextClose { context }, 5000)
        .unwrap();
    let call = peer.request().await;
    assert_eq!(call["method"], "close");
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, closing).await.outcome,
        Outcome::Succeeded
    );
    for late_event in [false, true] {
        if late_event {
            peer.send(&json!({"guid":"context","method":"close","params":{}}))
                .await;
        }
        let fetch = session
            .submit(
                Uuid::new_v4(),
                serde_json::from_value(json!({
                    "operation":"http_fetch","httpContext":client,
                    "request":{"url":"http://127.0.0.1:1/never-dispatched"}
                }))
                .unwrap(),
                5000,
            )
            .unwrap();
        let failed = complete(&session, fetch).await;
        assert_eq!(failed.code.as_deref(), Some("HTTP_CONTEXT_DISPOSED"));
        assert_eq!(
            failed.result.unwrap(),
            json!({"httpContext":client,"disposed":true,"replayed":false})
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn http_disposal_cleanup_waits_for_the_public_caller_seal() {
    let (session, peer, _directory, context, _page) = harness().await;
    let mut site = wire::Site::start().await;
    let client = session
        .0
        .host
        .http_contexts
        .lock()
        .unwrap()
        .create(
            context.connection_id,
            crate::browser::http::Options {
                base_url: Some(site.url.clone()),
                ..Default::default()
            },
        )
        .unwrap()["httpContext"]
        .clone();
    let id=session.submit(Uuid::new_v4(),serde_json::from_value(json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/A","readBody":true}})).unwrap(),5000).unwrap();
    let _held = site.held(1).await;
    assert!(session.0.host.http_callers.lock().unwrap().contains(&id));
    assert!(!session.operation(id).unwrap().settled);
    assert_eq!(
        session
            .0
            .host
            .close_http_callers(Instant::now())
            .await
            .unwrap_err()
            .code,
        "CLEANUP_PENDING"
    );
    session.0.host.http_contexts.lock().unwrap().shutdown_work();
    session.0.host.close_http_owned(deadline()).await.unwrap();
    session.0.host.close_http_callers(deadline()).await.unwrap();
    let sealed = session.operation(id).unwrap();
    assert!(sealed.settled);
    assert_eq!(sealed.code.as_deref(), Some("HTTP_CONTEXT_DISPOSED"));
    assert_eq!(sealed.result.as_ref().unwrap()["replayed"], false);
    assert!(session.0.host.http_callers.lock().unwrap().is_empty());
    assert_eq!(site.count.load(Ordering::SeqCst), 1);
    let directory = session.0.host.journal.directory();
    let records = std::fs::read_dir(directory)
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|entry| std::fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .collect::<Vec<_>>();
    assert!(
        records
            .iter()
            .any(|record| record["operationId"] == id.to_string()
                && record["settled"] == true
                && record["code"] == "HTTP_CONTEXT_DISPOSED")
    );
    drop(peer);
}
