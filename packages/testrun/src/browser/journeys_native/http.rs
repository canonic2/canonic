//! Isolated and browser-associated HTTP clients through each owned browser backend.
use super::investigation::Site;
use super::*;
use crate::browser::{
    contract::CallbackResponse,
    http::{Fetch, Options, Payload},
};
use std::sync::atomic::Ordering;

macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_http_chrome_headless, Chrome, Headless);
cell!(item_four_http_chrome_headed, Chrome, Headed);
cell!(item_four_http_firefox_headless, Firefox, Headless);
cell!(item_four_http_firefox_headed, Firefox, Headed);
cell!(item_four_http_webkit_headless, Webkit, Headless);
cell!(item_four_http_webkit_headed, Webkit, Headed);
fn request(url: &str, body: bool) -> Fetch {
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
        read_body: body,
        max_bytes: 65536,
    }
}
fn bytes(value: &Value) -> Vec<u8> {
    std::fs::read(
        value["body"]["attachment"]["path"]
            .as_str()
            .expect("Private HTTP attachment"),
    )
    .unwrap()
}
async fn run(engine: Engine, mode: Mode) {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let site = Site::start().await;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("state");
    crate::artifacts::directory(&directory).unwrap();
    let bundle = if engine == Engine::Chrome {
        Bundle::Chrome(
            super::super::preparation::prepare_chrome(|_| {})
                .await
                .unwrap(),
        )
    } else {
        Bundle::Playwright(
            super::super::preparation::prepare(engine, |_| {})
                .await
                .unwrap(),
        )
    };
    let session = Session::launch_connection(
        bundle,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        LaunchOptions {
            media_fixtures: Default::default(),
            proxy: None,
            engine,
            mode,
            context: Default::default(),
        },
        &directory,
    )
    .unwrap();
    let result = std::panic::AssertUnwindSafe(async {
        let ready = settled(&session, session.startup()).await;
        assert_eq!(ready.outcome, Outcome::Succeeded, "{ready:?}");
        let ready = ready.result.unwrap();
        let page: Reference = serde_json::from_value(ready["page"].clone()).unwrap();
        let context: Reference = serde_json::from_value(ready["context"].clone()).unwrap();
        successful(
            &session,
            Request::PageNavigate {
                page,
                navigation: Navigation::Goto {
                    url: format!("{}/audit", site.url),
                    wait_until: Load::Load,
                },
            },
        )
        .await;
        let create = || Request::HttpContextCreate {
            options: Options {
                base_url: Some(site.url.clone()),
                ..Default::default()
            },
        };
        let first = successful(&session, create()).await;
        let first: Reference = serde_json::from_value(first["httpContext"].clone()).unwrap();
        let second = successful(&session, create()).await;
        let second: Reference = serde_json::from_value(second["httpContext"].clone()).unwrap();
        let initial = successful(
            &session,
            Request::HttpFetch {
                http_context: first,
                request: request("/http-cookie", true),
            },
        )
        .await;
        assert!(
            bytes(&initial).is_empty(),
            "Browser cookies leaked into isolated HTTP client"
        );
        let own = successful(
            &session,
            Request::HttpFetch {
                http_context: first,
                request: request("/http-cookie", true),
            },
        )
        .await;
        assert_eq!(bytes(&own), b"Acme=Acme-secret");
        let isolated = successful(
            &session,
            Request::HttpFetch {
                http_context: second,
                request: request("/http-cookie", true),
            },
        )
        .await;
        assert!(bytes(&isolated).is_empty());
        successful(
            &session,
            Request::HttpContextCookiesClear {
                http_context: first,
                options: Default::default(),
            },
        )
        .await;
        let browser = successful(
            &session,
            Request::Evaluate {
                scope: page,
                expression: "document.cookie".into(),
                is_function: Some(false),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        assert!(
            browser["value"]["value"]
                .as_str()
                .unwrap()
                .contains("Acme=Acme-secret")
        );
        let associated = successful(
            &session,
            Request::HttpContextCreate {
                options: Options {
                    browser_context: Some(context),
                    allow_unknown_cookie_partitions: engine == Engine::Firefox,
                    base_url: Some(site.url.clone()),
                    ..Default::default()
                },
            },
        )
        .await;
        assert_eq!(associated["browserAssociated"], true);
        let associated = serde_json::from_value(associated["httpContext"].clone()).unwrap();
        successful(
            &session,
            Request::Evaluate {
                scope: page,
                expression: "document.cookie='FromPage=Acme-page; Path=/'".into(),
                is_function: Some(false),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        let shared = successful(
            &session,
            Request::HttpFetch {
                http_context: associated,
                request: request("/http-associated", true),
            },
        )
        .await;
        let shared_bytes = String::from_utf8(bytes(&shared)).unwrap();
        assert!(shared_bytes.contains("Acme=Acme-secret"), "{shared_bytes}");
        assert!(
            shared_bytes.contains("FromPage=Acme-page"),
            "{shared_bytes}"
        );
        assert_eq!(
            shared["browserCookieUpdate"]["state"], "acknowledged",
            "{shared}"
        );
        let browser = successful(
            &session,
            Request::Evaluate {
                scope: page,
                expression: "()=>fetch('/http-associated').then(response=>response.text())".into(),
                is_function: Some(true),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        assert!(
            browser["value"]["value"]
                .as_str()
                .unwrap()
                .contains("FromHttp=Acme-response"),
            "{browser}"
        );
        successful(
            &session,
            Request::HttpContextCookiesClear {
                http_context: associated,
                options: Default::default(),
            },
        )
        .await;
        let empty = successful(
            &session,
            Request::Evaluate {
                scope: page,
                expression: "document.cookie".into(),
                is_function: Some(false),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        assert_eq!(empty["value"]["value"], "");
        let cleared = successful(
            &session,
            Request::HttpFetch {
                http_context: associated,
                request: request("/http-associated", true),
            },
        )
        .await;
        assert!(
            bytes(&cleared).is_empty(),
            "HTTP-only cookies survived clearing: {cleared}"
        );
        successful(
            &session,
            Request::HttpContextRemove {
                http_context: associated,
            },
        )
        .await;
        let preserved = successful(
            &session,
            Request::Evaluate {
                scope: page,
                expression: "()=>fetch('/http-cookie').then(response=>response.text())".into(),
                is_function: Some(true),
                argument: Default::default(),
                return_handle: false,
            },
        )
        .await;
        assert!(
            preserved["value"]["value"]
                .as_str()
                .unwrap()
                .contains("FromHttp=Acme-response"),
            "Removing an HTTP client removed browser cookies: {preserved}"
        );
        let other = successful(
            &session,
            Request::ContextCreate {
                options: Default::default(),
            },
        )
        .await;
        let other = serde_json::from_value(other["context"].clone()).unwrap();
        let retired = successful(
            &session,
            Request::HttpContextCreate {
                options: Options {
                    browser_context: Some(other),
                    allow_unknown_cookie_partitions: engine == Engine::Firefox,
                    base_url: Some(site.url.clone()),
                    ..Default::default()
                },
            },
        )
        .await;
        let retired = serde_json::from_value(retired["httpContext"].clone()).unwrap();
        successful(&session, Request::ContextClose { context: other }).await;
        let stale = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::HttpFetch {
                        http_context: retired,
                        request: request("/http-associated", true),
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(stale.code.as_deref(), Some("STALE_REFERENCE"), "{stale:?}");
        successful(
            &session,
            Request::HttpContextRemove {
                http_context: retired,
            },
        )
        .await;
        let post = successful(
            &session,
            Request::HttpFetch {
                http_context: first,
                request: Fetch {
                    method: "POST".into(),
                    body: Some(Payload::Text {
                        text: "Acme request secret".into(),
                    }),
                    ..request("/api?token=Acme-http-private", true)
                },
            },
        )
        .await;
        assert_eq!(post["status"], 409);
        assert_eq!(bytes(&post), b"Acme response secret");
        assert!(!post.to_string().contains("Acme response secret"));
        assert!(!post.to_string().contains("Acme-http-private"));
        let request_id = serde_json::from_value(post["requestId"].clone()).unwrap();
        let repeated = successful(&session, Request::HttpResponseFinalize { request_id }).await;
        assert_eq!(repeated, post);
        assert_eq!(site.posts.load(Ordering::SeqCst), 1);
        if engine == Engine::Chrome && mode == Mode::Headless {
            let evidence =
                std::path::Path::new(post["body"]["attachment"]["path"].as_str().unwrap())
                    .parent()
                    .unwrap()
                    .to_path_buf();
            let saved = evidence.with_file_name("http-saved");
            std::fs::rename(&evidence, &saved).unwrap();
            let fault = std::panic::AssertUnwindSafe(async {
                std::fs::write(&evidence, b"Acme evidence blocker").unwrap();
                settled(
                    &session,
                    session
                        .submit(
                            Uuid::new_v4(),
                            Request::HttpFetch {
                                http_context: first,
                                request: Fetch {
                                    method: "POST".into(),
                                    body: Some(Payload::Text {
                                        text: "Acme request secret".into(),
                                    }),
                                    ..request("/api?token=Acme-http-private", true)
                                },
                            },
                            30000,
                        )
                        .unwrap(),
                )
                .await
            })
            .catch_unwind()
            .await;
            if evidence.is_file() {
                std::fs::remove_file(&evidence).unwrap();
            }
            std::fs::rename(&saved, &evidence).unwrap();
            let fault = fault.unwrap();
            assert_eq!(fault.code.as_deref(), Some("EVIDENCE_IO"), "{fault:?}");
            let retained = fault.result.unwrap();
            assert_eq!(retained["responseRetained"], true);
            let request_id = serde_json::from_value(retained["requestId"].clone()).unwrap();
            let recovered =
                successful(&session, Request::HttpResponseFinalize { request_id }).await;
            assert_eq!(recovered["status"], 409);
            assert_eq!(bytes(&recovered), b"Acme response secret");
            assert_eq!(
                successful(&session, Request::HttpResponseFinalize { request_id }).await,
                recovered
            );
            assert_eq!(site.posts.load(Ordering::SeqCst), 2);
        }
        let selected_file = root.path().join("Acme-selected.JSON");
        let selected_bytes = b"Acme selected file private\0\xff";
        std::fs::write(&selected_file, selected_bytes).unwrap();
        let uploader = successful(
            &session,
            Request::HttpContextCreate {
                options: Options {
                    browser_context: Some(context),
                    allow_unknown_cookie_partitions: engine == Engine::Firefox,
                    base_url: Some(site.url.clone()),
                    ..Default::default()
                },
            },
        )
        .await;
        let uploader: Reference = serde_json::from_value(uploader["httpContext"].clone()).unwrap();
        let before_upload = site.posts.load(Ordering::SeqCst);
        for http_context in [first, uploader] {
            let upload = successful(
                &session,
                Request::HttpFetch {
                    http_context,
                    request: Fetch {
                        method: "POST".into(),
                        body: Some(Payload::Multipart {
                            fields: vec![
                                crate::browser::http::MultipartPart::FilePath {
                                    name: "upload".into(),
                                    path: selected_file.to_str().unwrap().into(),
                                    filename: None,
                                    mime_type: String::new(),
                                },
                                crate::browser::http::MultipartPart::File {
                                    name: "inline".into(),
                                    filename: "Acme.xml".into(),
                                    mime_type: String::new(),
                                    data: "AP8=".into(),
                                },
                            ],
                        }),
                        ..request("/http-upload", true)
                    },
                },
            )
            .await;
            let body = bytes(&upload);
            let text = String::from_utf8_lossy(&body);
            assert!(
                text.contains("filename=\"Acme-selected.JSON\"\r\nContent-Type: application/json")
            );
            assert!(text.contains("filename=\"Acme.xml\"\r\nContent-Type: application/xml"));
            assert!(
                body.windows(selected_bytes.len())
                    .any(|bytes| bytes == selected_bytes)
            );
            assert!(body.windows(2).any(|bytes| bytes == [0, 255]));
            assert!(!upload.to_string().contains("Acme selected file private"));
            let request_id = serde_json::from_value(upload["requestId"].clone()).unwrap();
            assert_eq!(
                successful(&session, Request::HttpResponseFinalize { request_id }).await,
                upload
            );
            successful(
                &session,
                Request::HttpEvidenceRemove {
                    evidence_id: serde_json::from_value(upload["body"]["evidenceId"].clone())
                        .unwrap(),
                },
            )
            .await;
        }
        assert_eq!(site.posts.load(Ordering::SeqCst), before_upload + 2);
        successful(
            &session,
            Request::HttpContextRemove {
                http_context: uploader,
            },
        )
        .await;
        let missing = Fetch {
            method: "POST".into(),
            body: Some(Payload::Multipart {
                fields: vec![crate::browser::http::MultipartPart::FilePath {
                    name: "upload".into(),
                    path: root.path().join("missing.bin").to_str().unwrap().into(),
                    filename: None,
                    mime_type: String::new(),
                }],
            }),
            ..request("/http-upload", true)
        };
        let missing = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::HttpFetch {
                        http_context: first,
                        request: missing,
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(missing.code.as_deref(), Some("HTTP_FILE_IO"));
        assert_eq!(site.posts.load(Ordering::SeqCst), before_upload + 2);
        let bounded = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::HttpFetch {
                        http_context: first,
                        request: Fetch {
                            max_bytes: 8,
                            ..request("/audit", true)
                        },
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(bounded.code.as_deref(), Some("HTTP_BODY_BOUNDS"));
        responsive(&session, page, first, &site).await;
        let posts_before_loss = site.posts.load(Ordering::SeqCst);
        let lost = settled(
            &session,
            session
                .submit(
                    Uuid::new_v4(),
                    Request::HttpFetch {
                        http_context: first,
                        request: Fetch {
                            method: "POST".into(),
                            body: Some(Payload::Text {
                                text: "Acme request secret".into(),
                            }),
                            ..request("/http-drop", true)
                        },
                    },
                    30000,
                )
                .unwrap(),
        )
        .await;
        assert_eq!(lost.outcome, Outcome::Unknown, "{lost:?}");
        assert_eq!(lost.code.as_deref(), Some("HTTP_REQUEST_OUTCOME_UNKNOWN"));
        assert_eq!(site.posts.load(Ordering::SeqCst), posts_before_loss + 1);
        assert_eq!(session.status()["phase"], "ready");
        assert_eq!(
            successful(
                &session,
                Request::Evaluate {
                    scope: page,
                    expression: "document.title".into(),
                    is_function: Some(false),
                    argument: Default::default(),
                    return_handle: false
                }
            )
            .await["value"]["value"],
            "Acme audit 東京🙂"
        );
        let evidence_id = serde_json::from_value(post["body"]["evidenceId"].clone()).unwrap();
        successful(&session, Request::HttpEvidenceRemove { evidence_id }).await;
        successful(
            &session,
            Request::HttpContextRemove {
                http_context: first,
            },
        )
        .await;
        successful(
            &session,
            Request::HttpContextRemove {
                http_context: second,
            },
        )
        .await;
        assert!(
            successful(&session, Request::HttpContextList {}).await["contexts"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    })
    .catch_unwind()
    .await;
    site.release.notify_one();
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
async fn responsive(session: &Session, page: Reference, http_context: Reference, site: &Site) {
    successful(
        session,
        Request::Evaluate {
            scope: page,
            expression: "setTimeout(()=>alert('Acme HTTP dialog'),100);'Acme scheduled'".into(),
            is_function: Some(false),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    let pending = session
        .submit(
            Uuid::new_v4(),
            Request::HttpFetch {
                http_context,
                request: request("/http-hold", false),
            },
            30000,
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let callback = loop {
        let status = session.status();
        let resources = status["callbacks"]
            .as_array()
            .or_else(|| status["resources"].as_array())
            .unwrap();
        if let Some(callback) = resources.iter().find(|item| item["kind"] == "dialog") {
            break serde_json::from_value(callback["reference"].clone()).unwrap();
        }
        assert!(
            Instant::now() < until,
            "Dialog not available during HTTP request: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let list = successful(session, Request::HttpContextList {}).await;
    assert!(
        list["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |item| item["httpContext"] == serde_json::json!(http_context)
                    && item["active"] == true
            )
    );
    let busy = settled(
        session,
        session
            .submit(
                Uuid::new_v4(),
                Request::HttpContextRemove { http_context },
                30000,
            )
            .unwrap(),
    )
    .await;
    assert_eq!(busy.code.as_deref(), Some("HTTP_CONTEXT_BUSY"));
    successful(
        session,
        Request::CallbackResolve {
            callback,
            response: CallbackResponse::Dialog {
                accept: true,
                prompt_text: None,
            },
        },
    )
    .await;
    site.release.notify_one();
    let complete = settled(session, pending).await;
    assert_eq!(complete.outcome, Outcome::Succeeded, "{complete:?}");
}
