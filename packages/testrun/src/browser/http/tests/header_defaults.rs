//! Wire-observed live defaults, explicit empty overrides and uncertainty fences.
use super::*;
use crate::http_fixtures::defaults as wire;

#[tokio::test]
async fn header_wire_fixture_allows_progress_behind_an_idle_connection() {
    use tokio::io::AsyncWriteExt;
    let site = wire::Site::start(None).await;
    let mut idle = tokio::net::TcpStream::connect(site.url.strip_prefix("http://").unwrap())
        .await
        .unwrap();
    // A browser may preconnect or send only a partial request. Keep that
    // connection open until a separate actual HTTP request has completed.
    idle.write_all(b"G").await.unwrap();
    let mut contexts = Contexts::default();
    let client = create(&mut contexts, Uuid::new_v4(), &site.url);
    let id = contexts
        .start(
            client,
            fetch("/echo"),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    let Ready::Reply(reply) = ready(&mut contexts, id).await.unwrap() else {
        panic!("Expected concurrent request progress")
    };
    assert_eq!(reply.metadata["status"], 200);
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
    contexts.close().unwrap();
    drop(idle);
}

struct Browser;
impl browser::BrowserCookies for Browser {
    fn partition_identity_available(&self) -> bool {
        true
    }
    fn read(&self, _: Instant) -> Result<Value, Error> {
        Ok(json!([]))
    }
    fn write(&self, _: Value, _: Instant) -> Result<(), Error> {
        Ok(())
    }
}
fn headers(value: &str) -> Vec<Header> {
    vec![Header {
        name: "X-Acme".into(),
        value: value.into(),
    }]
}
fn browser_context() -> Reference {
    Reference {
        connection_id: Uuid::new_v4(),
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    }
}
fn options(
    context: Reference,
    base: Option<&str>,
    override_headers: Option<Vec<Header>>,
) -> Options {
    Options {
        browser_context: Some(context),
        base_url: base.map(String::from),
        headers: override_headers,
        ..Default::default()
    }
}
fn create_client(
    contexts: &mut Contexts,
    mut options: Options,
    initial: &[Header],
) -> Result<Reference, Error> {
    let context = options.browser_context.unwrap();
    contexts.inherit_headers(&mut options, context, initial);
    let created =
        contexts.create_with_browser(context.connection_id, options, Some(Arc::new(Browser)))?;
    Ok(serde_json::from_value(created["httpContext"].clone()).unwrap())
}
async fn send(contexts: &mut Contexts, client: Reference, fetch: Fetch) {
    let id = contexts
        .start(client, fetch, Instant::now() + Duration::from_secs(5))
        .unwrap();
    let Ready::Reply(reply) = ready(contexts, id).await.unwrap() else {
        panic!("Expected owned reply")
    };
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
}
#[tokio::test]
async fn http_header_defaults_follow_context_changes_with_explicit_empty_and_request_precedence() {
    let site = wire::Site::start(None).await;
    let context = browser_context();
    let mut contexts = Contexts::default();
    let inherited = create_client(
        &mut contexts,
        options(context, Some(&site.url), None),
        &headers("initial"),
    )
    .unwrap();
    let empty = create_client(
        &mut contexts,
        options(context, Some(&site.url), Some(vec![])),
        &headers("initial"),
    )
    .unwrap();
    let explicit = create_client(
        &mut contexts,
        options(context, Some(&site.url), Some(headers("explicit"))),
        &headers("initial"),
    )
    .unwrap();
    for (value, expected) in [
        (None, "initial"),
        (Some("changed"), "changed"),
        (Some("again"), "again"),
    ] {
        if let Some(value) = value {
            let _ = contexts.context_headers_pending(context);
            contexts.context_headers_applied(context, &headers(value));
        }
        send(&mut contexts, inherited, fetch("/echo")).await;
        assert_eq!(
            site.records().last().unwrap()["headers"]["x-acme"],
            expected
        );
        send(&mut contexts, empty, fetch("/echo")).await;
        assert!(
            site.records().last().unwrap()["headers"]
                .get("x-acme")
                .is_none()
        );
        send(&mut contexts, explicit, fetch("/echo")).await;
        assert_eq!(
            site.records().last().unwrap()["headers"]["x-acme"],
            "explicit"
        );
        send(
            &mut contexts,
            inherited,
            Fetch {
                headers: vec![Header {
                    name: "x-aCmE".into(),
                    value: "request".into(),
                }],
                ..fetch("/echo")
            },
        )
        .await;
        assert_eq!(
            site.records().last().unwrap()["headers"]["x-acme"],
            "request"
        );
    }
    contexts.context_headers_applied(context, &[]);
    send(&mut contexts, inherited, fetch("/echo")).await;
    assert!(
        site.records().last().unwrap()["headers"]
            .get("x-acme")
            .is_none()
    );
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_header_defaults_uncertainty_refuses_before_dispatch_and_allocation_until_acknowledged()
 {
    let site = wire::Site::start(None).await;
    let context = browser_context();
    let mut contexts = Contexts::default();
    let inherited = create_client(
        &mut contexts,
        options(context, Some(&site.url), None),
        &headers("initial"),
    )
    .unwrap();
    let explicit = create_client(
        &mut contexts,
        options(context, Some(&site.url), Some(vec![])),
        &[],
    )
    .unwrap();
    let _ = contexts.context_headers_pending(context);
    let error = contexts
        .start(
            inherited,
            fetch("/echo"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap_err();
    assert_eq!(error.code, "HTTP_CONTEXT_HEADERS_UNCERTAIN");
    assert!(contexts.jobs.is_empty());
    assert!(site.records().is_empty());
    assert_eq!(
        create_client(
            &mut contexts,
            options(context, Some(&site.url), None),
            &headers("old")
        )
        .unwrap_err()
        .code,
        "HTTP_CONTEXT_HEADERS_UNCERTAIN"
    );
    send(&mut contexts, explicit, fetch("/echo")).await;
    assert_eq!(site.records().len(), 1);
    contexts.context_headers_applied(context, &headers("recovered"));
    send(&mut contexts, inherited, fetch("/echo")).await;
    assert_eq!(
        site.records().last().unwrap()["headers"]["x-acme"],
        "recovered"
    );
    contexts.context_headers_closed(context);
    assert_eq!(
        contexts
            .start(
                inherited,
                fetch("/echo"),
                Instant::now() + Duration::from_secs(5)
            )
            .unwrap_err()
            .code,
        "HTTP_CONTEXT_HEADERS_UNCERTAIN"
    );
    assert!(contexts.browser_headers.is_empty());
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_header_defaults_reach_each_initial_origin_after_empty_to_nonempty_changes() {
    let site = wire::Site::start(None).await;
    let other = wire::Site::start(None).await;
    let context = browser_context();
    let mut contexts = Contexts::default();
    let no_base = create_client(&mut contexts, options(context, None, None), &[]).unwrap();
    let based = create_client(&mut contexts, options(context, Some(&site.url), None), &[]).unwrap();
    contexts.context_headers_applied(context, &headers("private"));
    for client in [no_base, based] {
        for target in [&site, &other] {
            send(
                &mut contexts,
                client,
                fetch(&format!("{}/echo", target.url)),
            )
            .await;
            assert_eq!(
                target.records().last().unwrap()["headers"]["x-acme"],
                "private"
            );
        }
    }
    let mut explicit = fetch(&format!("{}/echo", other.url));
    explicit.headers = headers("request");
    send(&mut contexts, based, explicit).await;
    assert_eq!(
        other.records().last().unwrap()["headers"]["x-acme"],
        "request"
    );
    contexts.context_headers_applied(context, &[]);
    send(&mut contexts, no_base, fetch(&format!("{}/echo", site.url))).await;
    assert!(
        site.records().last().unwrap()["headers"]
            .get("x-acme")
            .is_none()
    );
    assert!(contexts.jobs.is_empty());
    assert_eq!(site.records().len(), 3);
    assert_eq!(other.records().len(), 3);
    contexts.close().unwrap();
}
