//! Pinned `BrowserContext.cookies`/`addCookies`/`clearCookies` on an owned
//! browser context, with keyset pages over cookie identities.
use super::super::{
    contract::Reference,
    http::{self, CookieFilter, CookieUrls},
    session::Error,
};
use serde_json::{Value, json};

/// A cookie's identity in its store: name, domain, path and partition,
/// whose Chromium form also carries the cross-site ancestor flag.
fn key(cookie: &Value) -> Result<Vec<u8>, Error> {
    let field = |name: &str| {
        cookie[name]
            .as_str()
            .ok_or(Error::from("HTTP_BROWSER_COOKIE_PROTOCOL"))
    };
    let identity = json!([
        field("name")?,
        field("domain")?,
        field("path")?,
        cookie["partitionKey"].as_str().unwrap_or(""),
        cookie["_crHasCrossSiteAncestor"].as_bool()
    ]);
    Ok(crate::pagination::identity_key(&identity.to_string()))
}

/// One page of the context's cookies matching `urls`, in identity order.
pub(crate) fn page(
    context: Reference,
    cookies: &Value,
    urls: &CookieUrls,
    cursor: Option<String>,
    limit: Option<u64>,
) -> Result<Value, Error> {
    let parsed = urls.parse()?;
    let cookies = cookies.as_array().ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
    let rows = http::cookie_admin::matching(cookies, &parsed)
        .into_iter()
        .map(|cookie| Ok((key(&cookie)?, cookie)))
        .collect::<Result<Vec<_>, Error>>()?;
    let (items, cursor) = super::keyset(
        "browser_send.context_cookies_read",
        &json!({"context":context,"urls":urls}),
        rows,
        |cookie| cookie,
        cursor.as_deref(),
        limit,
        "browser_send context_cookies_read with urls",
    )?;
    let next = cursor.as_ref().map(|cursor| {
        let mut next = json!({"operation":"context_cookies_read","context":context,"urls":urls,"cursor":cursor});
        if let Some(limit) = limit {
            next["limit"] = json!(limit);
        }
        next
    });
    Ok(
        json!({"context":context,"cookies":items,"nextCursor":cursor,"nextRequest":next,"secretBearing":true}),
    )
}

/// Validated cookies for `addCookies`; unset attributes keep browser defaults.
pub(crate) fn added(cookies: Vec<Value>) -> Result<Vec<Value>, Error> {
    http::cookie_admin::normalize_browser(cookies)
}

pub(crate) fn add_result(context: Reference, count: usize) -> Value {
    json!({"context":context,"cookiesAdded":count})
}
pub(crate) fn clear_result(context: Reference, matched: Option<usize>) -> Value {
    json!({"context":context,"cookiesCleared":true,"filtered":matched.is_some(),"matchedCookies":matched})
}

/// The cookies a filter selects, expired as the pinned server clears them.
pub(crate) async fn expired(
    values: Vec<Value>,
    options: CookieFilter,
    deadline: tokio::time::Instant,
) -> Result<Vec<Value>, Error> {
    let indices = http::cookie_filter::select(&values, options, deadline).await?;
    let mut selected = http::cookie_filter::selected(values, indices)?;
    for cookie in &mut selected {
        cookie["expires"] = json!(0);
    }
    Ok(selected)
}

