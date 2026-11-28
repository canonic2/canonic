//! Configuration uncertainty fences inherited clients without replaying setters.
use super::*;
use crate::http_fixtures::defaults as wire;

fn submit(session: &Session, value: Value) -> Uuid {
    session
        .submit(Uuid::new_v4(), serde_json::from_value(value).unwrap(), 5000)
        .unwrap()
}

#[tokio::test]
async fn page_header_defaults_sdk_uses_exact_page_replaces_resets_and_retains_original_errors() {
    let (session, mut peer, _root, context, page) = harness().await;
    for headers in [
        json!([{"name":"X-Acme","value":"private-page-value"}]),
        json!([]),
    ] {
        let id = submit(
            &session,
            json!({"operation":"page_set_extra_http_headers","page":page,"headers":headers}),
        );
        let call = peer.request().await;
        assert_eq!(call["guid"], "page");
        assert_eq!(call["method"], "setExtraHTTPHeaders");
        assert_eq!(call["params"]["headers"], headers);
        peer.send(&json!({"id":call["id"],"result":{}})).await;
        let done = complete(&session, id).await;
        assert_eq!(done.outcome, Outcome::Succeeded);
        assert!(
            !done
                .result
                .unwrap()
                .to_string()
                .contains("private-page-value")
        );
    }
    let host = &session.0.host;
    assert_eq!(
        host.page_set_extra_http_headers(context, vec![], deadline())
            .await
            .unwrap_err()
            .code,
        "INVALID_SCOPE"
    );
    assert!(
        host.page_set_extra_http_headers(
            page,
            vec![crate::browser::http::Header {
                name: "X-Acme".into(),
                value: "bad\r\nvalue".into()
            }],
            deadline()
        )
        .await
        .is_err()
    );
    let not_sent = host
        .page_set_extra_http_headers(page, vec![], Instant::now())
        .await
        .unwrap_err();
    assert_eq!(not_sent.partial.unwrap()["stateMayHaveChanged"], false);
    let id = submit(
        &session,
        json!({"operation":"page_set_extra_http_headers","page":page,"headers":[]}),
    );
    let call = peer.request().await;
    peer.send(&method_error(call["id"].clone())).await;
    assert_eq!(complete(&session, id).await.outcome, Outcome::Unknown);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err(),
        "No setter replay after an uncertain SDK reply"
    );
    close(&session).await;
}

