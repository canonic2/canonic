//! Exact setter dispatch, uncertain inheritance and late context retirement.
use super::*;
use crate::http_fixtures::defaults as wire;
fn submit(session: &Session, value: Value) -> Uuid {
    session
        .submit(Uuid::new_v4(), serde_json::from_value(value).unwrap(), 5000)
        .unwrap()
}
fn credential(password: &str) -> Value {
    json!({"username":"Acme","password":password})
}
async fn fetch(
    session: &Session,
    peer: &mut Fixture,
    client: &Value,
    cookie_calls: usize,
) -> Operation {
    let id = submit(
        session,
        json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/auth/basic"}}),
    );
    for _ in 0..cookie_calls {
        let call = peer.request().await;
        let result = match call["method"].as_str().unwrap() {
            "cookies" => json!({"cookies":[]}),
            "addCookies" => json!({}),
            method => panic!("Unexpected credential cookie call: {method}"),
        };
        peer.send(&json!({"id":call["id"],"result":result})).await;
    }
    complete(session, id).await
}
#[test]
fn http_credentials_setter_requires_explicit_value_and_omits_secrets_or_native_dispatch() {
    use crate::browser::settings::ContextSetting;
    assert!(serde_json::from_value::<ContextSetting>(json!({"kind":"http_credentials"})).is_err());
    for value in [Value::Null, json!([])] {
        let setting: ContextSetting =
            serde_json::from_value(json!({"kind":"http_credentials","credentials":value})).unwrap();
        assert_eq!(
            setting.wire(Engine::Firefox).unwrap(),
            ("setHTTPCredentials", json!({}))
        );
        assert_eq!(setting.wire(Engine::Chrome), Err("CAPABILITY_UNAVAILABLE"));
    }
    let setting: ContextSetting = serde_json::from_value(
        json!({"kind":"http_credentials","credentials":credential("Acme-private-password")}),
    )
    .unwrap();
    assert_eq!(
        setting.summary(),
        json!({"kind":"http_credentials","credentialCount":1})
    );
    assert!(!format!("{setting:?}").contains("Acme-private-password"));
}
#[tokio::test]
async fn http_credentials_setter_fences_unknown_results_preserves_overrides_and_explicit_recovery()
{
    let site = wire::Site::start(None).await;
    let (session, mut peer, _root, context, _) = harness().await;
    session.0.host.state.lock().unwrap().context_options.insert(context,serde_json::from_value(json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"baseURL":site.url,"userAgent":"Acme","httpCredentials":credential("pass")})).unwrap());
    let created = submit(
        &session,
        json!({"operation":"http_context_create","options":{"browserContext":context,"allowUnknownCookiePartitions":true}}),
    );
    let client = complete(&session, created).await.result.unwrap()["httpContext"].clone();
    assert_expired_credential_update(&session, context).await;
    assert_eq!(
        fetch(&session, &mut peer, &client, 3).await.result.unwrap()["status"],
        200
    );
    let change = json!({"operation":"context_configure","context":context,"setting":{"kind":"http_credentials","credentials":credential("Acme-private-password")}});
    let id = submit(&session, change);
    let call = peer.request().await;
    assert_eq!(call["method"], "setHTTPCredentials");
    assert_eq!(
        call["params"]["httpCredentials"],
        json!([credential("Acme-private-password")])
    );
    peer.send(&method_error(call["id"].clone())).await;
    let failed = complete(&session, id).await;
    assert_eq!(failed.outcome, Outcome::Unknown);
    assert!(
        !failed
            .result
            .unwrap()
            .to_string()
            .contains("Acme-private-password")
    );
    let before = site.records().len();
    let id = submit(
        &session,
        json!({"operation":"http_fetch","httpContext":client,"request":{"url":"/auth/basic"}}),
    );
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("HTTP_CONTEXT_CREDENTIALS_UNCERTAIN")
    );
    assert_eq!(site.records().len(), before);
    let id = submit(
        &session,
        json!({"operation":"http_context_create","options":{"browserContext":context,"allowUnknownCookiePartitions":true}}),
    );
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("HTTP_CONTEXT_CREDENTIALS_UNCERTAIN")
    );
    let id = submit(
        &session,
        json!({"operation":"http_context_create","options":{"browserContext":context,"httpCredentials":credential("pass"),"allowUnknownCookiePartitions":true}}),
    );
    let explicit = complete(&session, id).await.result.unwrap()["httpContext"].clone();
    assert_eq!(
        fetch(&session, &mut peer, &explicit, 3)
            .await
            .result
            .unwrap()["status"],
        200
    );
    let id = submit(
        &session,
        json!({"operation":"context_configure","context":context,"setting":{"kind":"http_credentials","credentials":credential("pass")}}),
    );
    let call = peer.request().await;
    assert_eq!(call["method"], "setHTTPCredentials");
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    let applied = complete(&session, id).await;
    assert_eq!(applied.outcome, Outcome::Succeeded);
    assert_eq!(
        applied.result.unwrap()["requested"],
        json!({"kind":"http_credentials","credentialCount":1})
    );
    assert_eq!(
        fetch(&session, &mut peer, &client, 3).await.result.unwrap()["status"],
        200
    );
    let id = submit(
        &session,
        json!({"operation":"context_configure","context":context,"setting":{"kind":"http_credentials","credentials":Value::Null}}),
    );
    let call = peer.request().await;
    assert_eq!(call["params"], json!({}));
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    assert_eq!(complete(&session, id).await.outcome, Outcome::Succeeded);
    let reset = fetch(&session, &mut peer, &client, 2).await.result.unwrap();
    assert_eq!(reset["status"], 401);
    assert_eq!(reset["authenticationRetries"], 0);
    for client in [client, explicit] {
        let id = submit(
            &session,
            json!({"operation":"http_context_remove","httpContext":client}),
        );
        assert_eq!(complete(&session, id).await.outcome, Outcome::Succeeded);
    }
    close(&session).await;
}
#[tokio::test]
async fn http_credentials_setter_late_ack_cannot_revive_closed_context_defaults() {
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
        json!({"operation":"context_configure","context":context,"setting":{"kind":"http_credentials","credentials":credential("pass")}}),
    );
    let call = peer.request().await;
    peer.send(&json!({"guid":"context","method":"close","params":{}}))
        .await;
    peer.send(&json!({"id":call["id"],"result":{}})).await;
    let result = complete(&session, id).await;
    assert_eq!(result.outcome, Outcome::Succeeded);
    assert_eq!(result.result.unwrap()["scopeEnded"], true);
    let id = submit(
        &session,
        json!({"operation":"http_context_create","options":{"browserContext":context,"allowUnknownCookiePartitions":true}}),
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

#[tokio::test]
async fn http_credentials_setter_unknown_scope_refuses_creation_before_default_agent_probe() {
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
        json!({"operation":"context_configure","context":context,"setting":{"kind":"http_credentials","credentials":credential("pass")}}),
    );
    let call = peer.request().await;
    peer.send(&method_error(call["id"].clone())).await;
    assert_eq!(complete(&session, id).await.outcome, Outcome::Unknown);
    let id = submit(
        &session,
        json!({"operation":"http_context_create","options":{"browserContext":context,"allowUnknownCookiePartitions":true}}),
    );
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("HTTP_CONTEXT_CREDENTIALS_UNCERTAIN")
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err(),
        "Unknown inherited credentials must refuse before a metadata probe or client allocation"
    );
    assert_eq!(
        session.0.host.http_contexts.lock().unwrap().list()["contexts"],
        json!([])
    );
    close(&session).await;
}

async fn assert_expired_credential_update(session: &Session, context: Reference) {
    let not_sent = session
        .0
        .host
        .context_configure(
            context,
            serde_json::from_value(json!({"kind":"http_credentials","credentials":Value::Null}))
                .unwrap(),
            Instant::now(),
        )
        .await
        .unwrap_err();
    assert_eq!(not_sent.partial.unwrap()["stateMayHaveChanged"], false);
}
