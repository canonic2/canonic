//! Pin HTTP defaults to observed requests rather than implementation-shaped assertions.
use super::*;
use crate::http_fixtures::defaults as fixture;

async fn request(
    contexts: &mut Contexts,
    context: Reference,
    request: Fetch,
) -> Result<Value, Error> {
    let id = contexts
        .start(context, request, Instant::now() + Duration::from_secs(10))
        .unwrap();
    let Ready::Reply(reply) = ready(contexts, id).await? else {
        panic!("Expected retained HTTP reply")
    };
    let metadata = reply.metadata.clone();
    contexts.restore(id, reply);
    contexts.discard(id).unwrap();
    Ok(metadata)
}
fn client(contexts: &mut Contexts, site: &fixture::Site, headers: Vec<Header>) -> Reference {
    serde_json::from_value(
        contexts
            .create(
                contexts.connection.unwrap_or_else(Uuid::new_v4),
                Options {
                    base_url: Some(site.url.clone()),
                    headers: Some(headers),
                    ..Default::default()
                },
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap()
}
#[tokio::test]
async fn http_request_defaults_twenty_hops_and_explicit_budget_boundaries() {
    assert_eq!(Options::default().max_redirects, 20);
    assert_eq!(
        schemas(&json!({}))[0]["properties"]["options"]["properties"]["maxRedirects"]["default"],
        20
    );
    let site = fixture::Site::start(None).await;
    let mut contexts = Contexts::default();
    let context = client(&mut contexts, &site, vec![]);
    let response = request(&mut contexts, context, fetch("/chain/20"))
        .await
        .unwrap();
    assert_eq!(response["redirects"], 20);
    assert_eq!(site.records().len(), 21);
    let response = request(
        &mut contexts,
        context,
        Fetch {
            max_redirects: Some(0),
            ..fetch("/chain/20")
        },
    )
    .await
    .unwrap();
    assert_eq!(response["status"], 302);
    assert_eq!(response["redirectLimitReached"], false);
    assert_eq!(site.records().len(), 22);
    let error = request(&mut contexts, context, fetch("/chain/21"))
        .await
        .unwrap_err();
    assert_eq!(error.code, "HTTP_REDIRECT_LIMIT");
    assert_eq!(error.partial.unwrap()["redirects"], 20);
    assert_eq!(site.records().len(), 43);
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_request_defaults_empty_locations_do_not_dispatch_extra_hops() {
    let site = fixture::Site::start(None).await;
    let mut contexts = Contexts::default();
    let context = client(&mut contexts, &site, vec![]);
    for path in ["/empty", "/missing"] {
        let before = site.records().len();
        let response = request(&mut contexts, context, fetch(path)).await.unwrap();
        assert_eq!(response["status"], 302);
        assert_eq!(response["redirects"], 0);
        assert_eq!(site.records().len(), before + 1);
    }
    let before = site.records().len();
    let error = request(
        &mut contexts,
        context,
        Fetch {
            max_redirects: Some(1),
            ..fetch("/exhaust")
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "HTTP_REDIRECT_LIMIT");
    assert_eq!(site.records().len(), before + 2);
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_request_defaults_rewrite_methods_bodies_and_all_entity_headers() {
    let site = fixture::Site::start(None).await;
    let mut contexts = Contexts::default();
    let names = [
        "content-type",
        "content-encoding",
        "content-language",
        "content-location",
    ];
    let context = client(
        &mut contexts,
        &site,
        names
            .iter()
            .map(|name| Header {
                name: (*name).into(),
                value: if *name == "content-encoding" {
                    "identity"
                } else {
                    "Acme"
                }
                .into(),
            })
            .collect(),
    );
    for (status, method, rewritten) in [
        (301, "pOsT", true),
        (302, "post", true),
        (303, "pUt", true),
        (303, "get", false),
        (303, "GET", false),
        (303, "head", false),
        (307, "POST", false),
        (308, "PUT", false),
    ] {
        let body = (!matches!(method, "get" | "head")).then(|| Payload::Text {
            text: "Acme body".into(),
        });
        request(
            &mut contexts,
            context,
            Fetch {
                method: method.into(),
                body,
                ..fetch(&format!("/rewrite/{status}"))
            },
        )
        .await
        .unwrap();
        let rows = site.records();
        let last = rows.last().unwrap();
        assert_eq!(
            last["method"],
            if rewritten {
                "GET".into()
            } else {
                method.to_ascii_uppercase()
            },
            "{last}"
        );
        assert_eq!(
            last["body"],
            if rewritten || matches!(method, "get" | "head") {
                ""
            } else {
                "Acme body"
            },
            "{last}"
        );
        for name in names {
            assert_eq!(last["headers"].get(name).is_some(), !rewritten, "{last}");
        }
        if rewritten {
            assert!(last["headers"].get("content-length").is_none(), "{last}");
        }
    }
    contexts.remove(context).unwrap();
    contexts.close().unwrap();
}
#[tokio::test]
async fn http_request_defaults_headers_override_case_insensitively_and_survive_cross_origin() {
    let other = fixture::Site::start(None).await;
    let site = fixture::Site::start(Some(other.url.clone())).await;
    let mut contexts = Contexts::default();
    let plain = client(&mut contexts, &site, vec![]);
    request(&mut contexts, plain, fetch("/echo")).await.unwrap();
    let rows = site.records();
    let head = &rows.last().unwrap()["headers"];
    assert_eq!(head["accept"], "*/*");
    assert_eq!(head["accept-encoding"], "gzip,deflate,br");
    let context = client(
        &mut contexts,
        &site,
        vec![
            Header {
                name: "Accept".into(),
                value: "Acme context".into(),
            },
            Header {
                name: "Accept-Encoding".into(),
                value: "identity".into(),
            },
            Header {
                name: "X-Acme".into(),
                value: "Acme private".into(),
            },
        ],
    );
    let overridden = Fetch {
        headers: vec![
            Header {
                name: "aCcEpT".into(),
                value: "Acme request".into(),
            },
            Header {
                name: "aCcEpT-EnCoDiNg".into(),
                value: String::new(),
            },
        ],
        ..fetch("/echo")
    };
    request(&mut contexts, context, overridden).await.unwrap();
    let rows = site.records();
    let head = &rows.last().unwrap()["headers"];
    assert_eq!(head["accept"], "Acme request");
    assert_eq!(head["accept-encoding"], "");
    request(&mut contexts, context, fetch("/cross"))
        .await
        .unwrap();
    let rows = other.records();
    let head = &rows.last().unwrap()["headers"];
    assert_eq!(head["accept"], "Acme context");
    assert_eq!(head["accept-encoding"], "identity");
    assert_eq!(head["x-acme"], "Acme private");
    for context in [plain, context] {
        contexts.remove(context).unwrap();
    }
    contexts.close().unwrap();
}

#[tokio::test]
async fn http_user_agent_isolated_wire_default_and_cross_origin_overrides_are_preserved() {
    let other = fixture::Site::start(None).await;
    let site = fixture::Site::start(Some(other.url.clone())).await;
    let mut contexts = Contexts::default();
    let plain = client(&mut contexts, &site, vec![]);
    request(&mut contexts, plain, fetch("/echo")).await.unwrap();
    let observed = site.records().last().unwrap()["headers"]["user-agent"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        observed.starts_with("Playwright/1.64.1 (") || observed == "Playwright/unknown",
        "{observed}"
    );
    for override_agent in [Some("Acme request agent"), Some(""), None] {
        let mut call = fetch("/echo");
        if let Some(agent) = override_agent {
            call.headers.push(Header {
                name: "uSeR-aGeNt".into(),
                value: agent.into(),
            });
        }
        request(&mut contexts, plain, call).await.unwrap();
        assert_eq!(
            site.records().last().unwrap()["headers"]["user-agent"],
            override_agent.unwrap_or(&observed)
        );
    }
    let mut cross = fetch("/cross");
    cross.headers.push(Header {
        name: "User-Agent".into(),
        value: "Acme request agent".into(),
    });
    request(&mut contexts, plain, cross).await.unwrap();
    assert_eq!(
        site.records().last().unwrap()["headers"]["user-agent"],
        "Acme request agent"
    );
    assert_eq!(
        other.records().last().unwrap()["headers"]["user-agent"],
        "Acme request agent"
    );
    let origin = client(
        &mut contexts,
        &site,
        vec![Header {
            name: "User-Agent".into(),
            value: "Acme origin agent".into(),
        }],
    );
    request(&mut contexts, origin, fetch("/cross"))
        .await
        .unwrap();
    assert_eq!(
        site.records().last().unwrap()["headers"]["user-agent"],
        "Acme origin agent"
    );
    assert_eq!(
        other.records().last().unwrap()["headers"]["user-agent"],
        "Acme origin agent"
    );
    assert_eq!(
        site.records().len() + other.records().len(),
        8,
        "Identity preparation must not issue any probe requests"
    );
    let empty: Reference = serde_json::from_value(
        contexts
            .create(
                contexts.connection.unwrap(),
                Options {
                    base_url: Some(site.url.clone()),
                    user_agent: Some(String::new()),
                    ..Default::default()
                },
            )
            .unwrap()["httpContext"]
            .clone(),
    )
    .unwrap();
    request(&mut contexts, empty, fetch("/echo")).await.unwrap();
    assert_eq!(
        site.records().last().unwrap()["headers"]["user-agent"],
        observed,
        "Empty option selects the default; it does not send an empty header"
    );
    contexts.remove(empty).unwrap();
    contexts.remove(plain).unwrap();
    contexts.remove(origin).unwrap();
    contexts.close().unwrap();
}