#[tokio::test]
async fn page_header_defaults_sdk_late_reply_preserves_closed_scope() {
    let (session, mut peer, _root, _context, page) = harness().await;
    let id = submit(
        &session,
        json!({"operation":"page_set_extra_http_headers","page":page,"headers":[]}),
    );
    let call = peer.request().await;
    peer.send(&json!({"guid":"page","method":"close","params":{}}))
        .await;
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    assert_eq!(
        complete(&session, id).await.result.unwrap()["scopeEnded"],
        true
    );
    let id = submit(
        &session,
        json!({"operation":"page_set_extra_http_headers","page":page,"headers":[]}),
    );
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("STALE_REFERENCE")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}
async fn fetch_headers(session: &Session, peer: &mut Fixture, client: &Value) {
    let id = submit(
        session,
        json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/echo"}}),
    );
    let cookies = peer.request().await;
    assert_eq!(cookies["method"], "cookies");
    peer.send(&json!({"id":cookies["id"],"result":{"cookies":[]}}))
        .await;
    let response = complete(session, id).await;
    assert_eq!(response.outcome, Outcome::Succeeded, "{response:?}");
    let remove = submit(
        session,
        json!({"operation":"http_response_remove","requestId":response.result.unwrap()["requestId"]}),
    );
    assert_eq!(complete(session, remove).await.outcome, Outcome::Succeeded);
}
#[tokio::test]
async fn http_header_defaults_sdk_live_inheritance_follows_the_rejected_setter_rollback() {
    let site = wire::Site::start(None).await;
    let (session, mut peer, _root, context, _) = harness().await;
    let initial = vec![crate::browser::http::Header {
        name: "X-Acme".into(),
        value: "initial".into(),
    }];
    session.0.host.state.lock().unwrap().context_options.insert(
        context,
        ContextOptions {
            base_url: Some(site.url.clone()),
            user_agent: Some("Acme".into()),
            extra_http_headers: Some(initial),
            ..Default::default()
        },
    );
    let create = submit(
        &session,
        json!({"operation":"http_context_create","options":{"browserContext":context,"allowUnknownCookiePartitions":true}}),
    );
    let created = complete(&session, create).await;
    assert_eq!(created.outcome, Outcome::Succeeded, "{created:?}");
    let client = created.result.unwrap()["httpContext"].clone();
    let not_sent = session
        .0
        .host
        .context_configure(
            context,
            crate::browser::settings::ContextSetting::ExtraHttpHeaders { headers: vec![] },
            Instant::now(),
        )
        .await
        .unwrap_err();
    assert_eq!(not_sent.partial.unwrap()["stateMayHaveChanged"], false);
    fetch_headers(&session, &mut peer, &client).await;
    assert_eq!(
        site.records().last().unwrap()["headers"]["x-acme"],
        "initial"
    );
    let configure = json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[{"name":"X-Acme","value":"changed"}]}});
    let id = submit(&session, configure.clone());
    let call = peer.request().await;
    assert_eq!(call["method"], "setExtraHTTPHeaders");
    assert_eq!(call["params"]["headers"][0]["value"], "changed");
    peer.send(&method_error(call["id"].clone())).await;
    let failed = complete(&session, id).await;
    assert_eq!(failed.outcome, Outcome::Unknown);
    assert!(
        !failed.result.unwrap().to_string().contains("changed"),
        "Header values must not enter receipts"
    );
    // The rejected pinned setter restored its previous option.
    fetch_headers(&session, &mut peer, &client).await;
    assert_eq!(
        site.records().last().unwrap()["headers"]["x-acme"],
        "initial"
    );
    assert_eq!(site.records().len(), 2);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err(),
        "The rejected setter is not replayed"
    );
    let id = submit(&session, configure);
    let call = peer.request().await;
    assert_eq!(
        call["method"], "setExtraHTTPHeaders",
        "A later change is one explicit setter"
    );
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    let completed = complete(&session, id).await;
    assert_eq!(completed.outcome, Outcome::Succeeded);
    assert_eq!(
        completed.result.unwrap()["requested"],
        json!({"kind":"extra_http_headers","headerCount":1})
    );
    fetch_headers(&session, &mut peer, &client).await;
    assert_eq!(
        site.records().last().unwrap()["headers"]["x-acme"],
        "changed"
    );
    let id = submit(
        &session,
        json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[]}}),
    );
    let call = peer.request().await;
    assert_eq!(call["params"]["headers"], json!([]));
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    assert_eq!(complete(&session, id).await.outcome, Outcome::Succeeded);
    fetch_headers(&session, &mut peer, &client).await;
    assert!(
        site.records().last().unwrap()["headers"]
            .get("x-acme")
            .is_none()
    );
    let remove = submit(
        &session,
        json!({"operation":"http_context_remove","httpContext":client}),
    );
    assert_eq!(complete(&session, remove).await.outcome, Outcome::Succeeded);
    close(&session).await;
}

#[tokio::test]
async fn http_header_defaults_sdk_closed_context_is_not_revived_by_late_configuration_reply() {
    let (session, mut peer, _root, context, _) = harness().await;
    session
        .0
        .host
        .state
        .lock()
        .unwrap()
        .context_options
        .insert(context, ContextOptions::default());
    let id = submit(
        &session,
        json!({"operation":"context_configure","context":context,"setting":{"kind":"extra_http_headers","headers":[]}}),
    );
    let call = peer.request().await;
    peer.send(&json!({"guid":"context","method":"close","params":{}}))
        .await;
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    let response = complete(&session, id).await;
    assert_eq!(response.outcome, Outcome::Succeeded);
    assert_eq!(response.result.unwrap()["scopeEnded"], true);
    let create = submit(
        &session,
        json!({"operation":"http_context_create","options":{"browserContext":context,"allowUnknownCookiePartitions":true}}),
    );
    assert_eq!(
        complete(&session, create).await.code.as_deref(),
        Some("STALE_REFERENCE")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}
