//! Bounded URL matching shared by driver filtering and native callback admission.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Matcher {
    Exact {
        url: String,
    },
    Glob {
        pattern: String,
    },
    Regex {
        pattern: String,
        #[serde(default)]
        flags: String,
    },
    /// Pinned `URLPattern` component patterns.
    UrlPattern(super::urlpattern::UrlPattern),
}
pub(crate) enum Compiled {
    Regex(regex::bytes::Regex),
    /// Backtracking ECMAScript features through the pinned engine.
    Ecma(super::ecma::Regexp),
    Pattern(Box<super::urlpattern::Compiled>),
}
impl Compiled {
    pub(crate) fn is_match(&self, url: &str) -> bool {
        url.is_ascii()
            && (url.starts_with("http://") || url.starts_with("https://"))
            && match self {
                Self::Regex(regex) => regex.is_match(url.as_bytes()),
                Self::Ecma(regex) => regex.is_match(url),
                Self::Pattern(pattern) => pattern.is_match(url),
            }
    }
}
impl Matcher {
    pub(crate) fn schema() -> Value {
        json!({"oneOf":[
            {"type":"object","properties":{"kind":{"const":"exact"},"url":{"type":"string","maxLength":2048}},"required":["kind","url"],"additionalProperties":false},
            {"type":"object","properties":{"kind":{"const":"glob"},"pattern":{"type":"string","minLength":1,"maxLength":2048,"description":"ASCII absolute HTTP(S) or ** URL glob; * excludes slash, ** crosses it; ? is literal"}},"required":["kind","pattern"],"additionalProperties":false},
            {"type":"object","properties":{"kind":{"const":"regex"},"pattern":{"type":"string","minLength":0,"maxLength":2048,"description":"JavaScript RegExp source; a URL test exceeding 50 ms or the engine's memory bound does not match"},"flags":{"type":"string","maxLength":8,"default":"","description":"JavaScript RegExp flags from d, g, i, m, s, u, v and y (y anchors at the URL start)"}},"required":["kind","pattern"],"additionalProperties":false},
            super::urlpattern::UrlPattern::schema()
        ]})
    }
    pub(crate) fn original(&self) -> &str {
        match self {
            Self::Exact { url } => url,
            Self::Glob { pattern } => pattern,
            Self::Regex { pattern, .. } => pattern,
            Self::UrlPattern(pattern) => pattern.pathname.as_deref().unwrap_or("*"),
        }
    }
    pub(crate) fn source(&self) -> Result<String, &'static str> {
        if matches!(self, Self::Regex { .. } | Self::UrlPattern(_)) {
            // Logical regex and URLPattern matching stay in Rust. Both browser
            // transports admit broad candidates; the retained owner privately
            // continues nonmatches.
            self.compile()?;
            return Ok("^https?://".into());
        }
        let source = self.regular_source()?;
        Ok(if matches!(self, Self::Glob { .. }) {
            format!(
                "^(?=https?://){}",
                source.strip_prefix('^').expect("Anchored URL matcher")
            )
        } else {
            source
        })
    }
    fn regular_source(&self) -> Result<String, &'static str> {
        match self {
            Self::Regex { pattern, flags } => super::regexp::translate(pattern, flags),
            Self::UrlPattern(_) => unreachable!("URLPattern compiles per component"),
            Self::Exact { url } => {
                validate_url(url)?;
                Ok(format!("^{}$", regex::escape(url)))
            }
            Self::Glob { pattern } => {
                if pattern.is_empty()
                    || pattern.len() > 2048
                    || !pattern.is_ascii()
                    || pattern.chars().any(char::is_control)
                    || !(pattern.starts_with("http://")
                        || pattern.starts_with("https://")
                        || pattern.starts_with("**"))
                    || pattern.split_once("://").is_some_and(|(_, rest)| {
                        rest.split('/')
                            .next()
                            .is_some_and(|host| host.contains('@'))
                    })
                {
                    return Err("ROUTE_PATTERN_INVALID");
                }
                glob(&normalize(pattern, None))
            }
        }
    }
    /// Resolve exact URLs and non-`*` globs against a context `baseURL` like the
    /// pinned client; regexes and URL patterns never use the base.
    pub(crate) fn resolve(&mut self, base: &url::Url) {
        match self {
            Self::Exact { url } => *url = join(base, url),
            Self::Glob { pattern } => *pattern = normalize(pattern, Some(base)),
            Self::Regex { .. } | Self::UrlPattern(_) => {}
        }
    }
    pub(crate) fn compile(&self) -> Result<Compiled, &'static str> {
        if let Self::UrlPattern(pattern) = self {
            return pattern
                .compile()
                .map(|compiled| Compiled::Pattern(Box::new(compiled)));
        }
        if let Self::Regex { pattern, flags } = self {
            // The linear translation stays the fast path; patterns it cannot run
            // use the pinned backtracking engine with a per-URL budget.
            let ecma = pattern.len() <= 2048
                && flags.len() <= 8
                && (!pattern.is_ascii()
                    || super::regexp::translate(pattern, flags) == Err("ROUTE_REGEX_UNSUPPORTED"));
            if ecma {
                return super::ecma::Regexp::compile(pattern, flags).map(Compiled::Ecma);
            }
        }
        let mut builder = regex::bytes::RegexBuilder::new(&self.regular_source()?);
        if let Self::Regex { flags, .. } = self {
            builder
                .unicode(false)
                .case_insensitive(flags.contains('i'))
                .multi_line(flags.contains('m'))
                .dot_matches_new_line(flags.contains('s'));
        }
        builder
            .size_limit(65536)
            .build()
            .map(Compiled::Regex)
            .map_err(|_| "ROUTE_PATTERN_BOUNDS")
    }
    pub(crate) fn native_patterns(&self) -> Vec<String> {
        match self {
            Self::Exact { url } => vec![url.clone()],
            Self::Regex { .. } | Self::UrlPattern(_) => vec!["http://*".into(), "https://*".into()],
            Self::Glob { pattern } if pattern.starts_with("http://") => vec!["http://*".into()],
            Self::Glob { pattern } if pattern.starts_with("https://") => vec!["https://*".into()],
            Self::Glob { .. } => vec!["http://*".into(), "https://*".into()],
        }
    }
}
/// Pinned `constructURLBasedOnBaseURL`: unresolvable input stays unchanged for
/// the caller's existing validation to refuse.
pub(crate) fn join(base: &url::Url, url: &str) -> String {
    base.join(url)
        .map_or_else(|_| url.to_owned(), |url| url.to_string())
}
/// Pinned `resolveGlobBase`, optionally against a context `baseURL`.
pub(crate) fn normalize(pattern: &str, base: Option<&url::Url>) -> String {
    if pattern.starts_with('*') {
        return pattern.into();
    }
    // Protect glob-bearing segments while WHATWG URL normalization handles
    // literal hosts, ports and paths. Restore each token's original dialect.
    let pattern = pattern.replace("\\\\?", "?");
    let mut replacements = Vec::new();
    let mut token = |original: &str, replacement: String| {
        if original.is_empty() {
            return String::new();
        }
        replacements.push((replacement.clone(), original.to_owned()));
        replacement
    };
    let segments = pattern
        .split('/')
        .enumerate()
        .map(|(index, part)| {
            if matches!(part, "." | ".." | "") || index == 0 && part.ends_with(':') {
                return part.to_owned();
            }
            if !part.contains(['*', '?', '{', '}', '\\']) {
                return part.to_owned();
            }
            if let Some(question) = part.find('?') {
                let prefix = token(&part[..question], format!("$_{index}_$"));
                let suffix = token(&part[question..], format!("?$_{index}_$"));
                prefix + &suffix
            } else {
                token(part, format!("$_{index}_$"))
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    let parsed = match base {
        Some(base) => base.join(&segments),
        None => url::Url::parse(&segments),
    };
    let Ok(url) = parsed else {
        return pattern;
    };
    let origin = url.origin().ascii_serialization();
    let mut resolved = url.to_string();
    for (placeholder, original) in replacements {
        let replacement = if origin.contains(&placeholder) {
            original.to_ascii_lowercase()
        } else {
            original
        };
        resolved = resolved.replacen(&placeholder, &replacement, 1);
    }
    resolved
}
fn glob(pattern: &str) -> Result<String, &'static str> {
    // Pinned Playwright globToRegexPattern semantics; CDP's wildcard language
    // is broader, so native transports must additionally apply this matcher.
    let bytes = pattern.as_bytes();
    let mut result = String::from("^");
    let mut group = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && i + 1 < bytes.len() {
            i += 1;
            result.push_str(&regex::escape(&(bytes[i] as char).to_string()));
        } else if c == b'*' {
            let before = i.checked_sub(1).map(|i| bytes[i]);
            let mut count = 1;
            while i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                count += 1;
                i += 1;
            }
            if count > 1 {
                if bytes.get(i + 1) == Some(&b'/') {
                    result.push_str(if before == Some(b'/') {
                        "((.+/)|)"
                    } else {
                        "(.*/)"
                    });
                    i += 1;
                } else {
                    result.push_str("(.*)");
                }
            } else {
                result.push_str("([^/]*)");
            }
        } else {
            match c {
                b'{' => {
                    if group {
                        return Err("ROUTE_PATTERN_INVALID");
                    }
                    group = true;
                    result.push('(');
                }
                b'}' => {
                    if !group {
                        return Err("ROUTE_PATTERN_INVALID");
                    }
                    group = false;
                    result.push(')');
                }
                b',' if group => result.push('|'),
                _ => result.push_str(&regex::escape(&(c as char).to_string())),
            }
        }
        i += 1;
    }
    if group {
        return Err("ROUTE_PATTERN_INVALID");
    }
    result.push('$');
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regex_candidates_are_broad_but_owned_matching_is_ascii_and_flag_aware() {
        let matcher = Matcher::Regex {
            pattern: r"/Acme/(?:one|two)/[0-9]+\?token=\w+$".into(),
            flags: "i".into(),
        };
        let compiled = matcher.compile().unwrap();
        assert!(compiled.is_match("https://example.com/acme/TWO/42?token=Private_1"));
        for url in [
            "https://example.com/Acme/three/42?token=Private_1",
            "https://example.com/Acme/one/xx?token=Private_1",
            "https://example.com/Acme/one/42?token=東京",
            "ws://example.com/Acme/one/42?token=Private_1",
        ] {
            assert!(!compiled.is_match(url), "{url}");
        }
        assert_eq!(matcher.source().unwrap(), "^https?://");
        assert_eq!(matcher.native_patterns(), vec!["http://*", "https://*"]);
        let sensitive = Matcher::Regex {
            pattern: "Acme".into(),
            flags: String::new(),
        }
        .compile()
        .unwrap();
        assert!(sensitive.is_match("https://example.com/Acme"));
        assert!(!sensitive.is_match("https://example.com/acme"));
        // Rust's bounded automaton handles the nested repetition without a JS backtracking engine.
        let nested = Matcher::Regex {
            pattern: "(a+)+$".into(),
            flags: String::new(),
        }
        .compile()
        .unwrap();
        assert!(!nested.is_match(&format!("https://example.com/{}!", "a".repeat(20000))));
    }
    #[test]
    fn backtracking_regex_dialects_compile_and_invalid_sources_refuse_before_registration() {
        for (pattern, flags) in [
            ("(?=Acme)", ""),
            ("(?<=Acme)", ""),
            (r"(a)\1", ""),
            (r"\p{L}", "u"),
            (r"\u00E9", ""),
            ("Acme", "v"),
            ("東京", ""),
        ] {
            assert!(
                Matcher::Regex {
                    pattern: pattern.into(),
                    flags: flags.into()
                }
                .compile()
                .is_ok(),
                "{pattern}/{flags}"
            );
        }
        for (pattern, flags) in [("Acme", "ii"), ("(", ""), ("(?<=", ""), ("Acme", "uv")] {
            assert!(
                Matcher::Regex {
                    pattern: pattern.into(),
                    flags: flags.into()
                }
                .compile()
                .is_err(),
                "{pattern}/{flags}"
            );
        }
        for (pattern, flags, url, expected) in [
            ("(?=Acme)", "", "https://example.com/Acme", true),
            ("(?<=/)Acme$", "", "https://example.com/Acme", true),
            (r"/(a)\1$", "", "https://example.com/aa", true),
            (r"/(a)\1$", "", "https://example.com/ab", false),
            (r"\p{L}$", "u", "https://example.com/A", true),
            ("Acme", "v", "https://example.com/Acme", true),
        ] {
            let compiled = Matcher::Regex {
                pattern: pattern.into(),
                flags: flags.into(),
            }
            .compile()
            .unwrap();
            assert_eq!(compiled.is_match(url), expected, "{pattern}/{flags} {url}");
        }
        assert!(
            Matcher::Regex {
                pattern: "a".repeat(2049),
                flags: String::new()
            }
            .compile()
            .is_err()
        );
    }
    #[test]
    fn absolute_globs_normalize_literal_url_parts_without_rewriting_query_or_glob_operators() {
        for (pattern, url) in [
            (
                "https://EXAMPLE.com:443/Acme folder/**/item?Case=*",
                "https://example.com/Acme%20folder/item?Case=one",
            ),
            ("http://EXAMPLE.com:80", "http://example.com/"),
            (
                "https://{WWW,API}.EXAMPLE.com/{One,Two}",
                "https://api.example.com/Two",
            ),
            (
                "https://example.com/a/../{One,Two}?Case=*",
                "https://example.com/One?Case=two",
            ),
        ] {
            assert!(
                Matcher::Glob {
                    pattern: pattern.into()
                }
                .compile()
                .unwrap()
                .is_match(url),
                "{pattern}: {url}"
            );
        }
        assert!(
            !Matcher::Glob {
                pattern: "**/EXAMPLE.com/*".into()
            }
            .compile()
            .unwrap()
            .is_match("https://example.com/One")
        );
    }
    #[test]
    fn glob_slashes_alternatives_escapes_and_literal_queries_match_the_pinned_contract() {
        for (pattern, url, expected) in [
            ("**", "file:///Acme", false),
            ("**", "ws://example.com/Acme", false),
            ("**", "https://example.com/Acme", true),
            ("**/acme/*", "https://example.com/acme/one", true),
            ("**/acme/*", "https://example.com/acme/one/two", false),
            (
                "https://example.com/**/acme",
                "https://example.com/acme",
                true,
            ),
            (
                "https://example.com/**/acme",
                "https://example.com/a/b/acme",
                true,
            ),
            ("**/*.{png,jpg}", "https://example.com/a.png", true),
            ("**/*.{png,jpg}", "https://example.com/a.gif", false),
            ("**/acme?item=*", "https://example.com/acme?item=one", true),
            ("**/acme?item=*", "https://example.com/acmeXitem=one", false),
            (r"**/acme\*", "https://example.com/acme*", true),
            (r"**/acme\*", "https://example.com/acmeOther", false),
        ] {
            assert_eq!(
                Matcher::Glob {
                    pattern: pattern.into()
                }
                .compile()
                .unwrap()
                .is_match(url),
                expected,
                "{pattern}: {url}"
            );
        }
        for pattern in [
            "**/{a,{b}}",
            "**/a}",
            "**/{a",
            "file://*",
            "http://Acme:private@example.com/*",
        ] {
            assert!(
                Matcher::Glob {
                    pattern: pattern.into()
                }
                .compile()
                .is_err(),
                "{pattern}"
            );
        }
    }
}
