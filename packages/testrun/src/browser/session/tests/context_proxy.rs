//! SDK context proxies retain derived defaults and refuse late closed scopes.
use super::*;

fn options(password: bool) -> ContextOptions {
    let mut proxy =
        json!({"server":"http://AcmeIgnored:ignored@127.0.0.1:8080/path","username":"Acme proxy"});
    if password {
        proxy["password"] = json!("proxy example");
    }
    serde_json::from_value(json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"proxy":proxy,
        "httpCredentials":{"username":"Acme origin","password":"origin example"},"extraHTTPHeaders":[{"name":"X-Acme","value":"context"}]})).unwrap()
}

#[tokio::test]
async fn context_proxy_sdk_caches_effective_defaults_and_fences_closed_initialization() {
    for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
        for inherited in [false, true] {
            for closed in [false, true] {
                let (session, mut peer, _root, _context, _page) = harness().await;
                let mut options = options(true);
                {
                    let mut state = session.0.host.state.lock().unwrap();
                    state.engine = engine;
                    if inherited {
                        state.launch_proxy = options.proxy.take();
                    }
                }
                let id = session
                    .submit(Uuid::new_v4(), Request::ContextCreate { options }, 5000)
                    .unwrap();
                let call = peer.request().await;
                assert_eq!(call["method"], "newContext");
                if inherited {
                    assert!(call["params"]["proxy"].is_null());
                } else {
                    assert_eq!(
                        call["params"]["proxy"]["server"],
                        "http://AcmeIgnored:ignored@127.0.0.1:8080/path"
                    );
                }
                assert_eq!(
                    call["params"]["httpCredentials"][0]["username"],
                    "Acme origin"
                );
                let mut context_event = created(
                    "Acme-proxy-context",
                    "BrowserContext",
                    json!({"debugger":{"guid":"debugger"},"requestContext":{"guid":"request"},"tracing":{"guid":"tracing"},"options":{}}),
                );
                context_event["guid"] = json!("browser");
                peer.send(&context_event).await;
                peer.send(
                    &json!({"id":call["id"],"result":{"context":{"guid":"Acme-proxy-context"}}}),
                )
                .await;
                initialize_proxy_context(&mut peer, closed).await;
                let done = complete(&session, id).await;
                if closed {
                    assert_eq!(
                        done.code.as_deref(),
                        Some("STALE_REFERENCE"),
                        "{engine:?}: {done:?}"
                    );
                    assert_closed_proxy_defaults(&session);
                } else {
                    assert_eq!(done.outcome, Outcome::Succeeded, "{done:?}");
                    let context: Reference =
                        serde_json::from_value(done.result.unwrap()["context"].clone()).unwrap();
                    let state = session.0.host.state.lock().unwrap();
                    let stored = &state.context_options[&context];
                    assert_eq!(
                        stored.proxy.as_ref().unwrap().server,
                        "http://127.0.0.1:8080"
                    );
                    let url = url::Url::parse("http://127.0.0.1/auth").unwrap();
                    assert_eq!(
                        stored
                            .http_credentials
                            .as_ref()
                            .unwrap()
                            .matching(&url)
                            .unwrap()
                            .username,
                        if engine == Engine::Firefox {
                            "Acme origin"
                        } else {
                            "Acme proxy"
                        }
                    );
                    assert_eq!(
                        stored
                            .extra_http_headers
                            .as_ref()
                            .unwrap()
                            .iter()
                            .any(|header| header.name == "Proxy-Authorization"),
                        engine == Engine::Webkit
                    );
                }
                close(&session).await;
                assert!(
                    session.0.host.state.lock().unwrap().launch_proxy.is_none(),
                    "Verified browser closure releases its private launch defaults"
                );
            }
        }
    }
}

#[tokio::test]
async fn context_proxy_sdk_webkit_missing_password_refuses_before_remote_allocation() {
    let (session, mut peer, _root, _context, _page) = harness().await;
    session.0.host.state.lock().unwrap().engine = Engine::Webkit;
    let before = session.0.host.state.lock().unwrap().resources.len();
    let id = session
        .submit(
            Uuid::new_v4(),
            Request::ContextCreate {
                options: options(false),
            },
            5000,
        )
        .unwrap();
    assert_eq!(
        complete(&session, id).await.code.as_deref(),
        Some("HTTP_PROXY_PASSWORD_REQUIRED")
    );
    assert_eq!(session.0.host.state.lock().unwrap().resources.len(), before);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.request())
            .await
            .is_err()
    );
    close(&session).await;
}

async fn initialize_proxy_context(peer: &mut Fixture, closed: bool) {
    for method in [
        "registerSelectorEngine",
        "updateSubscription",
        "addInitScript",
    ] {
        let setup = peer.request().await;
        assert_eq!(setup["guid"], "Acme-proxy-context");
        assert_eq!(setup["method"], method);
        if closed && method == "addInitScript" {
            peer.send(&json!({"guid":"Acme-proxy-context","method":"close","params":{}}))
                .await;
        }
        let result = if method == "addInitScript" {
            let mut script = created("Acme-proxy-init", "Disposable", json!({}));
            script["guid"] = json!("Acme-proxy-context");
            peer.send(&script).await;
            json!({"disposable":{"guid":"Acme-proxy-init"}})
        } else {
            json!({})
        };
        peer.send(&json!({"id":setup["id"],"result":result})).await;
    }
}

fn assert_closed_proxy_defaults(session: &Session) {
    assert!(
        !session
            .0
            .host
            .state
            .lock()
            .unwrap()
            .context_options
            .values()
            .any(|options| options.proxy.is_some())
    );
}
