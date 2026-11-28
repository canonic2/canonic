//! Engine defaults come from the owned browser once, never page expressions.
use super::*;
use crate::http_fixtures::defaults as wire;

fn queued(session: &Session, request: Value) -> Uuid {
    session
        .submit(
            Uuid::new_v4(),
            serde_json::from_value(request).unwrap(),
            5000,
        )
        .unwrap()
}
async fn successful(session: &Session, request: Value) -> Value {
    let result = complete(session, queued(session, request)).await;
    assert_eq!(
        result.outcome,
        Outcome::Succeeded,
        "{result:?}; {:?}",
        session.private_failures()
    );
    result.result.unwrap()
}

#[tokio::test]
async fn http_user_agent_sdk_defaults_cache_native_metadata_and_preserve_overrides_on_the_wire() {
    for engine in [Engine::Firefox, Engine::Webkit] {
        let site = wire::Site::start(None).await;
        let (session, mut peer, _root, context, _page) = harness().await;
        session.0.host.state.lock().unwrap().engine = engine;
        for (index, (http_agent, browser_agent, expected)) in [
            (None, None, "Acme engine agent"),
            (None, None, "Acme engine agent"),
            (Some(""), None, "Acme engine agent"),
            (Some("Acme HTTP agent"), None, "Acme HTTP agent"),
            (None, Some("Acme context agent"), "Acme context agent"),
            (Some(""), Some("Acme context agent"), "Acme context agent"),
        ]
        .into_iter()
        .enumerate()
        {
            session.0.host.state.lock().unwrap().context_options.insert(
                context,
                ContextOptions {
                    user_agent: browser_agent.map(String::from),
                    ignore_https_errors: Some(true),
                    ..Default::default()
                },
            );
            let creating = queued(
                &session,
                json!({"operation":"http_context_create","options":{"browserContext":context,"baseUrl":site.url,"allowUnknownCookiePartitions":engine==Engine::Firefox,"userAgent":http_agent,"ignoreHttpsErrors":false}}),
            );
            if index == 0 {
                let query = peer.request().await;
                assert_eq!(query["guid"], "browser");
                assert_eq!(query["method"], "defaultUserAgentForTest");
                assert_eq!(query["metadata"]["internal"], true);
                peer.send(&json!({"id":query["id"],"result":{"userAgent":"Acme engine agent"}}))
                    .await;
            }
            let created = complete(&session, creating).await;
            assert_eq!(
                created.outcome,
                Outcome::Succeeded,
                "{created:?}; {:?}",
                session.private_failures()
            );
            let http = created.result.unwrap()["httpContext"].clone();
            let listed = session.0.host.http_contexts.lock().unwrap().list();
            assert_eq!(
                listed["contexts"][0]["tlsVerification"], true,
                "Explicit TLS false must survive inherited true"
            );
            for override_agent in [None, Some("Acme request agent"), None] {
                let headers = override_agent
                    .map(|agent| json!([{"name":"uSeR-aGeNt","value":agent}]))
                    .unwrap_or(json!([]));
                let fetching = queued(
                    &session,
                    json!({"operation":"http_fetch","httpContext":http,"request":{"url":"/echo","headers":headers}}),
                );
                let cookies = peer.request().await;
                assert_eq!(
                    cookies["method"], "cookies",
                    "Warm contexts must not query identity again or evaluate a page: {cookies}"
                );
                peer.send(&json!({"id":cookies["id"],"result":{"cookies":[]}}))
                    .await;
                let result = complete(&session, fetching).await;
                assert_eq!(
                    result.outcome,
                    Outcome::Succeeded,
                    "{result:?}; {:?}",
                    session.private_failures()
                );
                assert_eq!(
                    site.records().last().unwrap()["headers"]["user-agent"],
                    override_agent.unwrap_or(expected)
                );
                successful(&session,json!({"operation":"http_response_remove","requestId":result.result.unwrap()["requestId"]})).await;
            }
            successful(
                &session,
                json!({"operation":"http_context_remove","httpContext":http}),
            )
            .await;
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(25), peer.request())
                .await
                .is_err()
        );
        close(&session).await;
    }
}

#[tokio::test]
async fn http_user_agent_sdk_partition_refusal_does_not_probe_and_context_loss_fences_creation() {
    let (session, mut peer, _root, context, _page) = harness().await;
    session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .context_options
        .insert(context, ContextOptions::default());
    let refused = complete(
        &session,
        queued(
            &session,
            json!({"operation":"http_context_create","options":{"browserContext":context}}),
        ),
    )
    .await;
    assert_eq!(
        refused.code.as_deref(),
        Some("HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    let unsupported=complete(&session,queued(&session,json!({"operation":"http_context_create","options":{"browserContext":context,"cookiePartition":{"topLevelSite":"https://example.com"}}}))).await;
    assert_eq!(
        unsupported.code.as_deref(),
        Some("HTTP_COOKIE_PARTITION_UNSUPPORTED")
    );
    session.0.host.state.lock().unwrap().engine = Engine::Webkit;
    let ancestry=complete(&session,queued(&session,json!({"operation":"http_context_create","options":{"browserContext":context,"cookiePartition":{"topLevelSite":"https://example.com","hasCrossSiteAncestor":false}}}))).await;
    assert_eq!(
        ancestry.code.as_deref(),
        Some("HTTP_COOKIE_PARTITION_ANCESTRY_UNSUPPORTED")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    session.0.host.state.lock().unwrap().engine = Engine::Firefox;
    let creating = queued(
        &session,
        json!({"operation":"http_context_create","options":{"browserContext":context,"allowUnknownCookiePartitions":true}}),
    );
    let query = peer.request().await;
    assert_eq!(query["method"], "defaultUserAgentForTest");
    peer.send(&json!({"guid":"context","method":"close","params":{}}))
        .await;
    peer.send(&json!({"id":query["id"],"result":{"userAgent":"Acme engine agent"}}))
        .await;
    let result = complete(&session, creating).await;
    assert_eq!(
        result.code.as_deref(),
        Some("STALE_REFERENCE"),
        "{result:?}"
    );
    assert_eq!(
        session.0.host.http_contexts.lock().unwrap().list()["contexts"],
        json!([])
    );
    close(&session).await;
}
