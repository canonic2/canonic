//! Explicit browser cookie namespaces through the owned HTTPS loopback fixture.
use super::*;
use crate::browser::http::{CookiePartition, Fetch, Options};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{TlsAcceptor, rustls};

macro_rules! cell {
    ($name:ident,$engine:ident,$mode:ident) => {
        #[tokio::test]
        async fn $name() {
            run(Engine::$engine, Mode::$mode).await;
        }
    };
}
cell!(item_four_http_partitions_chrome_headless, Chrome, Headless);
cell!(item_four_http_partitions_chrome_headed, Chrome, Headed);
cell!(
    item_four_http_partitions_firefox_headless,
    Firefox,
    Headless
);
cell!(item_four_http_partitions_firefox_headed, Firefox, Headed);
cell!(item_four_http_partitions_webkit_headless, Webkit, Headless);
cell!(item_four_http_partitions_webkit_headed, Webkit, Headed);

struct Site {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Site {
    async fn start() -> Self {
        let cert =
            CertificateDer::from_pem_slice(include_bytes!("../http/fixtures/server.pem")).unwrap();
        let key = PrivateKeyDer::from_pem_slice(include_bytes!("../http/fixtures/server-key.pem"))
            .unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!(
            "https://localhost:{}",
            listener.local_addr().unwrap().port()
        );
        let task = tokio::spawn(async move {
            let mut calls = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    incoming=listener.accept()=>{
                        let(stream,_)=incoming.unwrap();let acceptor=acceptor.clone();
                        calls.spawn(async move {
                            let Ok(mut stream)=acceptor.accept(stream).await else{return;};
                            let mut bytes=Vec::new();let mut buffer=[0;1024];
                            while !bytes.windows(4).any(|part|part==b"\r\n\r\n") {
                                let Ok(count)=stream.read(&mut buffer).await else{return;};
                                if count==0||bytes.len()+count>8192{return;}bytes.extend_from_slice(&buffer[..count]);
                            }
                            let header=String::from_utf8_lossy(&bytes);
                            let field=|name:&str|header.lines().find_map(|line|line.split_once(':').filter(|(key,_)|key.eq_ignore_ascii_case(name)).map(|(_,value)|value.trim())).unwrap_or("");
                            let value=if header.starts_with("GET /set-a "){Some("Acme-A")}else if header.starts_with("GET /set-b "){Some("Acme-B")}else if header.starts_with("GET /set-c "){Some("Acme-C")}else{None};
                            let cookies=value.map(|value|format!("Set-Cookie: plain=Acme-shared; Secure; HttpOnly; SameSite=None; Path=/\r\nSet-Cookie: part={value}; Secure; HttpOnly; Partitioned; SameSite=None; Path=/\r\n")).unwrap_or_default();
                            let body=if header.starts_with("GET /echo "){field("cookie")}else if header.starts_with("GET /agent "){field("user-agent")}else{"Acme"};
                            let origin=field("origin");
                            let cors=if origin.is_empty(){String::new()}else{format!("Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Credentials: true\r\n")};
                            let response=format!("HTTP/1.1 200 OK\r\n{cookies}{cors}Content-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                            let _=stream.write_all(response.as_bytes()).await;
                        let _=stream.shutdown().await;
                        });
                    },
                    _=calls.join_next(),if !calls.is_empty()=>{}
                }
            }
        });
        Self { url, task }
    }
}
fn partition(site: &str, ancestor: Option<bool>) -> CookiePartition {
    CookiePartition {
        top_level_site: site.into(),
        has_cross_site_ancestor: ancestor,
    }
}
fn create(context: Reference, site: &Site, key: Option<CookiePartition>) -> Request {
    Request::HttpContextCreate {
        options: Options {
            browser_context: Some(context),
            cookie_partition: key,
            base_url: Some(site.url.clone()),
            ignore_https_errors: Some(true),
            ..Default::default()
        },
    }
}
fn reference(value: &Value, key: &str) -> Reference {
    serde_json::from_value(value[key].clone()).unwrap()
}
async fn fetch(session: &Session, context: Reference, path: &str) -> String {
    let request: Fetch = serde_json::from_value(json!({"url":path,"readBody":true})).unwrap();
    let value = successful(
        session,
        Request::HttpFetch {
            http_context: context,
            request,
        },
    )
    .await;
    assert_eq!(value["status"], 200);
    assert_ne!(value["browserCookieUpdate"]["state"], "unknown", "{value}");
    assert!(!value.to_string().contains("Acme-A"));
    assert!(!value.to_string().contains("Acme-B"));
    let body =
        std::fs::read_to_string(value["body"]["attachment"]["path"].as_str().unwrap()).unwrap();
    successful(
        session,
        Request::HttpEvidenceRemove {
            evidence_id: serde_json::from_value(value["body"]["evidenceId"].clone()).unwrap(),
        },
    )
    .await;
    body
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
            context: if engine == Engine::Chrome {
                ContextOptions::default()
            } else {
                ContextOptions {
                    user_agent: Some("Acme inherited browser agent".into()),
                    ignore_https_errors: Some(true),
                    ..Default::default()
                }
            },
        },
        &directory,
    )
    .unwrap();
    let result = std::panic::AssertUnwindSafe(async {
        let ready = settled(&session, session.startup()).await;
        assert_eq!(ready.outcome, Outcome::Succeeded, "{ready:?}");
        let context = reference(&ready.result.unwrap(), "context");
        let a = create(
            context,
            &site,
            Some(partition("https://acme.example", None)),
        );
        if engine == Engine::Firefox {
            let refused =
                settled(&session, session.submit(Uuid::new_v4(), a, 30000).unwrap()).await;
            assert_eq!(
                refused.code.as_deref(),
                Some("HTTP_COOKIE_PARTITION_UNSUPPORTED")
            );
            firefox_cookie_identity_probe(&session, &site).await;
            return;
        }
        let a = successful(&session, a).await;
        assert_eq!(a["cookiePartitionConfigured"], true);
        let a = reference(&a, "httpContext");
        let b = reference(
            &successful(
                &session,
                create(
                    context,
                    &site,
                    Some(partition("https://other.example", None)),
                ),
            )
            .await,
            "httpContext",
        );
        let mut ordinary_request = create(context, &site, None);
        if let Request::HttpContextCreate { options } = &mut ordinary_request {
            if engine == Engine::Chrome {
                options.user_agent = Some("Acme inherited browser agent".into());
            } else {
                options.ignore_https_errors = None;
            }
        }
        let ordinary = reference(&successful(&session, ordinary_request).await, "httpContext");
        assert_eq!(
            fetch(&session, ordinary, "/agent").await,
            "Acme inherited browser agent"
        );
        let inventory = successful(&session, Request::HttpContextList {}).await;
        let client = inventory["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|client| client["httpContext"] == json!(ordinary))
            .unwrap();
        assert_eq!(client["tlsVerification"], false);
        assert_eq!(client["userAgentConfigured"], true);
        fetch(&session, a, "/set-a").await;
        assert_eq!(
            fetch(&session, ordinary, "/echo").await,
            "plain=Acme-shared"
        );
        assert!(fetch(&session, a, "/echo").await.contains("part=Acme-A"));
        assert!(!fetch(&session, b, "/echo").await.contains("part="));
        fetch(&session, b, "/set-b").await;
        let first = fetch(&session, a, "/echo").await;
        assert!(first.contains("part=Acme-A"));
        assert!(!first.contains("part=Acme-B"));
        assert!(fetch(&session, b, "/echo").await.contains("part=Acme-B"));
        let same = create(
            context,
            &site,
            Some(partition("https://acme.example", Some(false))),
        );
        if engine == Engine::Chrome {
            let same = reference(&successful(&session, same).await, "httpContext");
            assert!(!fetch(&session, same, "/echo").await.contains("part="));
            fetch(&session, same, "/set-c").await;
            assert!(fetch(&session, a, "/echo").await.contains("part=Acme-A"));
            assert!(fetch(&session, same, "/echo").await.contains("part=Acme-C"));
        } else {
            let refused = settled(
                &session,
                session.submit(Uuid::new_v4(), same, 30000).unwrap(),
            )
            .await;
            assert_eq!(
                refused.code.as_deref(),
                Some("HTTP_COOKIE_PARTITION_ANCESTRY_UNSUPPORTED")
            );
        }
        let other = reference(
            &successful(
                &session,
                Request::ContextCreate {
                    options: Default::default(),
                },
            )
            .await,
            "context",
        );
        let other = reference(
            &successful(
                &session,
                create(other, &site, Some(partition("https://acme.example", None))),
            )
            .await,
            "httpContext",
        );
        assert!(fetch(&session, other, "/echo").await.is_empty());
    })
    .catch_unwind()
    .await;
    let closed = settled(&session, session.disconnect(30000).unwrap()).await;
    assert_eq!(closed.outcome, Outcome::Succeeded, "{closed:?}");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
async fn firefox_cookie_identity_probe(session: &Session, site: &Site) {
    let context = reference(
        &successful(
            session,
            Request::ContextCreate {
                options: ContextOptions {
                    ignore_https_errors: Some(true),
                    user_agent: Some("Acme inherited browser agent".into()),
                    ..Default::default()
                },
            },
        )
        .await,
        "context",
    );
    let page = reference(
        &successful(
            session,
            Request::PageCreate {
                context,
                select: false,
            },
        )
        .await,
        "page",
    );
    successful(
        session,
        Request::PageNavigate {
            page,
            navigation: Navigation::Goto {
                url: format!("{}/set-a", site.url),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    let other = reference(
        &successful(
            session,
            Request::PageCreate {
                context,
                select: false,
            },
        )
        .await,
        "page",
    );
    successful(
        session,
        Request::PageNavigate {
            page: other,
            navigation: Navigation::Goto {
                url: format!("{}/blank", site.url.replace("localhost", "127.0.0.1")),
                wait_until: Load::Load,
            },
        },
    )
    .await;
    let browser = successful(
        session,
        Request::Evaluate {
            scope: other,
            expression: format!(
                "async()=>await(await fetch({},{{credentials:'include'}})).text()",
                json!(format!("{}/echo", site.url))
            ),
            is_function: Some(true),
            argument: Default::default(),
            return_handle: false,
        },
    )
    .await;
    let browser = browser["value"]["value"].as_str().unwrap();
    assert!(
        browser.contains("plain=Acme-shared"),
        "Cross-site cookie probe did not establish the unpartitioned control: {browser}"
    );
    assert!(
        !browser.contains("part=Acme-A"),
        "The browser did not isolate the partitioned control cookie: {browser}"
    );
    let refused = settled(
        session,
        session
            .submit(Uuid::new_v4(), create(context, site, None), 30000)
            .unwrap(),
    )
    .await;
    assert_eq!(
        refused.code.as_deref(),
        Some("HTTP_COOKIE_PARTITION_IDENTITY_UNAVAILABLE")
    );
    assert_eq!(refused.result.as_ref().unwrap()["created"], false);
    let mut explicit = create(context, site, None);
    let Request::HttpContextCreate { options } = &mut explicit else {
        unreachable!()
    };
    options.allow_unknown_cookie_partitions = true;
    options.ignore_https_errors = None;
    let explicit = successful(session, explicit).await;
    assert_eq!(explicit["cookiePartitionIdentityAvailable"], false);
    assert_eq!(explicit["unknownCookiePartitionsAllowed"], true);
    let host = reference(&explicit, "httpContext");
    assert_eq!(
        fetch(session, host, "/agent").await,
        "Acme inherited browser agent"
    );
    let host = fetch(session, host, "/echo").await;
    assert!(
        host.contains("part=Acme-A") && host.contains("plain=Acme-shared"),
        "Explicit Firefox backend view changed unexpectedly: {host}"
    );
}
