//! Typed socket URL matchers retain normalized globs and share bounded regex preparation.
use super::*;
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Matcher {
    Exact { url: String },
    Glob { pattern: String },
    Regex(RegularExpression),
    UrlPattern(SocketPattern),
}
/// A URLPattern whose per-component page programs are prepared once.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    from = "crate::browser::routing::UrlPattern",
    into = "crate::browser::routing::UrlPattern"
)]
pub struct SocketPattern {
    pattern: crate::browser::routing::UrlPattern,
    prepared: Arc<OnceLock<Result<Arc<Value>, &'static str>>>,
}
impl From<crate::browser::routing::UrlPattern> for SocketPattern {
    fn from(pattern: crate::browser::routing::UrlPattern) -> Self {
        Self {
            pattern,
            prepared: Arc::default(),
        }
    }
}
impl From<SocketPattern> for crate::browser::routing::UrlPattern {
    fn from(socket: SocketPattern) -> Self {
        socket.pattern
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegularExpression {
    pattern: String,
    #[serde(default)]
    flags: String,
    #[serde(skip)]
    prepared: Arc<OnceLock<Result<Arc<Value>, &'static str>>>,
}
impl RegularExpression {
    fn prepare(&self) -> Result<Arc<Value>, &'static str> {
        self.prepared
            .get_or_init(|| super::automaton::compile(&self.pattern, &self.flags).map(Arc::new))
            .clone()
    }
}
fn http(value: &str) -> Result<String, &'static str> {
    if let Some(rest) = value.strip_prefix("wss://") {
        Ok(format!("https://{rest}"))
    } else if let Some(rest) = value.strip_prefix("ws://") {
        Ok(format!("http://{rest}"))
    } else {
        Err("WEBSOCKET_URL_INVALID")
    }
}
/// Pinned `toWebSocketBaseUrl`: an HTTP(S) context base resolves socket URLs as WS(S).
pub(crate) fn websocket_base(base: &url::Url) -> url::Url {
    let scheme = match base.scheme() {
        "http" => "ws",
        "https" => "wss",
        _ => return base.clone(),
    };
    let mut converted = base.clone();
    // HTTP(S) and WS(S) are special schemes, so WHATWG permits this conversion.
    if converted.set_scheme(scheme).is_err() {
        return base.clone();
    }
    converted
}
impl Matcher {
    /// Resolve exact URLs and non-`*` globs against the context `baseURL`.
    pub(crate) fn resolve(&mut self, base: &url::Url) {
        let base = websocket_base(base);
        match self {
            Self::Exact { url } => *url = crate::browser::routing::join_base(&base, url),
            Self::Glob { pattern } => {
                *pattern = crate::browser::routing::normalize_glob(pattern, Some(&base));
            }
            Self::Regex(_) | Self::UrlPattern(_) => {}
        }
    }
    pub(crate) fn prepare(&self) -> Result<Arc<Value>, &'static str> {
        match self {
            Self::Regex(expression) => return expression.prepare(),
            Self::UrlPattern(socket) => {
                return socket
                    .prepared
                    .get_or_init(|| {
                        super::automaton::compile_pattern(&socket.pattern).map(Arc::new)
                    })
                    .clone();
            }
            Self::Exact { .. } | Self::Glob { .. } => {}
        }
        Ok(Arc::new(json!({"regexSource":self.source()?})))
    }
    fn source(&self) -> Result<String, &'static str> {
        let matcher = match self {
            Self::Exact { url } => {
                validate_url(url)?;
                let url = url::Url::parse(url).map_err(|_| "WEBSOCKET_URL_INVALID")?;
                crate::browser::routing::Matcher::Exact {
                    url: http(url.as_str())?,
                }
            }
            Self::Glob { pattern } => crate::browser::routing::Matcher::Glob {
                pattern: if pattern.starts_with("**") {
                    pattern.clone()
                } else {
                    http(pattern)?
                },
            },
            Self::Regex(_) | Self::UrlPattern(_) => return Err("WEBSOCKET_REGEX_UNSUPPORTED"),
        };
        matcher.compile().map_err(|_| "WEBSOCKET_PATTERN_INVALID")?;
        matcher.source().map_err(|_| "WEBSOCKET_PATTERN_INVALID")
    }
    pub(crate) fn schema() -> Value {
        json!({"oneOf":[{"type":"object","properties":{"kind":{"const":"exact"},"url":{"type":"string","maxLength":2048}},"required":["kind","url"],"additionalProperties":false},{"type":"object","properties":{"kind":{"const":"glob"},"pattern":{"type":"string","minLength":1,"maxLength":2048,"description":"ASCII absolute WS(S) or ** glob; * excludes slash, ** crosses it; query marks are literal"}},"required":["kind","pattern"],"additionalProperties":false},{"type":"object","properties":{"kind":{"const":"regex"},"pattern":{"type":"string","minLength":0,"maxLength":2048,"description":"JavaScript RegExp source tested against the canonical WS(S) URL"},"flags":{"type":"string","maxLength":8,"default":"","description":"JavaScript RegExp flags (d, g, i, m, s, u, v, y; y anchors at the URL start)"}},"required":["kind","pattern"],"additionalProperties":false},crate::browser::routing::UrlPattern::schema()]})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regex_preparation_is_shared_across_admission_clones_and_hidden_from_serialization() {
        let matcher: Matcher = serde_json::from_value(
            json!({"kind":"regex","pattern":r"^wss?://[^/]+/Acme/[0-9]+\?token=\w+$","flags":"i"}),
        )
        .unwrap();
        let clone = matcher.clone();
        let prepared = matcher.prepare().unwrap();
        assert!(Arc::ptr_eq(&prepared, &clone.prepare().unwrap()));
        assert!(prepared["automaton"].is_object());
        assert_eq!(
            serde_json::to_value(&matcher).unwrap(),
            json!({"kind":"regex","pattern":r"^wss?://[^/]+/Acme/[0-9]+\?token=\w+$","flags":"i"})
        );
        // Patterns the DFA cannot run reach the page's native RegExp unchanged.
        for (pattern, flags) in [
            ("(?=Acme)", ""),
            (r"(a)\1", ""),
            (r"(?<!x)Acme", "i"),
            ("東京", ""),
            (r"\p{L}+", "u"),
            ("[\\p{L}--[a-z]]", "v"),
        ] {
            let matcher: Matcher =
                serde_json::from_value(json!({"kind":"regex","pattern":pattern,"flags":flags}))
                    .unwrap();
            assert_eq!(
                *matcher.prepare().unwrap(),
                json!({"nativeRegex":{"source":pattern,"flags":flags}}),
                "{pattern}/{flags}"
            );
        }
        // Syntax JavaScript rejects stays refused even where the pinned engine is lenient.
        for (pattern, flags) in [
            ("Acme", "ii"),
            ("(", ""),
            ("Acme", "x"),
            (r"\012", "u"),
            (r"(A)\8", "u"),
            (r"(?<n>A)[\k]", ""),
            (r"[\k](?<n>A)", ""),
            (r"(?<n>A)\k<missing>", ""),
        ] {
            let matcher: Matcher =
                serde_json::from_value(json!({"kind":"regex","pattern":pattern,"flags":flags}))
                    .unwrap();
            assert_eq!(
                matcher.prepare().unwrap_err(),
                "WEBSOCKET_REGEX_INVALID",
                "{pattern}/{flags}"
            );
        }
        for pattern in ["", r"\101cme", r"\k"] {
            let matcher: Matcher =
                serde_json::from_value(json!({"kind":"regex","pattern":pattern})).unwrap();
            let clone = matcher.clone();
            let prepared = matcher.prepare().unwrap();
            assert!(prepared["automaton"].is_object(), "{pattern}");
            assert!(Arc::ptr_eq(&prepared, &clone.prepare().unwrap()));
        }
        let huge: Matcher =
            serde_json::from_value(json!({"kind":"regex","pattern":"a".repeat(2049)})).unwrap();
        assert_eq!(huge.prepare().unwrap_err(), "WEBSOCKET_REGEX_BOUNDS");
        assert!(
            serde_json::from_value::<Matcher>(
                json!({"kind":"regex","pattern":"Acme","prepared":{}})
            )
            .is_err()
        );
        // JavaScript-only syntax reaches the DFA translated.
        let translated: Matcher = serde_json::from_value(
            json!({"kind":"regex","pattern":r"/(?<kind>(?i:ACME))/\d{2}[^]q=a{b}","flags":"ud"}),
        )
        .unwrap();
        assert_eq!(
            translated.prepare().unwrap_err(),
            "WEBSOCKET_REGEX_INVALID",
            "Annex B braces need non-u mode"
        );
        let translated: Matcher = serde_json::from_value(
            json!({"kind":"regex","pattern":r"/(?<kind>(?i:ACME))/\d{2}[^]q=a{b}","flags":"d"}),
        )
        .unwrap();
        assert!(translated.prepare().unwrap()["automaton"].is_object());
    }

    #[test]
    fn url_pattern_preparation_caches_components_and_shares_the_table_budget() {
        // URL patterns export one program per non-wildcard component, cached.
        let pattern: Matcher = serde_json::from_value(
            json!({"kind":"url_pattern","hostname":"*.example.com","pathname":"/acme/:id"}),
        )
        .unwrap();
        let prepared = pattern.prepare().unwrap();
        assert!(Arc::ptr_eq(&prepared, &pattern.clone().prepare().unwrap()));
        let programs = prepared["urlPattern"].as_array().unwrap();
        assert_eq!(programs.len(), 8);
        assert_eq!(
            programs.iter().map(Value::is_object).collect::<Vec<_>>(),
            [false, false, false, true, false, true, false, false]
        );
        assert_eq!(
            serde_json::to_value(&pattern).unwrap(),
            json!({"kind":"url_pattern","hostname":"*.example.com","pathname":"/acme/:id"})
        );
        for invalid in [
            json!({"kind":"url_pattern","pathname":"/:id/:id"}),
            json!({"kind":"url_pattern","pathname":"/((?=a)a)"}),
        ] {
            let matcher: Matcher = serde_json::from_value(invalid.clone()).unwrap();
            assert_eq!(
                matcher.prepare().unwrap_err(),
                "WEBSOCKET_REGEX_UNSUPPORTED",
                "{invalid}"
            );
        }
        // All components share one regex's table budget.
        let long = "([a-z0-9]{1,500})";
        let single: Matcher =
            serde_json::from_value(json!({"kind":"url_pattern","hostname":long})).unwrap();
        single.prepare().unwrap();
        let oversized: Matcher = serde_json::from_value(json!({"kind":"url_pattern",
            "username":long,"password":long,"hostname":long,"port":long,
            "pathname":long,"search":long,"hash":long}))
        .unwrap();
        assert_eq!(oversized.prepare().unwrap_err(), "WEBSOCKET_REGEX_BOUNDS");
        // A DFA over its table bounds still runs, as the SDK would, natively.
        let explosive: Matcher =
            serde_json::from_value(json!({"kind":"regex","pattern":"[ab]*a[ab]{20}$"})).unwrap();
        assert!(explosive.prepare().unwrap()["nativeRegex"].is_object());
    }
    #[test]
    fn socket_matchers_normalize_exact_urls_and_preserve_query_and_glob_operators() {
        let source = Matcher::Exact {
            url: "wss://EXAMPLE.com:443/acme?token=one.two".into(),
        }
        .source()
        .unwrap();
        assert_eq!(
            source,
            format!(
                "^{}$",
                regex::escape("https://example.com/acme?token=one.two")
            )
        );
        let source = Matcher::Glob {
            pattern: "wss://EXAMPLE.com:443/**/acme?token=*".into(),
        }
        .source()
        .unwrap();
        assert!(
            source.starts_with("^(?=https?://)https://example\\.com/"),
            "{source}"
        );
        assert!(source.ends_with("acme\\?token=([^/]*)$"), "{source}");
        assert!(
            Matcher::Glob {
                pattern: "**/acme/*?token=*".into()
            }
            .source()
            .is_ok()
        );
        for pattern in [
            "http://example.com/*",
            "file://*",
            "wss://Acme:private@example.com/*",
            "**/{a,{b}}",
        ] {
            assert!(
                Matcher::Glob {
                    pattern: pattern.into()
                }
                .source()
                .is_err(),
                "{pattern}"
            );
        }
    }
}