/// The pinned `cookies(urls)` selector, shared with HTTP contexts.
pub(crate) fn urls_schema() -> Value {
    json!({"anyOf":[{"type":"string","maxLength":4096},{"type":"array","maxItems":128,"items":{"type":"string","maxLength":4096}}]})
}
pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let urls = urls_schema();
    let mut read =
        json!({"operation":{"const":"context_cookies_read"},"context":reference,"urls":urls});
    read.as_object_mut().expect("read properties").extend(
        crate::pagination::properties(canonic_mcp_kit::page::Limits::STANDARD)
            .as_object()
            .expect("page properties")
            .clone(),
    );
    vec![
        json!({"type":"object","properties":read,"required":["operation","context"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"context_cookies_add"},"context":reference,"cookies":{"type":"array","minItems":1,"maxItems":256,"items":http::cookie_admin::cookie_schema()}},"required":["operation","context","cookies"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"context_cookies_clear"},"context":reference,"options":CookieFilter::schema()},"required":["operation","context"],"additionalProperties":false}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn context() -> Reference {
        Reference {
            connection_id: Uuid::nil(),
            id: Uuid::nil(),
            generation: Uuid::nil(),
        }
    }
    fn cookie(name: &str, domain: &str, path: &str) -> Value {
        json!({"name":name,"value":"private","domain":domain,"path":path,"expires":-1,"httpOnly":false,"secure":false,"sameSite":"Lax"})
    }

    #[test]
    fn context_cookie_pages_select_urls_and_keep_identity_order() {
        let cookies = json!([
            cookie("b", "example.com", "/"),
            cookie("a", "example.com", "/acme"),
            cookie("a", "example.com", "/"),
            cookie("other", "example.org", "/")
        ]);
        let all = page(context(), &cookies, &CookieUrls::default(), None, None).unwrap();
        let names = |value: &Value| {
            value["cookies"]
                .as_array()
                .unwrap()
                .iter()
                .map(|cookie| {
                    format!(
                        "{}{}",
                        cookie["name"].as_str().unwrap(),
                        cookie["path"].as_str().unwrap()
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&all), ["a/", "a/acme", "b/", "other/"]);
        assert_eq!(all["secretBearing"], true);
        let urls = CookieUrls::One("https://example.com/".into());
        let first = page(context(), &cookies, &urls, None, Some(1)).unwrap();
        assert_eq!(names(&first), ["a/"]);
        assert_eq!(first["nextRequest"]["urls"], "https://example.com/");
        let cursor = first["nextCursor"].as_str().unwrap().to_owned();
        let rest = page(context(), &cookies, &urls, Some(cursor.clone()), Some(10)).unwrap();
        assert_eq!(names(&rest), ["b/"]);
        assert!(rest["nextCursor"].is_null());
        // A cursor is bound to its URL selection.
        assert_eq!(
            page(
                context(),
                &cookies,
                &CookieUrls::default(),
                Some(cursor),
                None
            )
            .unwrap_err()
            .code,
            "invalid_cursor"
        );
        assert!(
            page(
                context(),
                &json!([{"name":"x"}]),
                &CookieUrls::default(),
                None,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn context_cookie_partitions_are_distinct_identities() {
        let mut partitioned = cookie("a", "example.com", "/");
        partitioned["partitionKey"] = json!("https://example.org");
        assert_ne!(
            key(&partitioned).unwrap(),
            key(&cookie("a", "example.com", "/")).unwrap()
        );
        // Chromium partitions that differ only in the ancestor flag are both
        // reached when a page boundary separates them.
        let mut cross_site = partitioned.clone();
        cross_site["_crHasCrossSiteAncestor"] = json!(true);
        partitioned["_crHasCrossSiteAncestor"] = json!(false);
        let cookies = json!([cross_site, partitioned]);
        let urls = CookieUrls::default();
        let first = page(context(), &cookies, &urls, None, Some(1)).unwrap();
        let cursor = first["nextCursor"].as_str().unwrap().to_owned();
        let rest = page(context(), &cookies, &urls, Some(cursor), Some(1)).unwrap();
        let mut ancestors = [
            &first["cookies"][0]["_crHasCrossSiteAncestor"],
            &rest["cookies"][0]["_crHasCrossSiteAncestor"],
        ]
        .map(|flag| flag.as_bool().unwrap());
        ancestors.sort_unstable();
        assert_eq!(ancestors, [false, true]);
        assert!(rest["nextCursor"].is_null());
    }

    #[tokio::test]
    async fn context_cookie_filters_expire_only_selected_cookies() {
        let values = vec![
            cookie("acme", "example.com", "/"),
            cookie("other", "example.com", "/"),
        ];
        let selected = expired(
            values,
            serde_json::from_value(json!({"name":{"regex":"^ac"}})).unwrap(),
            tokio::time::Instant::now() + std::time::Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0]["name"], "acme");
        assert_eq!(selected[0]["expires"], 0);
    }
}
