//! Redirect headers follow the pinned APIRequest wire transitions.
use super::*;
use crate::http_fixtures::defaults as fixture;

fn header(name: &str, value: &str) -> Header {
    Header {
        name: name.into(),
        value: value.into(),
    }
}
fn client(contexts: &mut Contexts, site: &fixture::Site) -> Reference {
    serde_json::from_value(
        contexts
            .create(
                Uuid::new_v4(),
                Options {
                    base_url: Some(site.url.clone()),
                    ..Default::default()
                },
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap()
}
async fn request(contexts: &mut Contexts, context: Reference, path: &str, headers: Vec<Header>) {
    let id = contexts
        .start(
            context,
            Fetch {
                headers,
                ..fetch(path)
            },
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    let Ready::Reply(reply) = ready(contexts, id).await.unwrap() else {
        panic!("Retained HTTP response")
    };
    assert_eq!(reply.metadata["status"], 200);
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
}
#[tokio::test]
async fn http_redirect_headers_retain_same_origin_authorization_and_explicit_values() {
    let site = fixture::Site::start(None).await;
    let mut contexts = Contexts::default();
    let context = client(&mut contexts, &site);
    request(
        &mut contexts,
        context,
        "/rewrite/307",
        vec![
            header("aUtHoRiZaTiOn", "Acme-private"),
            header("Proxy-Authorization", "Acme-proxy-private"),
            header("X-Acme", "Acme-value"),
        ],
    )
    .await;
    let rows = site.records();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row["headers"]["authorization"], "Acme-private");
        assert_eq!(row["headers"]["proxy-authorization"], "Acme-proxy-private");
        assert_eq!(row["headers"]["x-acme"], "Acme-value");
    }
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_redirect_headers_cross_origin_strip_authorization_preserving_other_explicit_values() {
    let other = fixture::Site::start(None).await;
    let site = fixture::Site::start(Some(other.url.clone())).await;
    let mut contexts = Contexts::default();
    let context = client(&mut contexts, &site);
    request(
        &mut contexts,
        context,
        "/cross",
        vec![
            header("Authorization", "Acme-private"),
            header("Proxy-Authorization", "Acme-proxy-private"),
            header("X-Acme", ""),
            header("Accept", "Acme-accept"),
            header("User-Agent", "Acme-agent"),
        ],
    )
    .await;
    let rows = other.records();
    assert_eq!(rows.len(), 1);
    let headers = &rows[0]["headers"];
    assert!(headers.get("authorization").is_none());
    assert_eq!(headers["proxy-authorization"], "Acme-proxy-private");
    assert_eq!(headers["x-acme"], "");
    assert_eq!(headers["accept"], "Acme-accept");
    assert_eq!(headers["user-agent"], "Acme-agent");
    assert_eq!(headers["host"], other.url.strip_prefix("http://").unwrap());
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_redirect_headers_refresh_cookie_and_host_on_same_origin_hops() {
    let site = fixture::Site::start(None).await;
    let mut contexts = Contexts::default();
    let context: Reference = serde_json::from_value(contexts.create(Uuid::new_v4(), Options {
        base_url: Some(site.url.clone()),
        storage_state: Some(StorageStateInput::Inline(json!({"cookies":[{"name":"jar","value":"Acme-jar","domain":"127.0.0.1","path":"/","expires":-1,"httpOnly":false,"secure":false,"sameSite":"Lax"}]}))),
        ..Default::default()
    }).unwrap()["httpContext"].clone()).unwrap();
    request(
        &mut contexts,
        context,
        "/rewrite/307",
        vec![
            header("Cookie", "caller=Acme-private"),
            header("Host", "acme.invalid"),
        ],
    )
    .await;
    let rows = site.records();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["headers"]["cookie"], "caller=Acme-private");
    assert_eq!(rows[0]["headers"]["host"], "acme.invalid");
    assert_eq!(rows[1]["headers"]["cookie"], "jar=Acme-jar");
    assert_eq!(
        rows[1]["headers"]["host"],
        site.url.strip_prefix("http://").unwrap()
    );
    contexts.close().unwrap();
}
