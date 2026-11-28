//! Real HTTP evidence, cookie, redirect and response-retention boundaries.
mod proxy;
mod proxy_refusal;
mod redirects;
mod socks;
use super::*;
mod client_certificates;
mod concurrency;
mod cookie_admin;
mod credentials;
mod defaults;
mod disposal;
mod header_defaults;
mod json_strings;
mod request_timeouts;
mod response;
mod storage_state;
mod tls;
mod wire_inputs;

#[test]
fn route_fetch_copies_the_paused_request_without_transport_or_cookie_headers() {
    let paused = super::super::har::Request::new(
        "https://example.com/acme?item=one",
        "POST",
        json!([
            {"name":"Content-Type","value":"application/json"},
            {"name":"Cookie","value":"session=secret"},
            {"name":"Host","value":"example.com"},
            {"name":"Content-Length","value":"2"},
            {"name":"X-Acme","value":"kept"}
        ]),
        Some(b"{}".to_vec()),
    )
    .unwrap();
    let options = RouteFetchOptions {
        max_redirects: Some(0),
        max_retries: Some(2),
        timeout_ms: Some(5000),
        overrides: None,
    };
    let fetch = route_fetch_request(&paused, &options).unwrap();
    assert_eq!(fetch.url, "https://example.com/acme?item=one");
    assert_eq!(fetch.method, "POST");
    let names = fetch
        .headers
        .iter()
        .map(|header| header.name.to_ascii_lowercase())
        .collect::<Vec<_>>();
    assert_eq!(names, ["content-type", "x-acme"]);
    assert_eq!(fetch.max_redirects, Some(0));
    assert_eq!(fetch.timeout_ms, Some(5000));
    assert!(fetch.body.is_some());
    assert!(
        fetch.read_body,
        "Fulfillment needs the response body as evidence"
    );
    assert_eq!(fetch.max_bytes, MAX_BODY);
    assert_eq!(fetch.max_retries, 2);
    for (redirects, retries, timeout, valid) in [
        (Some(21), None, None, false),
        (None, Some(21), None, false),
        (None, None, Some(0), true),
        (None, None, Some(120_001), false),
        (Some(20), Some(20), Some(120_000), true),
    ] {
        let options = RouteFetchOptions {
            max_redirects: redirects,
            max_retries: retries,
            timeout_ms: timeout,
            overrides: None,
        };
        assert_eq!(options.validate().is_ok(), valid);
    }

    // Overrides replace each field for this fetch; transport headers stay filtered.
    let overrides: crate::browser::routing::Overrides = serde_json::from_value(json!({
        "url":"http://example.com/other","method":"PUT","bodyBase64":"QWNtZQ==",
        "headers":[{"name":"X-Other","value":"one"},{"name":"Cookie","value":"forged=1"}]
    }))
    .unwrap();
    let options = RouteFetchOptions {
        overrides: Some(overrides),
        ..RouteFetchOptions::default()
    };
    let fetch = route_fetch_request(&paused, &options).unwrap();
    assert_eq!(fetch.url, "http://example.com/other");
    assert_eq!(fetch.method, "PUT");
    assert_eq!(
        fetch
            .headers
            .iter()
            .map(|header| header.name.as_str())
            .collect::<Vec<_>>(),
        ["X-Other"]
    );
    assert_eq!(
        serde_json::to_value(&fetch.body).unwrap(),
        json!({"kind":"base64","data":"QWNtZQ=="})
    );
    assert_eq!(fetch.max_retries, 0);
    for invalid in [
        json!({"url":"ftp://example.com/"}),
        json!({"method":"BAD METHOD"}),
        json!({"bodyBase64":"not base64"}),
    ] {
        let options = RouteFetchOptions {
            overrides: Some(serde_json::from_value(invalid.clone()).unwrap()),
            ..RouteFetchOptions::default()
        };
        assert!(options.validate().is_err(), "{invalid}");
    }
}

