use super::*;
use crate::browser::http::cookie_admin as admin;

#[test]
fn cookie_filter_expired_name_readd_keeps_sdk_name_order_until_iteration() {
    let values = admin::normalize(vec![
        json!({"name":"first","value":"expired","domain":"example.com","path":"/","expires":0}),
        json!({"name":"second","value":"live","domain":"example.com","path":"/"}),
    ])
    .unwrap();
    let replacement = admin::normalize(vec![
        json!({"name":"first","value":"replacement","domain":"example.com","path":"/"}),
    ])
    .unwrap();
    let mut before_read = Cookies::initialize(&values).unwrap();
    before_read.merge(&replacement).unwrap();
    assert_eq!(
        before_read
            .snapshot()
            .iter()
            .map(|cookie| cookie["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    let mut after_read = Cookies::initialize(&values).unwrap();
    assert_eq!(after_read.snapshot().len(), 1);
    after_read.merge(&replacement).unwrap();
    assert_eq!(
        after_read
            .snapshot()
            .iter()
            .map(|cookie| cookie["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );
}

#[test]
fn cookie_filter_expired_name_history_stays_bounded_and_iteration_recovers_capacity() {
    let values=(0..256).map(|index|json!({"name":format!("expired{index}"),"value":"x","domain":"example.com","path":"/","expires":0,"httpOnly":false,"secure":false,"sameSite":"Lax"})).collect::<Vec<_>>();
    let mut jar = Cookies::initialize(&values).unwrap();
    assert_eq!(jar.groups.len(), 256);
    assert!(jar.store.is_empty());
    let next = admin::normalize(vec![
        json!({"name":"next","value":"live","domain":"example.com","path":"/"}),
    ])
    .unwrap();
    assert_eq!(
        jar.merge(&next).unwrap_err().code,
        "HTTP_STORAGE_COOKIE_BOUNDS"
    );
    assert_eq!(jar.groups.len(), 256);
    assert!(jar.snapshot().is_empty());
    jar.merge(&next).unwrap();
    assert_eq!(jar.snapshot()[0]["name"], "next");
    let mut bytes=Cookies::initialize(&[json!({"name":"a".repeat(64000),"value":"expired","domain":"example.com","path":"/","expires":0,"httpOnly":false,"secure":false,"sameSite":"Lax"})]).unwrap();
    let extra=admin::normalize(vec![json!({"name":"b".repeat(2000),"value":"expired","domain":"example.com","path":"/","expires":0})]).unwrap();
    assert_eq!(
        bytes.merge(&extra).unwrap_err().code,
        "HTTP_STORAGE_COOKIE_BOUNDS"
    );
    assert_eq!(bytes.groups.len(), 1);
}

#[test]
fn cookie_filter_no_match_prunes_empty_groups_but_preserves_expired_unmatched_members() {
    let values = admin::normalize(vec![
        json!({"name":"first","value":"was-live","domain":"example.com","path":"/"}),
        json!({"name":"second","value":"live","domain":"example.com","path":"/"}),
    ])
    .unwrap();
    let mut retained = Cookies::initialize(&values).unwrap();
    let key = ("example.com".into(), "/".into(), "first".into());
    retained.store.get_mut(&key).unwrap()["expires"] = json!(0);
    retained.remove_selected(&[]);
    retained
        .merge(
            &admin::normalize(vec![
                json!({"name":"third","value":"added","domain":"example.com","path":"/"}),
            ])
            .unwrap(),
        )
        .unwrap();
    retained
        .merge(
            &admin::normalize(vec![
                json!({"name":"first","value":"readded","domain":"example.com","path":"/"}),
            ])
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        retained
            .snapshot()
            .iter()
            .map(|cookie| cookie["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
    let mut empty=Cookies::initialize(&[json!({"name":"first","value":"expired","domain":"example.com","path":"/","expires":0,"httpOnly":false,"secure":false,"sameSite":"Lax"}),values[1].clone()]).unwrap();
    empty.remove_selected(&[]);
    empty
        .merge(
            &admin::normalize(vec![
                json!({"name":"first","value":"readded","domain":"example.com","path":"/"}),
            ])
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        empty
            .snapshot()
            .iter()
            .map(|cookie| cookie["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );
}

#[tokio::test]
async fn cookie_filter_context_refusal_preserves_values_and_removal_uses_the_exact_scope_tuple() {
    let mut contexts = Contexts::default();
    let created = contexts.create(Uuid::new_v4(), Options::default()).unwrap();
    let reference: Reference = serde_json::from_value(created["httpContext"].clone()).unwrap();
    contexts
        .cookies_add(
            reference,
            vec![
                json!({"name":"Acme","value":"root","domain":"example.com","path":"/"}),
                json!({"name":"Acme","value":"child","domain":"example.com","path":"/acme"}),
                json!({"name":"Other","value":"kept","domain":"example.com","path":"/"}),
            ],
        )
        .unwrap();
    let values = contexts.cookie_filter_snapshot(reference).unwrap();
    let invalid = serde_json::from_value(json!({"name":{"regex":"["}})).unwrap();
    assert_eq!(
        super::super::cookie_filter::select(
            &values,
            invalid,
            Instant::now() + Duration::from_secs(2)
        )
        .await
        .unwrap_err()
        .code,
        "HTTP_COOKIE_REGEX_INVALID"
    );
    assert_eq!(contexts.cookie_filter_snapshot(reference).unwrap(), values);
    let filter = serde_json::from_value(json!({"name":"Acme","path":"/"})).unwrap();
    let indices = super::super::cookie_filter::select(
        &values,
        filter,
        Instant::now() + Duration::from_secs(2),
    )
    .await
    .unwrap();
    let selected = super::super::cookie_filter::selected(values, indices).unwrap();
    assert_eq!(
        contexts
            .cookies_remove_selected(reference, &selected)
            .unwrap()["matchedCookies"],
        1
    );
    let remaining = contexts.cookies_read(reference, &[]).unwrap();
    assert_eq!(remaining["cookies"].as_array().unwrap().len(), 2);
    assert_eq!(remaining["cookies"][0]["value"], "child");
    let foreign = Reference {
        connection_id: Uuid::new_v4(),
        ..reference
    };
    assert_eq!(
        contexts.cookie_filter_snapshot(foreign).unwrap_err().code,
        "FOREIGN_REFERENCE"
    );
    contexts.remove(reference).unwrap();
    assert_eq!(
        contexts.cookie_filter_snapshot(reference).unwrap_err().code,
        "STALE_REFERENCE"
    );
}

#[test]
fn cookie_admin_normalizes_url_defaults_and_rejects_invalid_input_before_mutation() {
    let browser = admin::normalize_browser(vec![
        json!({"name":"omitted","value":"attributes","url":"http://example.com/acme/page"}),
    ])
    .unwrap();
    assert_eq!(
        browser,
        vec![
            json!({"name":"omitted","value":"attributes","domain":"example.com","path":"/acme/","secure":false})
        ]
    );
    let values = admin::normalize(vec![json!({"name":"session","value":"secret","url":"https://example.com/acme/page?query=1","secure":false})]).unwrap();
    assert_eq!(
        values,
        vec![
            json!({"name":"session","value":"secret","domain":"example.com","path":"/acme/","secure":true,"expires":-1,"httpOnly":false,"sameSite":"Lax"})
        ]
    );
    for cookie in [
        json!({"name":"x","value":"1"}),
        json!({"name":"x","value":"1","url":"https://example.com/","path":"/"}),
        json!({"name":"x","value":"1","url":"https://example.com/","domain":"example.com"}),
        json!({"name":"x","value":"1","domain":"example.com"}),
        json!({"name":"x","value":"1","path":"/"}),
        json!({"name":"x","value":"1","url":"https://example.com/","sameSite":"Acme"}),
        json!({"name":"x","value":"1","url":"about:blank"}),
        json!({"name":"x","value":"1","url":"data:text/plain,acme"}),
        json!({"name":"x","value":"1","domain":"example.com","path":"/","expires":-2}),
        json!({"name":"x","value":"1","domain":"example.com","path":"/","expires":253402300800_u64}),
        json!({"name":"x","value":"bad;injection=1","url":"https://example.com/"}),
    ] {
        assert!(admin::normalize(vec![cookie]).is_err());
    }
}

#[test]
fn cookie_admin_read_url_matching_is_distinct_from_outgoing_cookie_selection() {
    let values = admin::normalize(vec![
        json!({"name":"scope","value":"a","domain":"example.com","path":"/acme","secure":true}),
        json!({"name":"local","value":"b","domain":"localhost","path":"/","secure":true}),
    ])
    .unwrap();
    let mut jar = Cookies::initialize(&values).unwrap();
    let url = Url::parse("https://child.example.com/acme-extra").unwrap();
    assert_eq!(
        admin::select(json!(jar.snapshot()), std::slice::from_ref(&url)).unwrap()["cookies"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(jar.header(&url), "");
    assert!(
        admin::select(
            json!(jar.snapshot()),
            &[Url::parse("http://example.com/acme").unwrap()]
        )
        .unwrap()["cookies"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        admin::select(
            json!(jar.snapshot()),
            &[Url::parse("http://localhost/").unwrap()]
        )
        .unwrap()["cookies"][0]["name"],
        "local"
    );
    assert_eq!(
        CookieUrls::One("https://example.com/acme".into())
            .parse()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(CookieUrls::default().parse().unwrap().len(), 0);
    assert!(CookieUrls::One("relative/path".into()).parse().is_err());
}

#[test]
fn cookie_admin_merge_replaces_in_name_order_and_bounds_failure_is_atomic() {
    let values = admin::normalize(vec![
        json!({"name":"a","value":"first","domain":"example.com","path":"/"}),
        json!({"name":"b","value":"second","domain":"example.com","path":"/"}),
        json!({"name":"a","value":"third","domain":"example.com","path":"/acme"}),
    ])
    .unwrap();
    let mut jar = Cookies::initialize(&values).unwrap();
    jar.merge(
        &admin::normalize(vec![
            json!({"name":"a","value":"replacement","domain":"example.com","path":"/"}),
        ])
        .unwrap(),
    )
    .unwrap();
    let before = jar.snapshot();
    assert_eq!(
        before
            .iter()
            .map(|value| value["value"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["third", "replacement", "second"]
    );
    let extra = (0..255).map(|index|json!({"name":format!("extra{index}"),"value":"x","domain":"example.com","path":"/","expires":-1,"httpOnly":false,"secure":false,"sameSite":"Lax"})).collect::<Vec<_>>();
    assert_eq!(
        jar.merge(&extra).unwrap_err().code,
        "HTTP_STORAGE_COOKIE_BOUNDS"
    );
    assert_eq!(jar.snapshot(), before);
    jar.merge(
        &admin::normalize(vec![
            json!({"name":"a","value":"expired","domain":"example.com","path":"/","expires":0}),
        ])
        .unwrap(),
    )
    .unwrap();
    assert_eq!(jar.snapshot().len(), 2);
}
