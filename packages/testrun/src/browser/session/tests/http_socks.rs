//! SDK-backed operation cancellation must reach its owned pending HTTP handshake.
use super::*;
use crate::browser::socks_fixture as endpoint;

#[tokio::test]
async fn http_socks_sdk_cancellation_settles_before_a_held_proxy_reply_and_prevents_origin_dispatch()
 {
    for engine in [Engine::Firefox, Engine::Webkit] {
        for scheme in ["socks4", "socks5"] {
            let site = endpoint::Site::start(endpoint::Mode::HoldReply, false).await;
            let (session, _peer, _root, _context, _page) = harness().await;
            session.0.host.state.lock().unwrap().engine = engine;
            let created=session.submit(Uuid::new_v4(),serde_json::from_value(json!({"operation":"http_context_create","options":{"proxy":{"server":site.server(scheme,"Acme:example@")},"userAgent":"Acme"}})).unwrap(),5000).unwrap();
            let created = complete(&session, created).await;
            assert_eq!(created.outcome, Outcome::Succeeded, "{created:?}");
            let client = created.result.unwrap()["httpContext"].clone();
            let pending=session.submit(Uuid::new_v4(),serde_json::from_value(json!({"operation":"http_fetch","httpContext":client,"request":{"url":site.url("http","localhost","/echo"),"method":"POST","body":{"kind":"text","text":"Acme private body"}}})).unwrap(),5000).unwrap();
            site.handshake().await;
            session.cancel(pending).await.unwrap();
            let result = tokio::time::timeout(Duration::from_secs(2), complete(&session, pending))
                .await
                .expect("Recognized cancellation must settle before the peer replies");
            assert_eq!(result.outcome, Outcome::Cancelled, "{result:?}");
            assert!(result.cancel_requested);
            site.grant();
            let facts = site.settled(1).await;
            assert!(facts["origin"].as_array().unwrap().is_empty());
            assert!(site.prefixes().is_empty());
            let sibling = endpoint::Site::start(endpoint::Mode::Forward, false).await;
            let created=session.submit(Uuid::new_v4(),serde_json::from_value(json!({"operation":"http_context_create","options":{"proxy":{"server":sibling.server(scheme,"")},"userAgent":"Acme"}})).unwrap(),5000).unwrap();
            let created = complete(&session, created).await;
            assert_eq!(created.outcome, Outcome::Succeeded);
            let fetch=session.submit(Uuid::new_v4(),serde_json::from_value(json!({"operation":"http_fetch","httpContext":created.result.unwrap()["httpContext"],"request":{"url":sibling.url("http","localhost","/echo")}})).unwrap(),5000).unwrap();
            let fetched = complete(&session, fetch).await;
            assert_eq!(fetched.outcome, Outcome::Succeeded, "{fetched:?}");
            assert_eq!(
                sibling.settled(1).await["origin"].as_array().unwrap().len(),
                1
            );
            session.0.host.close_http().unwrap();
            assert!(
                session
                    .0
                    .host
                    .http_contexts
                    .lock()
                    .unwrap()
                    .pending()
                    .is_empty()
            );
        }
    }
}

#[tokio::test]
async fn http_socks_sdk_cancelled_default_probe_does_not_allocate_a_late_http_client() {
    let (session, mut peer, _root, context, _page) = harness().await;
    session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .context_options
        .insert(context, ContextOptions::default());
    let creating=session.submit(Uuid::new_v4(),serde_json::from_value(json!({"operation":"http_context_create","options":{"browserContext":context,"proxy":{"server":"socks5://127.0.0.1:1080"},"allowUnknownCookiePartitions":true}})).unwrap(),5000).unwrap();
    let probe = peer.request().await;
    assert_eq!(probe["method"], "defaultUserAgentForTest");
    session.cancel(creating).await.unwrap();
    peer.send(&json!({"id":probe["id"],"result":{"userAgent":"Acme owned engine"}}))
        .await;
    let result = complete(&session, creating).await;
    assert_eq!(result.outcome, Outcome::Cancelled, "{result:?}");
    assert!(
        session.0.host.http_contexts.lock().unwrap().list()["contexts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    session.0.host.close_http().unwrap();
}