#[tokio::test]
async fn structured_http_payloads_set_defaults_and_preserve_explicit_content_types() {
    let site = Site::start(None).await;
    let mut contexts = Contexts::default();
    let context = create(&mut contexts, Uuid::new_v4(), &site.url);
    for explicit in [false, true] {
        for payload in [
            Payload::Json {
                value: json!({"name":"Acme 東京","enabled":true}),
            },
            Payload::Form {
                fields: vec![
                    FormField {
                        name: "label".into(),
                        value: "Acme +&".into(),
                    },
                    FormField {
                        name: "label".into(),
                        value: "two".into(),
                    },
                ],
            },
        ] {
            let expected = payload.bytes().unwrap();
            let content_type = if explicit {
                "application/acme-test"
            } else {
                payload.content_type().unwrap()
            };
            let response = body(
                &mut contexts,
                context,
                Fetch {
                    method: "POST".into(),
                    body: Some(payload),
                    headers: if explicit {
                        vec![Header {
                            name: "Content-Type".into(),
                            value: content_type.into(),
                        }]
                    } else {
                        vec![]
                    },
                    ..fetch("/payload")
                },
            )
            .await;
            let response: Value = serde_json::from_slice(&response).unwrap();
            assert_eq!(response["body"], String::from_utf8(expected).unwrap());
            assert_eq!(response["contentType"], content_type);
        }
    }
    let inherited = serde_json::from_value(
        contexts
            .create(
                context.connection_id,
                Options {
                    base_url: Some(site.url.clone()),
                    headers: Some(vec![Header {
                        name: "content-type".into(),
                        value: "application/acme-context".into(),
                    }]),
                    ..Default::default()
                },
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap();
    for override_type in [None, Some("application/acme-request")] {
        let response = body(
            &mut contexts,
            inherited,
            Fetch {
                method: "POST".into(),
                body: Some(Payload::Json {
                    value: json!({"name":"Acme"}),
                }),
                headers: override_type
                    .into_iter()
                    .map(|value| Header {
                        name: "Content-Type".into(),
                        value: value.into(),
                    })
                    .collect(),
                ..fetch("/payload")
            },
        )
        .await;
        let response: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(
            response["contentType"],
            override_type.unwrap_or("application/acme-context")
        );
    }
    assert_eq!(site.posts.load(Ordering::SeqCst), 6);
    contexts.remove(inherited).unwrap();
    contexts.remove(context).unwrap();
    site.close().await;
}

#[test]
fn browser_cookie_exchange_fences_domains_paths_expiry_and_partitions() {
    let cookies = json!([
        {"name":"host","value":"Acme","domain":"example.com","path":"/a","secure":false,"expires":-1,"httpOnly":true},
        {"name":"suffix","value":"Acme","domain":".example.com","path":"/","secure":false,"expires":-1},
        {"name":"secure","value":"Acme","domain":"example.com","path":"/","secure":true,"expires":-1},
        {"name":"expired","value":"Acme","domain":"example.com","path":"/","secure":false,"expires":0},
        {"name":"partition","value":"Acme","domain":"example.com","path":"/","secure":false,"expires":-1,"partitionKey":{"topLevelSite":"https://other.example"}}
    ]);
    assert_eq!(
        browser::header(
            &cookies,
            &Url::parse("http://example.com/a/b").unwrap(),
            None
        )
        .unwrap(),
        "host=Acme; suffix=Acme"
    );
    assert_eq!(
        browser::header(
            &cookies,
            &Url::parse("http://sub.example.com/ab").unwrap(),
            None
        )
        .unwrap(),
        "suffix=Acme"
    );
    assert_eq!(
        browser::header(
            &cookies,
            &Url::parse("http://notexample.com/a").unwrap(),
            None
        )
        .unwrap(),
        ""
    );
    let mut headers = ureq::http::HeaderMap::new();
    headers.append(
        "set-cookie",
        "Acme=value; Path=/a; HttpOnly; SameSite=Strict"
            .parse()
            .unwrap(),
    );
    headers.append("set-cookie", "gone=; Path=/; Max-Age=0".parse().unwrap());
    headers.append(
        "set-cookie",
        "foreign=value; Domain=other.example".parse().unwrap(),
    );
    headers.append(
        "set-cookie",
        "partition=value; Partitioned; Secure".parse().unwrap(),
    );
    let received = browser::response_cookies(
        &Url::parse("https://example.com/a/b").unwrap(),
        &headers,
        None,
    );
    assert_eq!(received.as_array().unwrap().len(), 2);
    assert_eq!(received[0]["domain"], "example.com");
    assert_eq!(received[0]["httpOnly"], true);
    assert_eq!(received[0]["sameSite"], "Strict");
    assert!(
        received[1]["expires"].as_f64().unwrap()
            <= time::OffsetDateTime::now_utc().unix_timestamp() as f64
    );
}

#[test]
fn selected_cookie_partitions_never_mix_sites_ancestry_or_opaque_keys() {
    let cookies = json!([
        {"name":"plain","value":"one","domain":"example.com","path":"/","secure":true,"expires":-1},
        {"name":"cross","value":"two","domain":"example.com","path":"/","secure":true,"expires":-1,"partitionKey":"https://acme.example","_crHasCrossSiteAncestor":true},
        {"name":"same","value":"three","domain":"example.com","path":"/","secure":true,"expires":-1,"partitionKey":"https://acme.example","_crHasCrossSiteAncestor":false},
        {"name":"other","value":"four","domain":"example.com","path":"/","secure":true,"expires":-1,"partitionKey":"https://other.example","_crHasCrossSiteAncestor":true},
        {"name":"opaque","value":"five","domain":"example.com","path":"/","secure":true,"expires":-1,"partitionKey":"https://acme.example","partitionKeyOpaque":true,"_crHasCrossSiteAncestor":true}
    ]);
    let url = Url::parse("https://example.com/path").unwrap();
    assert_eq!(browser::header(&cookies, &url, None).unwrap(), "plain=one");
    for (ancestor, expected) in [
        (Some(true), "plain=one; cross=two"),
        (Some(false), "plain=one; same=three"),
        (None, "plain=one; cross=two; same=three"),
    ] {
        let partition = CookiePartition {
            top_level_site: "https://acme.example".into(),
            has_cross_site_ancestor: ancestor,
        };
        assert_eq!(
            browser::header(&cookies, &url, Some(&partition)).unwrap(),
            expected
        );
    }
    let partition = CookiePartition {
        top_level_site: "https://other.example".into(),
        has_cross_site_ancestor: Some(true),
    };
    assert_eq!(
        browser::header(&cookies, &url, Some(&partition)).unwrap(),
        "plain=one; other=four"
    );
    let mut headers = ureq::http::HeaderMap::new();
    headers.append(
        "set-cookie",
        "selected=Acme; Secure; Partitioned; Path=/; SameSite=None"
            .parse()
            .unwrap(),
    );
    headers.append(
        "set-cookie",
        "insecure=Acme; Partitioned; Path=/".parse().unwrap(),
    );
    assert!(
        browser::response_cookies(&url, &headers, None)
            .as_array()
            .unwrap()
            .is_empty()
    );
    let values = browser::response_cookies(&url, &headers, Some(&partition));
    assert_eq!(values.as_array().unwrap().len(), 1);
    assert_eq!(values[0]["partitionKey"], "https://other.example");
    assert_eq!(values[0]["_crHasCrossSiteAncestor"], true);
    assert!(
        browser::response_cookies(
            &Url::parse("http://example.com/").unwrap(),
            &headers,
            Some(&partition)
        )
        .as_array()
        .unwrap()
        .is_empty()
    );
}

#[test]
fn partition_creation_requires_explicit_scope_and_a_supported_cookie_identity() {
    struct Cookies(browser::PartitionModel);
    impl browser::BrowserCookies for Cookies {
        fn partition_identity_available(&self) -> bool {
            true
        }
        fn read(&self, _: Instant) -> Result<Value, Error> {
            panic!("Creation must not read cookies")
        }
        fn write(&self, _: Value, _: Instant) -> Result<(), Error> {
            panic!("Creation must not write cookies")
        }
        fn partition_model(&self) -> browser::PartitionModel {
            self.0
        }
    }
    let owner = Uuid::new_v4();
    let context = Reference {
        connection_id: owner,
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let selected = CookiePartition {
        top_level_site: "https://acme.example".into(),
        has_cross_site_ancestor: None,
    };
    assert_eq!(
        Options {
            cookie_partition: Some(selected.clone()),
            ..Default::default()
        }
        .validate()
        .unwrap_err()
        .code,
        "HTTP_COOKIE_PARTITION_CONTEXT_REQUIRED"
    );
    for invalid in [
        "https://acme.example/",
        "https://acme.example/path",
        "https://user@acme.example",
        "https://acme.example?private=Acme",
        "null",
    ] {
        assert_eq!(
            CookiePartition {
                top_level_site: invalid.into(),
                has_cross_site_ancestor: None
            }
            .validate()
            .unwrap_err()
            .code,
            "HTTP_COOKIE_PARTITION_INVALID"
        );
    }
    for (model, ancestor, error) in [
        (
            browser::PartitionModel::Unsupported,
            None,
            Some("HTTP_COOKIE_PARTITION_UNSUPPORTED"),
        ),
        (
            browser::PartitionModel::Site,
            Some(false),
            Some("HTTP_COOKIE_PARTITION_ANCESTRY_UNSUPPORTED"),
        ),
        (browser::PartitionModel::Site, None, None),
        (browser::PartitionModel::SiteAndAncestor, None, None),
        (browser::PartitionModel::SiteAndAncestor, Some(false), None),
    ] {
        let mut contexts = Contexts::default();
        let options = Options {
            browser_context: Some(context),
            cookie_partition: Some(CookiePartition {
                has_cross_site_ancestor: ancestor,
                ..selected.clone()
            }),
            ..Default::default()
        };
        let result = contexts.create_with_browser(owner, options, Some(Arc::new(Cookies(model))));
        if let Some(error) = error {
            assert_eq!(result.unwrap_err().code, error);
            continue;
        }
        let result = result.unwrap();
        assert_eq!(result["cookiePartitionConfigured"], true);
        assert_eq!(
            result["cookiePartitionCrossSiteAncestor"],
            json!(if model == browser::PartitionModel::SiteAndAncestor {
                Some(ancestor.unwrap_or(true))
            } else {
                None
            })
        );
        assert!(!contexts.list().to_string().contains("acme.example"));
    }
}

#[test]
fn unknown_cookie_partition_identity_requires_explicit_opt_in_before_any_cookie_io() {
    struct Unlabeled;
    impl browser::BrowserCookies for Unlabeled {
        fn partition_identity_available(&self) -> bool {
            false
        }
        fn read(&self, _: Instant) -> Result<Value, Error> {
            panic!("No cookie read during creation")
        }
        fn write(&self, _: Value, _: Instant) -> Result<(), Error> {
            panic!("No cookie write during creation")
        }
    }
    let owner = Uuid::new_v4();
    let context = Reference {
        connection_id: owner,
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let options = Options {
        browser_context: Some(context),
        ..Default::default()
    };
    let mut contexts = Contexts::default();
    let rejected = contexts
        .create_with_browser(owner, options.clone(), Some(Arc::new(Unlabeled)))
        .unwrap_err();
    assert_eq!(rejected.code, "HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE");
    assert_eq!(rejected.partial.unwrap()["created"], false);
    assert!(contexts.list()["contexts"].as_array().unwrap().is_empty());
    let allowed = contexts
        .create_with_browser(
            owner,
            Options {
                allow_unknown_cookie_partitions: true,
                ..options
            },
            Some(Arc::new(Unlabeled)),
        )
        .unwrap();
    assert_eq!(allowed["cookiePartitionIdentityAvailable"], false);
    assert_eq!(allowed["unknownCookiePartitionsAllowed"], true);
    let listed = &contexts.list()["contexts"][0];
    assert_eq!(listed["cookiePartitionIdentityAvailable"], false);
    assert_eq!(listed["unknownCookiePartitionsAllowed"], true);
}

#[test]
fn browser_http_defaults_inherit_only_omitted_values_and_preserve_explicit_tls_false() {
    let context = super::super::contract::ContextOptions {
        user_agent: Some("Acme browser agent".into()),
        ignore_https_errors: Some(true),
        ..Default::default()
    };
    let mut inherited = Options::default();
    inherited.inherit(&context);
    assert_eq!(inherited.user_agent.as_deref(), Some("Acme browser agent"));
    assert_eq!(inherited.ignore_https_errors, Some(true));
    let mut explicit = Options {
        user_agent: Some("Acme HTTP agent".into()),
        ignore_https_errors: Some(false),
        ..Default::default()
    };
    explicit.inherit(&context);
    assert_eq!(explicit.user_agent.as_deref(), Some("Acme HTTP agent"));
    assert_eq!(explicit.ignore_https_errors, Some(false));
    explicit.user_agent = Some("Acme\r\nAuthorization: private".into());
    assert_eq!(explicit.validate().unwrap_err().code, "HTTP_HEADER_INVALID");
}

#[tokio::test]
async fn user_agent_defaults_yield_to_request_headers_without_changing_the_client() {
    let site = Site::start(None).await;
    let mut contexts = Contexts::default();
    let created = contexts
        .create(
            Uuid::new_v4(),
            Options {
                base_url: Some(site.url.clone()),
                user_agent: Some("Acme default agent".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let context = serde_json::from_value(created["httpContext"].clone()).unwrap();
    for override_agent in [None, Some("Acme request agent"), None] {
        let mut request = fetch("/agent");
        if let Some(agent) = override_agent {
            request.headers.push(Header {
                name: "User-Agent".into(),
                value: agent.into(),
            });
        }
        assert_eq!(
            String::from_utf8(body(&mut contexts, context, request).await).unwrap(),
            override_agent.unwrap_or("Acme default agent")
        );
    }
    site.close().await;
}

#[tokio::test]
async fn multipart_request_header_boundary_matches_the_dispatched_entity() {
    let site = Site::start(None).await;
    let mut contexts = Contexts::default();
    let context = create(&mut contexts, Uuid::new_v4(), &site.url);
    let bytes = body(
        &mut contexts,
        context,
        Fetch {
            method: "POST".into(),
            body: Some(Payload::Multipart {
                fields: vec![
                    MultipartPart::Text {
                        name: "label".into(),
                        value: "Acme multipart private 東京".into(),
                    },
                    MultipartPart::File {
                        name: "label".into(),
                        filename: "Acme.txt".into(),
                        mime_type: "text/plain".into(),
                        data: "QWNtZSBmaWxl".into(),
                    },
                ],
            }),
            ..fetch("/payload")
        },
    )
    .await;
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    let boundary = result["contentType"]
        .as_str()
        .unwrap()
        .strip_prefix("multipart/form-data; boundary=")
        .unwrap();
    let body = result["body"].as_str().unwrap();
    assert!(body.starts_with(&format!("--{boundary}\r\n")));
    assert!(body.ends_with(&format!("--{boundary}--\r\n")));
    assert!(body.contains("Acme multipart private 東京"));
    assert!(
        body.contains("filename=\"Acme.txt\"\r\nContent-Type: text/plain\r\n\r\nAcme file\r\n")
    );
    assert_eq!(site.posts.load(Ordering::SeqCst), 1);
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
    site.close().await;
}

#[tokio::test]
async fn status_failure_options_override_defaults_without_replaying_posts_or_disclosing_bodies() {
    let site = Site::start(None).await;
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    for default in [false, true] {
        let owner = contexts
            .create(
                connection,
                Options {
                    base_url: Some(site.url.clone()),
                    fail_on_status_code: default,
                    ..Default::default()
                },
            )
            .unwrap();
        let context = serde_json::from_value(owner["httpContext"].clone()).unwrap();
        for requested in [None, Some(false), Some(true)] {
            let id = contexts
                .start(
                    context,
                    Fetch {
                        method: "POST".into(),
                        fail_on_status_code: requested,
                        max_retries: 2,
                        body: Some(Payload::Text {
                            text: "Acme request private".into(),
                        }),
                        ..fetch("/write?token=Acme-query-private")
                    },
                    Instant::now() + Duration::from_secs(5),
                )
                .unwrap();
            match ready(&mut contexts, id).await {
                Err(error) => {
                    assert!(requested.unwrap_or(default));
                    assert_eq!(error.code, "HTTP_STATUS");
                    let result = error.partial.unwrap();
                    assert_eq!(result["status"], 409);
                    assert_eq!(result["dispatchCompleted"], true);
                    assert!(!result.to_string().contains("Acme response private"));
                    assert!(!result.to_string().contains("Acme-query-private"));
                }
                Ok(Ready::Reply(reply)) => {
                    assert!(!requested.unwrap_or(default));
                    assert_eq!(reply.metadata["status"], 409);
                    contexts.restore(id, reply);
                    contexts.discard(id).unwrap();
                }
                _ => panic!("Unexpected witness"),
            }
        }
        contexts.remove(context).unwrap();
    }
    assert_eq!(site.posts.load(Ordering::SeqCst), 6);
    contexts.close().unwrap();
    site.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn connection_reset_retries_require_explicit_opt_in_and_report_replayed_posts() {
    for (retries, path, succeeds) in [
        (0, "/reset-once", false),
        (1, "/reset-once", true),
        (1, "/reset-always", false),
    ] {
        let site = Site::start(None).await;
        let mut contexts = Contexts::default();
        let context = create(&mut contexts, Uuid::new_v4(), &site.url);
        let id = contexts
            .start(
                context,
                Fetch {
                    method: "POST".into(),
                    max_retries: retries,
                    body: Some(Payload::Text {
                        text: "Acme request private".into(),
                    }),
                    ..fetch(path)
                },
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        match ready(&mut contexts, id).await {
            Err(error) => {
                assert!(!succeeds);
                assert_eq!(error.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
                let partial = error.partial.unwrap();
                assert_eq!(partial["connectionReset"], true);
                assert_eq!(partial["retryCount"], retries);
                assert_eq!(partial["replayed"], retries > 0);
            }
            Ok(Ready::Reply(reply)) => {
                assert!(succeeds);
                assert_eq!(retries, 1);
                assert_eq!(reply.metadata["status"], 200);
                assert_eq!(reply.metadata["retryCount"], 1);
                assert_eq!(reply.metadata["replayed"], true);
                contexts.restore(id, reply);
                contexts.discard(id).unwrap();
            }
            _ => panic!("Unexpected witness"),
        }
        assert_eq!(site.posts.load(Ordering::SeqCst), 1 + retries as usize);
        contexts.remove(context).unwrap();
        contexts.close().unwrap();
        site.close().await;
    }
}

struct UncertainBrowserCookies {
    fail_read: bool,
}
struct HeldBrowserCookies {
    started: Arc<tokio::sync::Notify>,
    released: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl browser::BrowserCookies for HeldBrowserCookies {
    fn partition_identity_available(&self) -> bool {
        true
    }
    fn read(&self, _deadline: Instant) -> Result<Value, Error> {
        self.started.notify_one();
        self.released
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| Error::from("HTTP_COOKIE_LOOKUP_INTERRUPTED"))?;
        Ok(json!([]))
    }
    fn write(&self, _cookies: Value, _deadline: Instant) -> Result<(), Error> {
        Ok(())
    }
}
#[tokio::test]
async fn held_browser_cookie_lookup_prevents_late_dispatch_after_deadline_or_cancel() {
    for (cancel, request_timeout) in [(false, None), (true, None), (false, Some(100))] {
        let site = Site::start(None).await;
        let mut contexts = Contexts::default();
        let connection = Uuid::new_v4();
        let context = Reference {
            connection_id: connection,
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let started = Arc::new(tokio::sync::Notify::new());
        let (release, released) = std::sync::mpsc::channel();
        let owner = contexts
            .create_with_browser(
                connection,
                Options {
                    browser_context: Some(context),
                    base_url: Some(site.url.clone()),
                    ..Default::default()
                },
                Some(Arc::new(HeldBrowserCookies {
                    started: started.clone(),
                    released: Mutex::new(released),
                })),
            )
            .unwrap();
        let owner = serde_json::from_value(owner["httpContext"].clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let id = contexts
            .start(
                owner,
                Fetch {
                    method: "POST".into(),
                    timeout_ms: request_timeout,
                    body: Some(Payload::Text {
                        text: "Acme request private".into(),
                    }),
                    ..fetch("/write")
                },
                deadline,
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        if cancel {
            contexts.cancel_request(id);
        } else {
            let effective = contexts.jobs[&id].deadline;
            if request_timeout.is_some() {
                assert!(effective < deadline);
            }
            tokio::time::sleep_until(effective).await;
        }
        release.send(()).unwrap();
        let error = match ready(&mut contexts, id).await {
            Err(error) => error,
            _ => panic!("Expired or cancelled request was sent"),
        };
        assert_eq!(
            error.code,
            if cancel { "CANCELLED" } else { "HTTP_DEADLINE" }
        );
        assert_eq!(error.partial.unwrap()["requestMayHaveBeenSent"], false);
        assert_eq!(site.posts.load(Ordering::SeqCst), 0);
        contexts.remove(owner).unwrap();
        contexts.close().unwrap();
        site.close().await;
    }
}
impl browser::BrowserCookies for UncertainBrowserCookies {
    fn partition_identity_available(&self) -> bool {
        true
    }
    fn read(&self, _deadline: Instant) -> Result<Value, Error> {
        if self.fail_read {
            return Err("OUTCOME_UNKNOWN".into());
        }
        Ok(json!([]))
    }
    fn write(&self, _cookies: Value, _deadline: Instant) -> Result<(), Error> {
        Err("HTTP_COOKIE_WRITE_OUTCOME_UNKNOWN".into())
    }
}
#[tokio::test]
async fn unknown_browser_cookie_update_retains_the_response_without_following_redirect_or_replaying()
 {
    let site = Site::start(None).await;
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    let browser_context = Reference {
        connection_id: connection,
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let created = contexts
        .create_with_browser(
            connection,
            Options {
                browser_context: Some(browser_context),
                base_url: Some(site.url.clone()),
                ..Default::default()
            },
            Some(Arc::new(UncertainBrowserCookies { fail_read: false })),
        )
        .unwrap();
    let context = serde_json::from_value(created["httpContext"].clone()).unwrap();
    let id = contexts
        .start(
            context,
            fetch("/redirect"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Expected retained HTTP response")
    };
    assert_eq!(reply.metadata["status"], 302);
    assert_eq!(reply.metadata["browserCookieUpdate"]["state"], "unknown");
    assert_eq!(reply.metadata["redirectStoppedForCookieUpdate"], true);
    assert_eq!(reply.metadata["replayed"], false);
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
    contexts.remove(context).unwrap();
    let created = contexts
        .create_with_browser(
            connection,
            Options {
                browser_context: Some(browser_context),
                base_url: Some(site.url.clone()),
                ..Default::default()
            },
            Some(Arc::new(UncertainBrowserCookies { fail_read: true })),
        )
        .unwrap();
    let context = serde_json::from_value(created["httpContext"].clone()).unwrap();
    let id = contexts
        .start(
            context,
            fetch("/echo"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let error = match ready(&mut contexts, id).await {
        Err(error) => error,
        _ => panic!("Uncertain cookie read was accepted"),
    };
    assert_eq!(error.code, "HTTP_BROWSER_COOKIES_UNAVAILABLE");
    assert_eq!(error.partial.unwrap()["requestMayHaveBeenSent"], false);
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
    site.close().await;
}
use std::sync::atomic::AtomicUsize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn fetch(url: &str) -> Fetch {
    Fetch {
        url: url.into(),
        params: None,
        timeout_ms: None,
        max_redirects: None,
        fail_on_status_code: None,
        max_retries: 0,
        ignore_https_errors: None,
        method: "GET".into(),
        headers: vec![],
        body: None,
        read_body: true,
        max_bytes: 65536,
    }
}
fn create(contexts: &mut Contexts, connection: Uuid, base: &str) -> Reference {
    serde_json::from_value(
        contexts
            .create(
                connection,
                Options {
                    base_url: Some(base.into()),
                    ..Default::default()
                },
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap()
}
async fn ready(contexts: &mut Contexts, id: Uuid) -> Result<Ready, Error> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(reply) = contexts.ready(id)? {
            return Ok(reply);
        }
        assert!(Instant::now() < deadline, "HTTP response never settled");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
struct Site {
    url: String,
    posts: Arc<AtomicUsize>,
    task: JoinHandle<()>,
    stop: tokio::sync::oneshot::Sender<()>,
}
impl Site {
    #[expect(
        clippy::too_many_lines,
        reason = "One owned loopback fixture serves redirect, body and header variants on a single listener"
    )]
    async fn start(redirect: Option<String>) -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let posts = Arc::new(AtomicUsize::new(0));
        let count = posts.clone();
        #[cfg(unix)]
        let resets = AtomicUsize::new(0);
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            loop {
                let mut stream = tokio::select! {accepted=listener.accept()=>accepted.unwrap().0,_=&mut stopped=>break};
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 1024];
                while bytes.len() < 16384 && !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                }
                let Some(end) = bytes
                    .windows(4)
                    .position(|part| part == b"\r\n\r\n")
                    .map(|offset| offset + 4)
                else {
                    continue;
                };
                let header = String::from_utf8_lossy(&bytes[..end]).into_owned();
                let length = header
                    .lines()
                    .find_map(|line| {
                        line.split_once(':')
                            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let read = stream.read(&mut buffer).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                }
                if header.starts_with("POST ") {
                    count.fetch_add(1, Ordering::SeqCst);
                }
                let path = header.split_whitespace().nth(1).unwrap_or("");
                #[cfg(unix)]
                if path == "/reset-always"
                    || (path == "/reset-once" && resets.fetch_add(1, Ordering::SeqCst) == 0)
                {
                    use std::os::fd::AsRawFd;
                    let linger = libc::linger {
                        l_onoff: 1,
                        l_linger: 0,
                    };
                    // The owned fixture must reset rather than gracefully close,
                    // so the client receives the precise retryable transport error.
                    let result = unsafe {
                        libc::setsockopt(
                            stream.as_raw_fd(),
                            libc::SOL_SOCKET,
                            libc::SO_LINGER,
                            (&linger as *const libc::linger).cast(),
                            std::mem::size_of::<libc::linger>() as libc::socklen_t,
                        )
                    };
                    assert_eq!(result, 0);
                    drop(stream);
                    continue;
                }
                if path == "/drop" {
                    continue;
                }
                if path == "/truncated" {
                    let _=stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nAcme").await;
                    continue;
                }
                let (status, extra, body) = if path.starts_with("/query") {
                    (
                        "200 OK",
                        String::new(),
                        json!({"target":path,"method":header.split_whitespace().next().unwrap()})
                            .to_string(),
                    )
                } else if path == "/agent" {
                    (
                        "200 OK",
                        String::new(),
                        header
                            .lines()
                            .find_map(|line| {
                                line.split_once(':')
                                    .filter(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
                                    .map(|(_, value)| value.trim())
                            })
                            .unwrap_or("")
                            .into(),
                    )
                } else if path == "/payload" {
                    ("200 OK", String::new(), json!({"body":String::from_utf8_lossy(&bytes[end..end+length]),"contentType":header.lines().find_map(|line|line.split_once(':').filter(|(name,_)|name.eq_ignore_ascii_case("content-type")).map(|(_,value)|value.trim())).unwrap_or("")}).to_string())
                } else if path.starts_with("/write") {
                    assert_eq!(&bytes[end..], b"Acme request private");
                    (
                        "409 Conflict",
                        "Set-Cookie: acme=Acme-cookie-private; Path=/; HttpOnly\r\n".into(),
                        "Acme response private".into(),
                    )
                } else if path == "/redirect" {
                    (
                        "302 Found",
                        "Location: /echo\r\nSet-Cookie: redirect=Acme; Path=/\r\n".into(),
                        String::new(),
                    )
                } else if path == "/cross" {
                    (
                        "302 Found",
                        format!("Location: {}\r\n", redirect.as_ref().unwrap()),
                        String::new(),
                    )
                } else if path == "/loop" {
                    ("302 Found", "Location: /loop\r\n".into(), String::new())
                } else if path == "/big" {
                    ("200 OK", String::new(), "x".repeat(1024))
                } else {
                    ("200 OK",String::new(),json!({"cookie":header.lines().find_map(|line|line.split_once(':').filter(|(name,_)|name.eq_ignore_ascii_case("cookie")).map(|(_,value)|value.trim())).unwrap_or(""),"authorization":header.lines().any(|line|line.to_ascii_lowercase().starts_with("authorization:")),"custom":header.lines().any(|line|line.to_ascii_lowercase().starts_with("x-acme:"))}).to_string())
                };
                let _=stream.write_all(format!("HTTP/1.1 {status}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await;
            }
        });
        Self {
            url,
            posts,
            task,
            stop,
        }
    }
    async fn close(self) {
        let _ = self.stop.send(());
        self.task.await.unwrap();
    }
}

#[test]
fn cookie_retention_prunes_expiry_and_evicts_oldest_without_history_growth() {
    let url = absolute_url("https://example.com/acme").unwrap();
    let mut cookies = Cookies::default();
    for index in 0..2000 {
        let mut headers = ureq::http::HeaderMap::new();
        headers.append(
            ureq::http::header::SET_COOKIE,
            format!("acme{index}=private; Path=/; Secure")
                .parse()
                .unwrap(),
        );
        cookies.receive(&url, &headers);
    }
    assert_eq!(cookies.keys.len(), 256);
    assert_eq!(cookies.store.len(), 256);
    assert_eq!(cookies.groups.len(), 256);
    assert_eq!(
        cookies
            .groups
            .iter()
            .map(|(_, keys)| keys.len())
            .sum::<usize>(),
        256
    );
    assert_eq!(cookies.order.len(), 256);
    assert!(cookies.bytes <= 65536);
    assert_eq!(cookies.evicted, 1744);
    assert!(
        cookies
            .header(&absolute_url("http://example.com/").unwrap())
            .is_empty()
    );
    assert!(
        cookies
            .header(&absolute_url("https://other.example/").unwrap())
            .is_empty()
    );
    let mut headers = ureq::http::HeaderMap::new();
    headers.append(
        ureq::http::header::SET_COOKIE,
        "acme1999=gone; Max-Age=0; Path=/; Secure".parse().unwrap(),
    );
    cookies.receive(&url, &headers);
    assert_eq!(cookies.keys.len(), 255);
    assert_eq!(cookies.store.len(), 255);
    let mut headers = ureq::http::HeaderMap::new();
    headers.append(
        ureq::http::header::SET_COOKIE,
        format!("large={}; Path=/", "x".repeat(5000))
            .parse()
            .unwrap(),
    );
    cookies.receive(&url, &headers);
    assert_eq!(cookies.dropped, 1);
    assert_eq!(cookies.keys.len(), 255);
}

#[test]
fn http_validation_rejects_invalid_scheme_credentials_and_header_injection_before_dispatch() {
    assert!(absolute_url("file:///acme").is_err());
    assert!(absolute_url("https://Acme:%ff@example.com/").is_err());
    assert!(absolute_url("https:///").is_err());
    let mut request = fetch("https://example.com/");
    request.headers.push(Header {
        name: "x-acme".into(),
        value: "Acme\r\nInjected: private".into(),
    });
    assert!(request.validate().is_err());
    request.headers.clear();
    request.max_bytes = MAX_BODY + 1;
    assert!(request.validate().is_err());
}

#[test]
fn default_http_headers_allow_global_scope_without_losing_header_validation() {
    let headers = vec![Header {
        name: "authorization".into(),
        value: "Acme private".into(),
    }];
    let options = Options {
        headers: Some(headers),
        ..Default::default()
    };
    options.validate().unwrap();
    let mut invalid = options.clone();
    invalid.headers.as_mut().unwrap()[0].value = "Acme\r\nInjected: private".into();
    assert_eq!(invalid.validate().unwrap_err().code, "HTTP_HEADER_INVALID");
    let mut contexts = Contexts::default();
    let context = contexts.create(Uuid::new_v4(), options).unwrap();
    let context = serde_json::from_value(context["httpContext"].clone()).unwrap();
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
}

#[tokio::test]
async fn http_write_is_retained_across_failed_finalization_and_never_replayed() {
    let site = Site::start(None).await;
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    let context = create(&mut contexts, connection, &site.url);
    let request = Fetch {
        method: "POST".into(),
        body: Some(Payload::Text {
            text: "Acme request private".into(),
        }),
        headers: vec![Header {
            name: "Authorization".into(),
            value: "Acme-authorization-private".into(),
        }],
        ..fetch("/write?token=Acme-query-private")
    };
    let id = contexts
        .start(context, request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        contexts.remove(context).unwrap_err().code,
        "HTTP_CONTEXT_BUSY"
    );
    assert_eq!(
        contexts.clear_cookies(context).unwrap_err().code,
        "HTTP_CONTEXT_BUSY"
    );
    assert!(
        !contexts
            .evidence_list()
            .to_string()
            .contains("Acme request private")
    );
    let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Unexpected completion witness");
    };
    assert_eq!(reply.metadata["status"], 409);
    assert_eq!(
        reply.body.as_deref(),
        Some(b"Acme response private".as_slice())
    );
    for private in [
        "Acme response private",
        "Acme-cookie-private",
        "Acme-query-private",
        "Acme-authorization-private",
    ] {
        assert!(!reply.metadata.to_string().contains(private));
    }
    contexts.remove(context).unwrap();
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("occupied");
    std::fs::write(&blocked, b"Acme").unwrap();
    assert!(materialize(&blocked, &reply).is_err());
    contexts.restore(id, reply);
    let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Response bytes lost after I/O failure");
    };
    let artifact = materialize(root.path(), &reply).unwrap();
    let value = contexts.complete(id, reply.metadata, artifact).unwrap();
    assert_eq!(
        std::fs::read(value["body"]["attachment"]["path"].as_str().unwrap()).unwrap(),
        b"Acme response private"
    );
    let Ready::Complete(repeated) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Missing finalization witness");
    };
    assert_eq!(repeated, value);
    assert_eq!(site.posts.load(Ordering::SeqCst), 1);
    contexts.close().unwrap();
    site.close().await;
}

async fn body(contexts: &mut Contexts, context: Reference, request: Fetch) -> Vec<u8> {
    let id = contexts
        .start(context, request, Instant::now() + Duration::from_secs(5))
        .unwrap();
    let Ready::Reply(reply) = ready(contexts, id).await.unwrap() else {
        panic!("Unexpected completion witness");
    };
    let body = reply.body.as_ref().unwrap().clone();
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
    body
}
#[tokio::test]
async fn http_cookies_are_isolated_and_redirects_strip_origin_bound_headers() {
    let destination = Site::start(None).await;
    let cross = destination.url.replace("127.0.0.1", "localhost");
    let site = Site::start(Some(format!("{cross}/echo"))).await;
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    let first = create(&mut contexts, connection, &site.url);
    let second = create(&mut contexts, connection, &site.url);
    let request = Fetch {
        method: "POST".into(),
        body: Some(Payload::Text {
            text: "Acme request private".into(),
        }),
        ..fetch("/write")
    };
    body(&mut contexts, first, request).await;
    let echoed: Value =
        serde_json::from_slice(&body(&mut contexts, first, fetch("/echo")).await).unwrap();
    assert_eq!(echoed["cookie"], "acme=Acme-cookie-private");
    let isolated: Value =
        serde_json::from_slice(&body(&mut contexts, second, fetch("/echo")).await).unwrap();
    assert_eq!(isolated["cookie"], "");
    let redirected: Value =
        serde_json::from_slice(&body(&mut contexts, first, fetch("/redirect")).await).unwrap();
    assert!(
        redirected["cookie"]
            .as_str()
            .unwrap()
            .contains("redirect=Acme")
    );
    let request = Fetch {
        headers: vec![
            Header {
                name: "Authorization".into(),
                value: "Acme private".into(),
            },
            Header {
                name: "x-acme".into(),
                value: "Acme private".into(),
            },
        ],
        ..fetch("/cross")
    };
    let crossed: Value =
        serde_json::from_slice(&body(&mut contexts, first, request).await).unwrap();
    assert_eq!(
        crossed,
        json!({"cookie":"","authorization":false,"custom":true})
    );
    contexts.clear_cookies(first).unwrap();
    let cleared: Value =
        serde_json::from_slice(&body(&mut contexts, first, fetch("/echo")).await).unwrap();
    assert_eq!(cleared["cookie"], "");
    contexts.close().unwrap();
    site.close().await;
    destination.close().await;
}
#[tokio::test]
async fn http_body_overflow_and_lost_write_reply_do_not_reissue_requests() {
    let site = Site::start(None).await;
    let mut contexts = Contexts::default();
    let context = create(&mut contexts, Uuid::new_v4(), &site.url);
    let id = contexts
        .start(
            context,
            Fetch {
                max_bytes: 8,
                max_retries: 2,
                ..fetch("/big")
            },
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let error = match ready(&mut contexts, id).await {
        Err(error) => error,
        _ => panic!("Oversize body accepted"),
    };
    assert_eq!(error.code, "HTTP_BODY_BOUNDS");
    assert_eq!(error.partial.unwrap()["dispatchCompleted"], true);
    let truncated = contexts
        .start(
            context,
            fetch("/truncated"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let error = match ready(&mut contexts, truncated).await {
        Err(error) => error,
        _ => panic!("Truncated response accepted"),
    };
    assert_eq!(error.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
    assert_eq!(error.partial.unwrap()["response"]["status"], 200);
    let id = contexts
        .start(
            context,
            Fetch {
                method: "POST".into(),
                body: Some(Payload::Text {
                    text: "Acme".into(),
                }),
                max_retries: 2,
                ..fetch("/drop")
            },
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let error = match ready(&mut contexts, id).await {
        Err(error) => error,
        _ => panic!("Lost write reported success"),
    };
    assert_eq!(error.code, "HTTP_REQUEST_OUTCOME_UNKNOWN");
    assert_eq!(error.partial.unwrap()["requestMayHaveBeenSent"], true);
    assert_eq!(
        contexts.ready(id).err().unwrap().code,
        "HTTP_REQUEST_OUTCOME_UNKNOWN"
    );
    assert_eq!(site.posts.load(Ordering::SeqCst), 1);
    contexts.close().unwrap();
    site.close().await;
}

#[tokio::test]
async fn http_redirect_limits_distinguish_disabled_following_from_exhaustion() {
    let site = Site::start(None).await;
    let mut contexts = Contexts::default();
    let connection = Uuid::new_v4();
    for limit in [0, 1] {
        let context = contexts
            .create(
                connection,
                Options {
                    base_url: Some(site.url.clone()),
                    max_redirects: limit,
                    ..Default::default()
                },
            )
            .unwrap();
        let context = serde_json::from_value(context["httpContext"].clone()).unwrap();
        let id = contexts
            .start(
                context,
                fetch("/loop"),
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        if limit == 0 {
            let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
                panic!("Unexpected witness");
            };
            assert_eq!(reply.metadata["status"], 302);
            assert_eq!(reply.metadata["redirectLimitReached"], false);
            contexts.restore(id, reply);
            contexts.discard(id).unwrap();
        } else {
            let error = match ready(&mut contexts, id).await {
                Err(error) => error,
                _ => panic!("Redirect limit ignored"),
            };
            assert_eq!(error.code, "HTTP_REDIRECT_LIMIT");
            assert_eq!(error.partial.unwrap()["redirects"], 1);
        }
        contexts.remove(context).unwrap();
    }
    contexts.close().unwrap();
    site.close().await;
}

#[tokio::test]
async fn request_query_and_redirect_options_override_context_defaults() {
    let site = Site::start(None).await;
    let mut contexts = Contexts::default();
    let context = create(&mut contexts, Uuid::new_v4(), &site.url);
    for (params, expected) in [
        (
            Parameters::Pairs(vec![
                QueryParameter {
                    name: "label".into(),
                    value: "Acme +&".into(),
                },
                QueryParameter {
                    name: "label".into(),
                    value: "東京".into(),
                },
            ]),
            "/query?old=one+two&label=Acme+%2B%26&label=%E6%9D%B1%E4%BA%AC",
        ),
        (
            Parameters::Encoded("label=Acme%20two".into()),
            "/query?label=Acme%20two",
        ),
    ] {
        let result = body(
            &mut contexts,
            context,
            Fetch {
                method: "gEt".into(),
                params: Some(params),
                ..fetch("/query?old=one%20two")
            },
        )
        .await;
        let result: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(result["target"], expected);
        assert_eq!(result["method"], "GET");
    }
    for limit in [0, 1] {
        let id = contexts
            .start(
                context,
                Fetch {
                    max_redirects: Some(limit),
                    ..fetch("/redirect")
                },
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
            panic!("Unexpected witness")
        };
        assert_eq!(reply.metadata["status"], if limit == 0 { 302 } else { 200 });
        assert_eq!(reply.metadata["redirects"], limit);
        contexts.restore(id, reply);
        contexts.discard(id).unwrap();
    }
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
    site.close().await;
}
