//! Independent wire assertions for credential selection and acknowledged replay.
use super::*;
use crate::http_fixtures::defaults as fixture;
use base64::Engine as _;

fn credentials(value: Value) -> Credentials {
    serde_json::from_value(value).unwrap()
}
fn entry(origin: Value, send: Value) -> Value {
    json!({"username":"Acme","password":"pass","origin":origin,"send":send})
}
async fn request(
    contexts: &mut Contexts,
    context: Reference,
    request: Fetch,
) -> Result<Value, Error> {
    let id = contexts
        .start(context, request, Instant::now() + Duration::from_secs(10))
        .unwrap();
    let Ready::Reply(reply) = ready(contexts, id).await? else {
        panic!("Expected HTTP reply")
    };
    let metadata = reply.metadata.clone();
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
    Ok(metadata)
}
fn client(contexts: &mut Contexts, site: &fixture::Site, value: Value) -> Reference {
    serde_json::from_value(
        contexts
            .create(
                contexts.connection.unwrap_or_else(Uuid::new_v4),
                Options {
                    base_url: Some(site.url.clone()),
                    http_credentials: Some(credentials(value)),
                    ..Default::default()
                },
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap()
}
#[test]
fn http_credentials_first_matching_origin_and_sdk_wire_inheritance_are_explicit() {
    let url = Url::parse("https://example.com/path").unwrap();
    let configured = credentials(json!([
        {"username":"port","password":"private","origin":"https://example.com:443"},
        {"username":"case","password":"private","origin":"HTTPS://EXAMPLE.COM"},
        {"username":"wildcard","password":"private"}
    ]));
    assert_eq!(configured.matching(&url).unwrap().username, "case");
    assert_eq!(
        configured
            .matching(&Url::parse("http://example.com/").unwrap())
            .unwrap()
            .username,
        "wildcard"
    );
    assert_eq!(
        credentials(json!([
            entry(json!(""), Value::Null),
            entry(json!("https://example.com"), json!("always"))
        ]))
        .matching(&url)
        .unwrap()
        .username,
        "Acme"
    );
    let context:super::super::super::contract::ContextOptions=serde_json::from_value(json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"httpCredentials":entry(Value::Null,Value::Null)})).unwrap();
    assert_eq!(
        context.wire()["httpCredentials"],
        json!([{"username":"Acme","password":"pass"}])
    );
    assert_eq!(
        context.validate_for(super::super::super::contract::Engine::Chrome),
        Ok(())
    );
    let mut inherited = Options::default();
    inherited.inherit(&context);
    assert_eq!(
        inherited
            .http_credentials
            .as_ref()
            .unwrap()
            .matching(&url)
            .unwrap()
            .header(),
        "Basic QWNtZTpwYXNz"
    );
    let mut explicit = Options {
        http_credentials: Some(credentials(json!([]))),
        ..Default::default()
    };
    explicit.inherit(&context);
    assert!(explicit.http_credentials.unwrap().matching(&url).is_none());
    assert!(!format!("{configured:?}").contains("private"));
    let unicode = credentials(json!({"username":"é","password":"π"}));
    assert_eq!(
        unicode.matching(&url).unwrap().header(),
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("é:π")
        )
    );
}
#[test]
fn http_credentials_bounds_refuse_before_client_allocation_without_secret_diagnostics() {
    let mut contexts = Contexts::default();
    for value in [
        json!(vec![entry(Value::Null, Value::Null); 33]),
        json!({"username":"x".repeat(4097),"password":"Acme-private"}),
    ] {
        let error = contexts
            .create(
                Uuid::new_v4(),
                Options {
                    http_credentials: Some(credentials(value)),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert_eq!(error.code, "HTTP_CREDENTIALS_BOUNDS");
        assert!(contexts.items.is_empty());
        assert!(!format!("{error:?}").contains("Acme-private"));
    }
}
#[tokio::test]
async fn http_credentials_challenge_replays_exact_body_once_and_carries_challenge_cookies() {
    let site = fixture::Site::start(None).await;
    let mut contexts = Contexts::default();
    let context = client(&mut contexts, &site, entry(json!(site.url), Value::Null));
    let response = request(
        &mut contexts,
        context,
        Fetch {
            method: "POST".into(),
            body: Some(Payload::Text {
                text: "Acme immutable body".into(),
            }),
            ..fetch("/auth/basic")
        },
    )
    .await
    .unwrap();
    assert_eq!(response["status"], 200);
    assert_eq!(response["authenticationRetries"], 1);
    assert_eq!(response["retryCount"], 0);
    assert_eq!(response["replayed"], true);
    let records = site.records();
    assert_eq!(records.len(), 2);
    assert!(records[0]["headers"]["authorization"].is_null());
    for record in &records {
        assert_eq!(record["method"], "POST");
        assert_eq!(record["body"], "Acme immutable body");
    }
    assert_eq!(records[1]["headers"]["authorization"], "Basic QWNtZTpwYXNz");
    assert_eq!(records[1]["headers"]["cookie"], "Acme-auth=ready");
    assert!(!response.to_string().contains("QWNtZTpwYXNz"));
    assert!(!response.to_string().contains("password"));
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_credentials_always_overrides_initial_header_and_challenges_remain_single_use() {
    let site = fixture::Site::start(None).await;
    let mut contexts = Contexts::default();
    for (path, send, header, expected_status, expected_requests) in [
        (
            "/auth/basic",
            Some("always"),
            Some("Basic explicit-wrong"),
            200,
            1,
        ),
        ("/auth/basic", None, Some("Basic explicit-wrong"), 401, 1),
        ("/auth/basic", None, Some(""), 200, 2),
        ("/auth/reject", None, None, 401, 2),
        ("/auth/lower", None, None, 401, 1),
        ("/auth/digest", None, None, 401, 1),
        ("/auth/absent", None, None, 401, 1),
        ("/auth/prefix", None, None, 200, 2),
    ] {
        let context = client(&mut contexts, &site, entry(Value::Null, json!(send)));
        let before = site.records().len();
        let response = request(
            &mut contexts,
            context,
            Fetch {
                headers: header
                    .map(|value| {
                        vec![Header {
                            name: "Authorization".into(),
                            value: value.into(),
                        }]
                    })
                    .unwrap_or_default(),
                ..fetch(path)
            },
        )
        .await
        .unwrap();
        assert_eq!(response["status"], expected_status, "{path}");
        assert_eq!(site.records().len(), before + expected_requests, "{path}");
        assert_eq!(
            response["authenticationRetries"],
            expected_requests - 1,
            "{path}"
        );
        assert_eq!(response["replayed"], expected_requests > 1, "{path}");
        contexts.remove(context).unwrap();
    }
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_credentials_redirects_reselect_origin_without_forwarding_credentials() {
    let second = fixture::Site::start(None).await;
    let first = fixture::Site::start(Some(second.url.clone())).await;
    let mut contexts = Contexts::default();
    for (entries, path, expected_status, expected_requests, expected_auth) in [
        (
            json!([entry(json!(first.url), json!("always"))]),
            "/auth/cross",
            401,
            2,
            0,
        ),
        (
            json!([
                entry(json!(first.url), json!("always")),
                entry(json!(second.url), Value::Null)
            ]),
            "/auth/cross",
            200,
            3,
            1,
        ),
        (
            entry(json!(first.url), json!("always")),
            "/auth/redirect",
            200,
            2,
            0,
        ),
    ] {
        let context = client(&mut contexts, &first, entries);
        let before = first.records().len() + second.records().len();
        let second_before = second.records().len();
        let response = request(&mut contexts, context, fetch(path)).await.unwrap();
        assert_eq!(response["status"], expected_status);
        assert_eq!(response["authenticationRetries"], expected_auth);
        assert_eq!(
            first.records().len() + second.records().len(),
            before + expected_requests
        );
        if path == "/auth/cross" {
            assert!(second.records()[second_before]["headers"]["authorization"].is_null());
        }
        contexts.remove(context).unwrap();
    }
    contexts.close().unwrap();
}

#[tokio::test]
async fn http_credentials_unknown_cookie_update_prevents_challenge_replay() {
    struct Cookies;
    impl browser::BrowserCookies for Cookies {
        fn partition_identity_available(&self) -> bool {
            true
        }
        fn partition_model(&self) -> browser::PartitionModel {
            browser::PartitionModel::Unsupported
        }
        fn read(&self, _: Instant) -> Result<Value, Error> {
            Ok(json!([]))
        }
        fn write(&self, _: Value, _: Instant) -> Result<(), Error> {
            Err("OUTCOME_UNKNOWN".into())
        }
    }
    let site = fixture::Site::start(None).await;
    let mut contexts = Contexts::default();
    let owner = Uuid::new_v4();
    let associated = Reference {
        connection_id: owner,
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let context = serde_json::from_value(
        contexts
            .create_with_browser(
                owner,
                Options {
                    browser_context: Some(associated),
                    base_url: Some(site.url.clone()),
                    http_credentials: Some(credentials(entry(Value::Null, Value::Null))),
                    ..Default::default()
                },
                Some(Arc::new(Cookies)),
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap();
    let response = request(&mut contexts, context, fetch("/auth/basic"))
        .await
        .unwrap();
    assert_eq!(response["status"], 401);
    assert_eq!(response["browserCookieUpdate"]["state"], "unknown");
    assert_eq!(response["authenticationRetries"], 0);
    assert_eq!(response["replayed"], false);
    assert_eq!(site.records().len(), 1);
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
}

struct Browser;
impl browser::BrowserCookies for Browser {
    fn partition_identity_available(&self) -> bool {
        true
    }
    fn partition_model(&self) -> browser::PartitionModel {
        browser::PartitionModel::Unsupported
    }
    fn read(&self, _: Instant) -> Result<Value, Error> {
        Ok(json!([]))
    }
    fn write(&self, _: Value, _: Instant) -> Result<(), Error> {
        Ok(())
    }
}
fn associated(owner: Uuid) -> Reference {
    Reference {
        connection_id: owner,
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    }
}
fn inherited_options(contexts: &mut Contexts, context: Reference, site: &fixture::Site) -> Options {
    let mut options = Options {
        browser_context: Some(context),
        base_url: Some(site.url.clone()),
        ..Default::default()
    };
    contexts.inherit_credentials(
        &mut options,
        context,
        Some(&credentials(entry(Value::Null, Value::Null))),
    );
    options
}
#[tokio::test]
async fn http_credentials_live_defaults_fence_uncertainty_and_close_without_touching_overrides() {
    let site = fixture::Site::start(None).await;
    let mut contexts = Contexts::default();
    let owner = Uuid::new_v4();
    let context = associated(owner);
    let options = inherited_options(&mut contexts, context, &site);
    assert!(
        serde_json::to_value(&options).unwrap()["httpCredentials"].is_null(),
        "Inherited options must not retain a serialized initial secret copy"
    );
    let inherited = serde_json::from_value(
        contexts
            .create_with_browser(owner, options, Some(Arc::new(Browser)))
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap();
    let explicit = client(&mut contexts, &site, entry(Value::Null, Value::Null));
    assert_eq!(
        request(&mut contexts, inherited, fetch("/auth/basic"))
            .await
            .unwrap()["status"],
        200
    );
    let previous = contexts.context_credentials_pending(context).unwrap();
    assert_eq!(
        contexts
            .start(
                inherited,
                fetch("/auth/basic"),
                Instant::now() + Duration::from_secs(5)
            )
            .unwrap_err()
            .code,
        "HTTP_CONTEXT_CREDENTIALS_UNCERTAIN"
    );
    assert!(contexts.jobs.is_empty());
    let before = site.records().len();
    let options = inherited_options(&mut contexts, context, &site);
    assert_eq!(
        contexts
            .create_with_browser(owner, options, Some(Arc::new(Browser)))
            .unwrap_err()
            .code,
        "HTTP_CONTEXT_CREDENTIALS_UNCERTAIN"
    );
    assert_eq!(site.records().len(), before);
    assert_eq!(contexts.items.len(), 2);
    assert_eq!(
        request(&mut contexts, explicit, fetch("/auth/basic"))
            .await
            .unwrap()["status"],
        200
    );
    contexts.context_credentials_applied(context, Some(&previous));
    assert_eq!(
        request(&mut contexts, inherited, fetch("/auth/basic"))
            .await
            .unwrap()["status"],
        200
    );
    contexts.context_credentials_applied(context, None);
    let response = request(&mut contexts, inherited, fetch("/auth/basic"))
        .await
        .unwrap();
    assert_eq!(response["status"], 401);
    assert_eq!(response["authenticationRetries"], 0);
    contexts.context_credentials_applied(
        context,
        Some(&credentials(
            json!({"username":"Acme2","password":"new-pass"}),
        )),
    );
    assert_eq!(
        request(&mut contexts, inherited, fetch("/auth/changed"))
            .await
            .unwrap()["status"],
        200
    );
    let admitted_defaults = contexts.get(inherited).unwrap().options.clone();
    contexts.context_defaults_closed(context);
    assert_eq!(
        admitted_defaults.effective_credentials().unwrap_err().code,
        "HTTP_CONTEXT_CREDENTIALS_UNCERTAIN"
    );
    assert_eq!(
        contexts
            .start(
                inherited,
                fetch("/auth/basic"),
                Instant::now() + Duration::from_secs(5)
            )
            .unwrap_err()
            .code,
        "HTTP_CONTEXT_DISPOSED"
    );
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_credentials_admission_snapshot_survives_changes_before_dispatch_and_challenge() {
    struct Gated {
        entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl browser::BrowserCookies for Gated {
        fn partition_identity_available(&self) -> bool {
            true
        }
        fn partition_model(&self) -> browser::PartitionModel {
            browser::PartitionModel::Unsupported
        }
        fn read(&self, _: Instant) -> Result<Value, Error> {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                let _ = entered.send(());
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| Error::from("Acme-cookie-gate-timeout"))?;
            }
            Ok(json!([]))
        }
        fn write(&self, _: Value, _: Instant) -> Result<(), Error> {
            Ok(())
        }
    }
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
    let site = fixture::Site::start(None).await;
    let mut contexts = Contexts::default();
    let owner = Uuid::new_v4();
    let context = associated(owner);
    let options = inherited_options(&mut contexts, context, &site);
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let gate = Release(Some(release));
    let inherited = serde_json::from_value(
        contexts
            .create_with_browser(
                owner,
                options,
                Some(Arc::new(Gated {
                    entered: Mutex::new(Some(entered)),
                    release: Mutex::new(released),
                })),
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap();
    let id = contexts
        .start(
            inherited,
            fetch("/auth/basic"),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap();
    assert!(site.records().is_empty());
    contexts.context_credentials_applied(
        context,
        Some(&credentials(json!({"username":"Acme","password":"wrong"}))),
    );
    drop(gate);
    let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Expected retained original reply")
    };
    assert_eq!(reply.metadata["status"], 200);
    assert_eq!(reply.metadata["authenticationRetries"], 1);
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
    assert_eq!(site.records().len(), 2);
    assert_eq!(
        site.records()[1]["headers"]["authorization"],
        "Basic QWNtZTpwYXNz"
    );
    let response = request(&mut contexts, inherited, fetch("/auth/basic"))
        .await
        .unwrap();
    assert_eq!(response["status"], 401);
    assert_eq!(response["authenticationRetries"], 1);
    assert_eq!(site.records().len(), 4);
    contexts.close().unwrap();
}
