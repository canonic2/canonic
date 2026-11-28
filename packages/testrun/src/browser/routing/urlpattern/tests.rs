use super::*;
use crate::browser::routing::Matcher;

fn pattern(value: serde_json::Value) -> Matcher {
    let mut value = value;
    value["kind"] = "url_pattern".into();
    serde_json::from_value(value).unwrap()
}
fn matches(value: serde_json::Value, url: &str) -> bool {
    pattern(value.clone())
        .compile()
        .unwrap_or_else(|code| panic!("{value}: {code}"))
        .is_match(url)
}

#[test]
fn component_patterns_match_like_the_pinned_url_pattern() {
    for (value, url, expected) in [
        // Named segments stop at `/`; omitted components match anything.
        (
            serde_json::json!({"pathname":"/api/:id"}),
            "https://example.com/api/42",
            true,
        ),
        (
            serde_json::json!({"pathname":"/api/:id"}),
            "https://example.com/api/42/x",
            false,
        ),
        (
            serde_json::json!({"pathname":"/api/:id"}),
            "https://example.com/api/",
            false,
        ),
        // Full wildcards cross segments; hostname segments stop at `.`.
        (
            serde_json::json!({"pathname":"/files/*"}),
            "https://example.com/files/a/b",
            true,
        ),
        (
            serde_json::json!({"hostname":"*.example.com"}),
            "https://cdn.example.com/x",
            true,
        ),
        (
            serde_json::json!({"hostname":"*.example.com"}),
            "https://example.com/x",
            false,
        ),
        (
            serde_json::json!({"hostname":"{:sub.}?example.com"}),
            "https://example.com/",
            true,
        ),
        (
            serde_json::json!({"hostname":"{:sub.}?example.com"}),
            "https://a.example.com/",
            true,
        ),
        // Custom regexp groups and modifiers.
        (
            serde_json::json!({"pathname":r"/items/:id(\d+)"}),
            "https://example.com/items/7",
            true,
        ),
        (
            serde_json::json!({"pathname":r"/items/:id(\d+)"}),
            "https://example.com/items/x",
            false,
        ),
        (
            serde_json::json!({"pathname":"/posts{/:slug}?"}),
            "https://example.com/posts",
            true,
        ),
        (
            serde_json::json!({"pathname":"/posts{/:slug}?"}),
            "https://example.com/posts/hi",
            true,
        ),
        (
            serde_json::json!({"pathname":"/tags/:tag+"}),
            "https://example.com/tags/a/b",
            true,
        ),
        (
            serde_json::json!({"pathname":"/tags/:tag+"}),
            "https://example.com/tags",
            false,
        ),
        (
            serde_json::json!({"pathname":"/tags/:tag*"}),
            "https://example.com/tags",
            true,
        ),
        (
            serde_json::json!({"pathname":r"/a\:b"}),
            "https://example.com/a:b",
            true,
        ),
        // Pinned `ignoreCase: true`.
        (
            serde_json::json!({"pathname":"/API"}),
            "https://example.com/api",
            true,
        ),
        // Init processing strips `:`, `?` and `#`; default ports are empty.
        (
            serde_json::json!({"protocol":"https:"}),
            "https://example.com/",
            true,
        ),
        (
            serde_json::json!({"protocol":"https:"}),
            "http://example.com/",
            false,
        ),
        (
            serde_json::json!({"search":"?q=:term"}),
            "https://example.com/?q=acme",
            true,
        ),
        // Search has no segment delimiter, so a named group takes the rest.
        (
            serde_json::json!({"search":"?q=:term"}),
            "https://example.com/?q=acme&x=1",
            true,
        ),
        (
            serde_json::json!({"search":"?q=:term"}),
            "https://example.com/?x=1",
            false,
        ),
        (
            serde_json::json!({"hash":"#top"}),
            "https://example.com/#top",
            true,
        ),
        (
            serde_json::json!({"port":"8080"}),
            "http://example.com:8080/",
            true,
        ),
        (
            serde_json::json!({"port":"80"}),
            "http://example.com/",
            false,
        ),
        (
            serde_json::json!({"port":""}),
            "https://example.com:443/",
            true,
        ),
        (
            serde_json::json!({"username":"acme","password":"*"}),
            "https://acme:x@example.com/",
            true,
        ),
    ] {
        assert_eq!(matches(value.clone(), url), expected, "{value} {url}");
    }
}

#[test]
fn malformed_and_unsupported_patterns_refuse_before_registration() {
    for value in [
        serde_json::json!({"pathname":"/:id/:id"}),
        serde_json::json!({"pathname":"/(foo"}),
        serde_json::json!({"pathname":"/((x))"}),
        serde_json::json!({"pathname":"/(?x)"}),
        serde_json::json!({"pathname":"/:"}),
        serde_json::json!({"pathname":"/{a"}),
        serde_json::json!({"pathname":"/a\\"}),
        serde_json::json!({"pathname":"/caf\u{e9}"}),
        serde_json::json!({"pathname":"/a".repeat(600)}),
    ] {
        assert_eq!(
            pattern(value.clone()).compile().err(),
            Some(INVALID),
            "{value}"
        );
    }
    assert_eq!(
        pattern(serde_json::json!({"pathname":"/((?=a)a)"}))
            .compile()
            .err(),
        Some("ROUTE_REGEX_UNSUPPORTED")
    );
    assert!(
        serde_json::from_value::<Matcher>(serde_json::json!({"kind":"url_pattern","path":"/"}))
            .is_err()
    );
}
